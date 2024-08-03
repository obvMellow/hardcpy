mod commands;
mod test;

use fdlimit::{raise_fd_limit, Outcome};
#[cfg(not(test))]
use indicatif_log_bridge::LogWrapper;

use crate::commands::*;
use clap::{Parser, Subcommand};
use colored::Colorize;
use indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget, ProgressStyle};
use ini::Ini;
use log::{error, info};
use std::collections::VecDeque;
use std::ffi::OsString;
use std::fs::{DirEntry, File, ReadDir};
use std::hash::{Hash, Hasher};
use std::io::{ErrorKind, Write};
use std::path::PathBuf;
use std::string::ToString;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use std::{fs, io};

#[derive(Clone)]
struct Conclusion {
    pub total_count: usize,
    pub error_count: usize,
    pub error_list: Vec<String>,
    pub total_size: FileSize,
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
    SoftDelete { id: String },
    /// Deletes the backup
    Delete { id: String },
    /// Reverts a backup. Copies destination to source, recovering the source
    Revert {
        id: String,

        #[arg(short, long)]
        multithread: bool,
    },
    /// Creates a backup
    Create {
        source: PathBuf,
        dest: PathBuf,

        #[arg(short, long)]
        multithread: bool,
    },
}

const SEPARATOR: char = '┇';

fn main() {
    let mut config_dir = dirs::config_dir().unwrap_or_else(|| {
        println!(
            "{} Couldn't get a config directory, using current directory.",
            "[INFO]".bright_yellow()
        );
        std::env::current_dir().unwrap()
    });
    config_dir.push("hardcpy");
    fs::create_dir_all(&config_dir).unwrap();

    let mut config = Ini::load_from_file(config_dir.join("config.ini")).unwrap_or(Ini::new());

    let args = Args::parse();

    match args.command {
        Commands::List => list(config),
        Commands::SoftDelete { id } => soft_delete(config_dir, config, id),
        Commands::Delete { id } => delete(config_dir, config, id),
        Commands::Revert { id, multithread } => revert(config_dir, config, id, multithread),
        Commands::Create {
            source,
            dest,
            multithread,
        } => {
            _copy(config_dir, &mut config, multithread, source, dest);
        }
    }
}

fn _copy(
    config_dir: PathBuf,
    config: &mut Ini,
    is_multithread: bool,
    source_str: PathBuf,
    dest_str: PathBuf,
) -> bool {
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

    if is_multithread {
        conclusion = multithread(source, PathBuf::from(&dest_str), source_name.clone());
    } else {
        conclusion = singlethread(source, PathBuf::from(&dest_str), source_name.clone());
    }

    let v = format!(
        "{}{SEPARATOR}{}",
        source_str.display(),
        dest_str.join(source_name).to_str().unwrap()
    );
    let mut hasher = fnv::FnvHasher::default();
    v.hash(&mut hasher);
    config
        .with_section(Some("Backups"))
        .set(hasher.finish().to_string(), v);

    config.write_to_file(config_dir.join("config.ini")).unwrap();

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
        let mut log_file = File::create("hardcpy.log").unwrap();
        for err in conclusion.error_list {
            log_file.write_all(err.as_ref()).unwrap();
        }
    }
    false
}

fn singlethread(src: ReadDir, dest: PathBuf, src_name: OsString) -> Conclusion {
    let mut stack = VecDeque::new();
    stack.push_front(src);
    let mut file_list: VecDeque<(DirEntry, &OsString, &PathBuf)> = VecDeque::new();
    let mut error_count = 0;
    let mut error_list = Vec::new();
    let mut total_size = FileSize::new();
    let mut curr_progress = 0;

    let multi = MultiProgress::new();
    multi.set_move_cursor(true);

    #[cfg(not(test))]
    {
        let logger = colog::default_builder().build();
        LogWrapper::new(multi.clone(), logger).try_init().unwrap();
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

                                if let Err(e) = _copy_file(&f.0, f.1, f.2) {
                                    let err = format!(
                                        "Couldn't copy {:#?} because of error: {e}. Skipping\n",
                                        p
                                    );
                                    error!("{}", err);
                                    error_count += 1;
                                    error_list.push(err);
                                    continue;
                                }
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

        if let Err(e) = _copy_file(&f.0, f.1, f.2) {
            let err = format!("Couldn't copy {:#?} because of error: {e}. Skipping\n", p);
            error!("{}", err);
            error_count += 1;
            error_list.push(err);
            continue;
        }
        pb.inc(progress);
    }

    pb.finish();
    multi.remove(&pb);
    t.join().unwrap();

    total_size.update();
    return Conclusion {
        total_count,
        error_count,
        error_list,
        total_size,
    };
}

fn _pb_update(pb_clone: ProgressBar) -> JoinHandle<()> {
    std::thread::spawn(move || {
        while !pb_clone.is_finished() {
            pb_clone.tick();
            std::thread::sleep(Duration::from_millis(100));
        }
    })
}

fn multithread(src: ReadDir, dest: PathBuf, src_name: OsString) -> Conclusion {
    let conclusion = Arc::new(Mutex::new(Conclusion::new()));

    _multithread(src, dest, src_name, conclusion.clone());

    let guard = conclusion.lock().unwrap();
    let new = Conclusion {
        total_count: guard.total_count,
        error_count: guard.error_count,
        error_list: guard.error_list.clone(),
        total_size: guard.total_size,
    };
    return new;
}

fn _multithread(
    src: ReadDir,
    dest: PathBuf,
    src_name: OsString,
    conclusion: Arc<Mutex<Conclusion>>,
) {
    let files_list = Arc::new(Mutex::new(Vec::new()));
    let mut thread_pool = Vec::new();

    let multi = MultiProgress::with_draw_target(ProgressDrawTarget::stderr_with_hz(255));
    multi.set_move_cursor(true);

    #[cfg(not(test))]
    {
        let logger = colog::default_builder().build();
        LogWrapper::new(multi.clone(), logger).try_init().unwrap();
    }

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
        conclusion.clone(),
        files_list.clone(),
        pb.clone(),
    );
    pb.finish();
    t.join().unwrap();
    multi.remove(&pb);

    let total = conclusion.lock().unwrap().total_size.byte as u64;
    let pb = multi.add(ProgressBar::new(total));
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

    let files_list_clone = files_list.clone();
    let mut lock = files_list_clone.lock().unwrap();
    while let Some(e) = lock.pop() {
        let conclusion_clone = conclusion.clone();
        let pb_clone = pb.clone();
        thread_pool.push(std::thread::spawn(move || {
            let p = e.0.path();
            let progress = e.0.metadata().unwrap().len();

            info!("{} {:#?}", "Copying".green().bold(), p);

            if let Err(e) = _copy_file(&e.0, &e.1, &e.2) {
                let err = format!("Couldn't copy {:#?} because of error: {e}", p);
                error!("{}", err);
                let mut guard = conclusion_clone.lock().unwrap();
                guard.error_count += 1;
                guard.error_list.push(err);
                return;
            }
            pb_clone.inc(progress);
        }));
    }

    for thread in thread_pool {
        thread.join().unwrap();
    }
    pb.finish();
    t.join().unwrap();
}

fn _multithread_discover(
    src: ReadDir,
    dest: PathBuf,
    src_name: OsString,
    conclusion: Arc<Mutex<Conclusion>>,
    files_list: Arc<Mutex<Vec<(DirEntry, OsString, PathBuf)>>>,
    pb: ProgressBar,
) {
    let mut thread_pool = Vec::new();

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
                    conclusion.lock().unwrap().error_list.push(err);
                    continue;
                }
            };
            let dest = dest.clone();
            let src_name = src_name.clone();
            let conclusion_clone = conclusion.clone();
            let files_list_clone = files_list.clone();
            let pb_clone = pb.clone();
            thread_pool.push(std::thread::spawn(move || {
                _multithread_discover(
                    dir,
                    dest,
                    src_name,
                    conclusion_clone,
                    files_list_clone,
                    pb_clone,
                );
            }));
        }

        if entry.file_type().unwrap().is_file() {
            info!("{} {:#?}", "Discovered".green().bold(), entry.path());
            let mut lock = conclusion.lock().unwrap();
            lock.total_size.byte += entry.metadata().unwrap().len() as usize;
            lock.total_count += 1;
            files_list
                .lock()
                .unwrap()
                .push((entry, src_name.clone(), dest.clone()));
            pb.inc(1);
        }
    }

    for thread in thread_pool {
        thread.join().unwrap();
    }

    conclusion.lock().unwrap().total_size.update();
}

fn _copy_file(entry: &DirEntry, src_name: &OsString, dest: &PathBuf) -> io::Result<()> {
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
    Ok(())
}
