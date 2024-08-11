mod commands;
mod test;

use fdlimit::{raise_fd_limit, Outcome};
use indicatif_log_bridge::LogWrapper;
use rusqlite::{Connection, Transaction};
use sha2::{Digest, Sha256};

use crate::commands::*;
use clap::{Parser, Subcommand};
use colored::Colorize;
use indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget, ProgressStyle};
use log::{error, info};
use std::collections::VecDeque;
use std::ffi::OsString;
use std::fs::{DirEntry, File, ReadDir};
use std::hash::{Hash, Hasher};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::string::ToString;
use std::sync::mpsc;
use std::sync::mpsc::Sender;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use std::{fs, io};

#[derive(Clone)]
struct Conclusion {
    pub total_count: usize,
    pub error_count: usize,
    pub error_list: Vec<String>,
    pub total_size: FileSize,
    pub path_list: Vec<(PathBuf, PathBuf)>,
}

enum ConclusionFields {
    TotalCount(usize),
    Error(String),
    FileSize(FileSize),
    PathCouple((PathBuf, PathBuf)),
}

#[derive(Copy, Clone)]
struct FileSize {
    pub gb: usize,
    pub mb: usize,
    pub kb: usize,
    pub byte: usize,
}

impl Conclusion {
    pub fn new() -> Conclusion {
        Self {
            total_count: 0,
            error_count: 0,
            error_list: Vec::new(),
            total_size: FileSize::new(),
            path_list: Vec::new(),
        }
    }
}

impl FileSize {
    pub fn new() -> FileSize {
        Self {
            gb: 0,
            mb: 0,
            kb: 0,
            byte: 0,
        }
    }

    pub fn from_bytes(bytes: usize) -> Self {
        let mut o = Self::new();
        o.byte += bytes;
        o.update();
        o
    }

    pub fn update(&mut self) {
        self.kb = self.byte / 1024;
        self.mb = self.kb / 1024;
        self.gb = self.mb / 1024;
    }

    pub fn to_string(&self) -> String {
        let mut size_str = String::new();
        if self.gb != 0 {
            size_str += &format!("{:.2} GB", self.mb as f64 / 1024.0);
        } else if self.mb != 0 {
            size_str += &format!("{} MB", self.mb)
        } else if self.kb != 0 {
            size_str += &format!("{} KB", self.kb)
        } else {
            size_str += &format!("{} Bytes", self.byte)
        }
        size_str
    }
}

impl From<u64> for FileSize {
    fn from(value: u64) -> Self {
        let mut v = Self::new();
        v.byte = value as usize;
        v.update();
        v
    }
}

#[derive(Parser, Debug)]
#[command(version, about = "Simple backup tool written in Rust", long_about = None)]
struct Args {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Lists all backups saved
    List,
    /// Deletes the entry for a backup. Doesn't delete the actual files
    SoftDelete { id: u64 },
    /// Deletes the backup
    Delete { id: u64 },
    /// Reverts a backup. Copies destination to source, recovering the source
    Revert {
        id: u64,

        #[arg(short, long)]
        /// Enables multithreading. This feature is not complete and can be unstable
        multithread: bool,
    },
    /// Creates a backup
    Create {
        source: PathBuf,
        dest: PathBuf,

        #[arg(short, long)]
        /// Enables multithreading. This feature is not complete and can be unstable
        multithread: bool,
    },
    /// Verifies that the tracked source files match destination files
    Verify { id: u64 },
}

#[derive(Debug)]
struct BackupEntry {
    id: u64,
    from: PathBuf,
    to: PathBuf,
    compression: Option<String>,
}

#[derive(Debug)]
struct FileEntry {
    backup_id: u64,
    from: PathBuf,
    to: PathBuf,
    sha256: String,
}

fn main() {
    let args = Args::parse();

    let mut db_dir = dirs::config_dir().unwrap_or_else(|| {
        println!(
            "{} Couldn't get a config directory, using current directory.",
            "[INFO]".bright_yellow()
        );
        std::env::current_dir().unwrap()
    });
    db_dir.push("hardcpy");
    fs::create_dir_all(&db_dir).unwrap();

    let mut conn = Connection::open(db_dir.join("backups.db")).unwrap();
    let mut tx = conn.transaction().unwrap();

    tx.execute(
        "CREATE TABLE IF NOT EXISTS Backups (
            id INTEGER PRIMARY KEY,
            source TEXT NOT NULL,
            dest TEXT NOT NULL,
            compression TEXT
        )",
        (),
    )
    .unwrap();

    tx.execute(
        "CREATE TABLE IF NOT EXISTS Files (
            backup_id INTEGER NOT NULL,
            source TEXT NOT NULL,
            dest TEXT NOT NULL,
            sha256 TEXT NOT NULL,
            PRIMARY KEY (source, dest)
        )",
        (),
    )
    .unwrap();

    match args.command {
        Commands::List => list(&tx),
        Commands::SoftDelete { id } => soft_delete(&tx, id),
        Commands::Delete { id } => delete(&tx, id),
        Commands::Revert { id, multithread } => revert(&tx, id, multithread),
        Commands::Create {
            source,
            dest,
            multithread,
        } => {
            _copy(&tx, multithread, source, dest);
        }
        Commands::Verify { id } => verify(&mut tx, id),
    }
    tx.commit().unwrap();
}

fn _copy(conn: &Transaction, is_multithread: bool, source_str: PathBuf, dest_str: PathBuf) -> bool {
    let source_name = source_str.iter().last().unwrap().to_owned();

    let source = match fs::read_dir(&source_str) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("Error: {} (\"{}\")", e, source_str.display());
            return true;
        }
    };

    match fs::create_dir_all(&dest_str) {
        Ok(_) => {}
        Err(e) => {
            eprintln!(
                "{} {} (\"{}\")",
                "Error:".red().bold(),
                e,
                dest_str.display()
            );
            return true;
        }
    }

    let timer = Instant::now();
    let conclusion;
    let multi;

    if is_multithread {
        (conclusion, multi) = multithread(source, PathBuf::from(&dest_str), source_name.clone());
    } else {
        (conclusion, multi) = singlethread(source, PathBuf::from(&dest_str), source_name.clone());
    }

    let v = format!(
        "{}{}",
        source_str.display(),
        dest_str.join(source_name.clone()).to_str().unwrap()
    );
    let mut hasher = fnv::FnvHasher::default();
    v.hash(&mut hasher);
    let h = hasher.finish();
    conn.execute(
        "INSERT OR REPLACE INTO Backups (id, source, dest, compression) VALUES (?1, ?2, ?3, ?4)",
        (
            h as i64,
            source_str.display().to_string(),
            dest_str.display().to_string(),
            None::<String>,
        ),
    )
    .unwrap();

    multi.clear().unwrap();
    multi.set_move_cursor(true);

    let pb = multi.add(ProgressBar::new(conclusion.total_count as u64));

    pb.set_style(
        ProgressStyle::with_template(
            "{spinner:.green} {msg:.blue.bold} [{bar:50.cyan/blue}] {human_pos}/{human_len} [{elapsed_precise}] ({eta})",
        )
        .unwrap()
        .progress_chars("#>-"),
    );
    pb.set_message("Hashing");

    pb.set_position(0);

    let pb_clone = pb.clone();
    let t = _pb_update(pb_clone);

    if let Outcome::LimitRaised { from, to } = raise_fd_limit().unwrap() {
        info!("Increased max files open limit from {} to {}", from, to);
    }

    for (from, to) in conclusion.path_list {
        let mut read_from = File::open(from.clone()).unwrap();
        let mut hasher = Sha256::new();

        info!("{} \"{}\"", "Hashing".green().bold(), from.display());
        let file_size = read_from.metadata().unwrap().len();
        let max_buf_size = 1024 * 1024 * 1024 * 4;
        let buf_size = file_size.min(max_buf_size);
        let mut buf = Vec::with_capacity(buf_size as usize);
        while read_from.read_to_end(&mut buf).unwrap() > 0 {
            hasher.update(&buf);
        }

        conn.execute(
            r#"INSERT OR REPLACE INTO Files (backup_id, source, dest, sha256) VALUES (?1, ?2, ?3, ?4)
        "#,
            (
                h as i64,
                from.display().to_string(),
                to.display().to_string(),
                format!("{:x}", hasher.finalize()),
            ),
        )
        .unwrap();
        pb.inc(1);
    }
    pb.finish();
    multi.remove(&pb);
    t.join().unwrap();

    let pb = multi.add(ProgressBar::new(conclusion.total_count as u64));

    pb.set_style(
        ProgressStyle::with_template(
            "{spinner:.green} {msg:.blue.bold} [{bar:50.cyan/blue}] {human_pos}/{human_len} [{elapsed_precise}] ({eta})",
        )
        .unwrap()
        .progress_chars("#>-"),
    );
    pb.set_message("Verifying");

    pb.set_position(0);

    let pb_clone = pb.clone();
    let t = _pb_update(pb_clone);

    let mut stmt = conn
        .prepare("SELECT source, dest, sha256 FROM Files WHERE backup_id = ?1")
        .unwrap();
    let iter = stmt
        .query_map([h as i64], |row| {
            Ok(FileEntry {
                backup_id: h,
                from: row.get::<usize, String>(0).unwrap().into(),
                to: row.get::<usize, String>(1).unwrap().into(),
                sha256: row.get_unwrap(2),
            })
        })
        .unwrap();

    for entry in iter {
        let entry = entry.unwrap();
        let mut read_from = File::open(&entry.to).unwrap();
        let mut hasher = Sha256::new();

        info!(
            "{} \"{}\"",
            "Verifying".green().bold(),
            entry.to.display().to_string()
        );
        let file_size = read_from.metadata().unwrap().len();
        let max_buf_size = 1024 * 1024 * 1024 * 4;
        let buf_size = file_size.min(max_buf_size);
        let mut buf = Vec::with_capacity(buf_size as usize);
        while read_from.read_to_end(&mut buf).unwrap() > 0 {
            hasher.update(&buf);
        }

        if format!("{:x}", hasher.finalize()) != entry.sha256 {
            info!(
                "\n{} \"{}\"",
                "Copying".green().bold(),
                entry.to.display().to_string()
            );
            fs::copy(entry.from, entry.to).unwrap();
        }
        pb.inc(1);
    }
    pb.finish();
    t.join().unwrap();

    // Formatting the size info.
    let size_str = conclusion.total_size.to_string();

    // Formatting the elapsed time.
    let mut elapsed_str = String::new();
    let elapsed = timer.elapsed();
    let ms = elapsed.as_millis();
    let sec_f64 = elapsed.as_secs_f64();
    let sec = ms / 1000;
    let min = sec / 60;
    let hr = min / 60;

    if hr != 0 {
        elapsed_str += &format!("{} Hours {}", hr, min % 60);
    } else if min != 0 {
        elapsed_str += &format!("{} Minutes {} Seconds", min, sec % 60);
    } else {
        elapsed_str += &format!("{:.1} Seconds", sec_f64);
    }

    println!(
        "\n\n{} {} files {}{}{} in {} {}{}{}",
        "Copied".green().bold(),
        conclusion.total_count - conclusion.error_count,
        "(".truecolor(150, 150, 150),
        size_str.truecolor(150, 150, 150),
        ")".truecolor(150, 150, 150),
        elapsed_str,
        "(".truecolor(150, 150, 150),
        conclusion.error_count.to_string().truecolor(150, 150, 150),
        " errors)".truecolor(150, 150, 150),
    );

    if conclusion.error_list.len() > 0 {
        let log_folder = dirs::config_dir()
            .unwrap_or(std::env::current_dir().unwrap())
            .join("hardcpy/logs");
        fs::create_dir_all(&log_folder).unwrap();

        let path = log_folder.join(chrono::Local::now().to_rfc2822());
        let mut log_file = File::create(&path).unwrap();
        for err in conclusion.error_list {
            log_file
                .write_all(err.replace(" Skipping", "").as_ref())
                .unwrap();
        }

        error!("Errors were written to \"{}\"", path.display().to_string());
    }
    false
}

fn singlethread(src: ReadDir, dest: PathBuf, src_name: OsString) -> (Conclusion, MultiProgress) {
    let mut stack = VecDeque::new();
    stack.push_front(src);
    let mut file_list: VecDeque<(DirEntry, &OsString, &PathBuf)> = VecDeque::new();
    let mut error_count = 0;
    let mut error_list = Vec::new();
    let mut total_size = FileSize::new();
    let mut path_list = Vec::new();
    let mut curr_progress = 0;

    let multi = MultiProgress::new();
    multi.set_move_cursor(true);

    let logger = colog::default_builder().build();
    let _ = LogWrapper::new(multi.clone(), logger).try_init();

    let pb = multi.add(ProgressBar::new(u64::MAX));

    pb.set_style(
        ProgressStyle::with_template(
            "{spinner:.green} {msg:.blue.bold} {human_pos} files. [{elapsed_precise}]",
        )
        .unwrap()
        .progress_chars("#>-"),
    );
    pb.set_message("Discovered");

    pb.set_position(0);

    let pb_clone = pb.clone();
    let t = _pb_update(pb_clone);

    if let Outcome::LimitRaised { from, to } = raise_fd_limit().unwrap() {
        info!("Increased max files open limit from {} to {}", from, to);
    }

    while let Some(curr_dir) = stack.pop_front() {
        for entry in curr_dir {
            let entry = entry.unwrap();
            let entry_path = entry.path();

            if entry.file_type().unwrap().is_dir() {
                // If it's a directory, push its contents onto the stack
                let dir_content = match fs::read_dir(&entry_path) {
                    Ok(v) => v,
                    Err(e) => match e.raw_os_error().unwrap_or(0) {
                        // We copy the currently discovered files if we reach fd limit
                        24 => {
                            error!("Too many file handles open, switching to copying.");
                            for _ in 0..5 {
                                stack.pop_front();
                            }
                            let mut progress;
                            let total = total_size.byte;
                            let pb = multi.add(ProgressBar::new(total as u64));
                            pb.set_style(
                                ProgressStyle::with_template(
                                    "{spinner:.green} [{elapsed_precise}] [{bar:50.cyan/blue}] {bytes}/{total_bytes} ({eta})",
                                )
                                .unwrap()
                                .progress_chars("#>-"),
                            );

                            pb.set_position(curr_progress);

                            let pb_clone = pb.clone();
                            let t = _pb_update(pb_clone);

                            while let Some(f) = file_list.pop_front() {
                                let p = f.0.path();

                                progress = f.0.metadata().unwrap().len();
                                curr_progress += progress;
                                info!(
                                    "{} \"{}\" ({})",
                                    "Copying".green().bold(),
                                    p.display(),
                                    FileSize::from(progress).to_string().bold()
                                );

                                let dest_path = match _copy_file(&f.0, f.1, f.2) {
                                    Ok(v) => v,
                                    Err(e) => {
                                        let err = format!(
                                            "Couldn't copy {:#?} because of error: {e}. Skipping\n",
                                            p
                                        );
                                        error!("{}", err);
                                        error_count += 1;
                                        error_list.push(err);
                                        continue;
                                    }
                                };
                                path_list.push((p, dest_path));
                                pb.inc(progress);
                            }
                            pb.finish();
                            multi.remove(&pb);
                            t.join().unwrap();

                            continue;
                        }
                        _ => {
                            let err = format!(
                                "Couldn't read {:#?} because of error: {e}. Skipping",
                                entry_path
                            );
                            error!("{}", err);
                            continue;
                        }
                    },
                };
                stack.push_back(dir_content);
            } else if entry.file_type().unwrap().is_file() {
                // If it's a file, add to the list
                info!("{} {:#?}.", "Discovered".green().bold(), entry.path());

                total_size.byte += entry.metadata().unwrap().len() as usize;
                file_list.push_front((entry, &src_name, &dest));
                pb.inc(1);
            }
        }
    }

    pb.finish();
    t.join().unwrap();
    multi.remove(&pb);

    let total_count = file_list.len();
    let mut progress;
    let total = total_size.byte;

    let pb = multi.add(ProgressBar::new(total as u64));
    pb.set_style(
        ProgressStyle::with_template(
            "{spinner:.green} [{elapsed_precise}] [{bar:50.cyan/blue}] {bytes}/{total_bytes} ({eta})",
        )
        .unwrap()
        .progress_chars("#>-"),
    );

    pb.set_position(curr_progress);

    let pb_clone = pb.clone();
    let t = _pb_update(pb_clone);

    while let Some(f) = file_list.pop_front() {
        let p = f.0.path();

        progress = f.0.metadata().unwrap().len();
        info!(
            "{} \"{}\" ({})",
            "Copying".green().bold(),
            p.display(),
            FileSize::from(progress).to_string().bold()
        );

        let dest_path = match _copy_file(&f.0, f.1, f.2) {
            Ok(v) => v,
            Err(e) => {
                let err = format!("Couldn't copy {:#?} because of error: {e}. Skipping\n", p);
                error!("{}", err);
                error_count += 1;
                error_list.push(err);
                continue;
            }
        };
        path_list.push((p, dest_path));
        pb.inc(progress);
    }

    pb.finish();
    multi.remove(&pb);
    t.join().unwrap();

    total_size.update();
    return (
        Conclusion {
            total_count,
            error_count,
            error_list,
            total_size,
            path_list,
        },
        multi,
    );
}

fn _pb_update(pb_clone: ProgressBar) -> JoinHandle<()> {
    std::thread::spawn(move || {
        while !pb_clone.is_finished() {
            pb_clone.tick();
            std::thread::sleep(Duration::from_millis(100));
        }
    })
}

fn multithread(src: ReadDir, dest: PathBuf, src_name: OsString) -> (Conclusion, MultiProgress) {
    let mut conclusion = Conclusion::new();

    let multi = _multithread(src, dest, src_name, &mut conclusion);

    return (conclusion, multi);
}

fn _multithread(
    src: ReadDir,
    dest: PathBuf,
    src_name: OsString,
    conclusion: &mut Conclusion,
) -> MultiProgress {
    let mut files_list = Vec::new();
    let mut thread_pool = Vec::new();
    let (conclusion_send, conclusion_recv) = mpsc::channel();
    let (files_list_send, files_list_recv) = mpsc::channel();

    let multi = MultiProgress::with_draw_target(ProgressDrawTarget::stderr_with_hz(255));
    multi.set_move_cursor(true);

    let logger = colog::default_builder().build();
    let _ = LogWrapper::new(multi.clone(), logger).try_init();

    if let Outcome::LimitRaised { from, to } = raise_fd_limit().unwrap() {
        info!("Increased max files open limit from {} to {}", from, to);
    }
    let pb = multi.add(ProgressBar::new(u64::MAX));

    pb.set_style(
        ProgressStyle::with_template(
            "{spinner:.green} {msg:.blue.bold} {human_pos} files. [{elapsed_precise}]",
        )
        .unwrap()
        .progress_chars("#>-"),
    );
    pb.set_message("Discovered");

    pb.set_position(0);

    let pb_clone = pb.clone();
    let t = _pb_update(pb_clone);

    _multithread_discover(
        src,
        dest,
        src_name,
        conclusion_send,
        files_list_send,
        pb.clone(),
    );
    pb.finish();
    t.join().unwrap();
    multi.remove(&pb);

    while let Ok(v) = conclusion_recv.recv() {
        match v {
            ConclusionFields::TotalCount(x) => conclusion.total_count += x,
            ConclusionFields::Error(x) => {
                conclusion.error_count += 1;
                conclusion.error_list.push(x)
            }
            ConclusionFields::FileSize(x) => {
                conclusion.total_size.byte += x.byte;
                conclusion.total_size.update();
            }
            ConclusionFields::PathCouple(x) => conclusion.path_list.push(x),
        }
    }

    while let Ok(v) = files_list_recv.recv() {
        files_list.push(v);
    }

    let pb = multi.add(ProgressBar::new(conclusion.total_size.byte as u64));
    pb.set_style(
        ProgressStyle::with_template(
            "{spinner:.green} [{elapsed_precise}] [{bar:50.cyan/blue}] {bytes}/{total_bytes} ({eta})",
        )
        .unwrap()
        .progress_chars("#>-"),
    );

    pb.set_position(0);

    let pb_clone = pb.clone();
    let t = _pb_update(pb_clone);

    let (conclusion_send, conclusion_recv) = mpsc::channel();
    while let Some(e) = files_list.pop() {
        let conclusion_clone = conclusion_send.clone();
        let pb_clone = pb.clone();
        thread_pool.push(std::thread::spawn(move || {
            let p = e.0.path();
            let progress = e.0.metadata().unwrap().len();

            info!("{} {:#?}", "Copying".green().bold(), p);

            let t = match _copy_file(&e.0, &e.1, &e.2) {
                Ok(v) => v,
                Err(e) => {
                    let err = format!("Couldn't copy {:#?} because of error: {e}", p);
                    error!("{}", err);
                    conclusion_clone.send(ConclusionFields::Error(err)).unwrap();
                    return;
                }
            };
            conclusion_clone
                .send(ConclusionFields::PathCouple((p, t)))
                .unwrap();
            pb_clone.inc(progress);
        }));
    }
    drop(conclusion_send);

    while let Ok(v) = conclusion_recv.recv() {
        match v {
            ConclusionFields::TotalCount(x) => conclusion.total_count += x,
            ConclusionFields::Error(x) => {
                conclusion.error_count += 1;
                conclusion.error_list.push(x)
            }
            ConclusionFields::FileSize(x) => {
                conclusion.total_size.byte += x.byte;
                conclusion.total_size.update();
            }
            ConclusionFields::PathCouple(x) => conclusion.path_list.push(x),
        }
    }

    for thread in thread_pool {
        thread.join().unwrap();
    }
    pb.finish();
    t.join().unwrap();
    multi
}

fn _multithread_discover(
    src: ReadDir,
    dest: PathBuf,
    src_name: OsString,
    conclusion_chan: Sender<ConclusionFields>,
    files_list_chan: Sender<(DirEntry, OsString, PathBuf)>,
    pb: ProgressBar,
) {
    for f in src {
        let entry = f.unwrap();

        if entry.file_type().unwrap().is_dir() {
            let dir = match fs::read_dir(entry.path()) {
                Ok(v) => v,
                Err(e) => {
                    let err = format!(
                        "Couldn't read {:#?} because of error: {e}. Skipping\n",
                        entry.path()
                    );
                    error!("{}", err);
                    conclusion_chan.send(ConclusionFields::Error(err)).unwrap();
                    continue;
                }
            };
            let dest = dest.clone();
            let src_name = src_name.clone();
            let conclusion_clone = conclusion_chan.clone();
            let files_list_clone = files_list_chan.clone();
            let pb_clone = pb.clone();
            std::thread::spawn(move || {
                _multithread_discover(
                    dir,
                    dest,
                    src_name,
                    conclusion_clone,
                    files_list_clone,
                    pb_clone,
                );
            });
        }

        if entry.file_type().unwrap().is_file() {
            info!("{} {:#?}", "Discovered".green().bold(), entry.path());
            conclusion_chan
                .send(ConclusionFields::FileSize(FileSize::from_bytes(
                    entry.metadata().unwrap().len() as usize,
                )))
                .unwrap();
            conclusion_chan
                .send(ConclusionFields::TotalCount(1))
                .unwrap();
            files_list_chan
                .send((entry, src_name.clone(), dest.clone()))
                .unwrap();
            pb.inc(1);
        }
    }
}

fn _copy_file(entry: &DirEntry, src_name: &OsString, dest: &PathBuf) -> io::Result<PathBuf> {
    // Get the full path of the entry
    let full_path = entry.path();

    // Find the position of `src_name` in the full path
    let mut path = PathBuf::new();
    let mut found_src = false;

    for component in full_path.components() {
        match component {
            std::path::Component::Normal(x) if x == src_name => {
                found_src = true;
                path.push(x);
            }
            std::path::Component::Prefix(_)
            | std::path::Component::RootDir
            | std::path::Component::CurDir => {}
            _ => {
                // Once we've found `src_name`, push remaining components to `path`
                if found_src {
                    path.push(component.as_os_str());
                }
            }
        }
    }

    if path.components().count() == 0 {
        path.push("root/");
    }

    let mut dest_dir = dest.join(&path);
    dest_dir.pop(); // Pop the last element which is the file name.
    fs::create_dir_all(&dest_dir)?;

    let file_name = entry.file_name();
    let dest_path = dest_dir.join(file_name);
    fs::copy(&full_path, &dest_path)?;
    Ok(dest_path)
}
