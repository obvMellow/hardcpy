use crate::{BackupEntry, FileEntry, _copy, _pb_update};
use colored::Colorize;
use indicatif::{HumanCount, MultiProgress, ProgressBar, ProgressStyle};
use indicatif_log_bridge::LogWrapper;
use log::{error, info};
use rusqlite::{Result, Transaction};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::Read;

pub fn verify(conn: &Transaction, id: u64) {
    let mut error_list = Vec::new();
    let mut verified = 0;
    let mut real_count = 0;
    let mut copied = 0;
    let mut stmt = conn
        .prepare("SELECT source, dest, sha256 FROM Files WHERE backup_id = ?1")
        .unwrap();
    let iter = stmt
        .query_map([id as i64], |row| {
            Ok(FileEntry {
                backup_id: id,
                from: row.get::<usize, String>(0).unwrap().into(),
                to: row.get::<usize, String>(1).unwrap().into(),
                sha256: row.get_unwrap(2),
            })
        })
        .unwrap();
    let multi = MultiProgress::new();
    let logger = colog::default_builder().build();
    LogWrapper::new(multi.clone(), logger).try_init().unwrap();

    let pb = multi.add(ProgressBar::new(
        _count_matches(conn, id as i64).unwrap() as u64
    ));

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

    for entry in iter {
        real_count += 1;
        let entry = entry.unwrap();
        info!(
            "{} \"{}\"",
            "Verifying".green().bold(),
            entry.to.display().to_string()
        );
        let mut read_from = match File::open(&entry.to) {
            Ok(v) => v,
            Err(_) => {
                info!(
                    "\n{} {}",
                    "Copying".blue().bold(),
                    entry.from.display().to_string()
                );
                match fs::copy(&entry.from, &entry.to) {
                    Ok(v) => v,
                    Err(e) => {
                        error!("{e}");
                        error_list.push(e);
                        continue;
                    }
                };
                copied += 1;
                File::open(&entry.to).unwrap()
            }
        };
        let mut hasher = Sha256::new();

        let file_size = read_from.metadata().unwrap().len();
        let max_buf_size = 1024 * 1024 * 1024 * 4;
        let buf_size = file_size.min(max_buf_size);
        let mut buf = Vec::with_capacity(buf_size as usize);
        while read_from.read_to_end(&mut buf).unwrap() > 0 {
            hasher.update(&buf);
        }

        let hash = format!("{:x}", hasher.finalize());
        if hash != entry.sha256 {
            info!(
                "\n{} \"{}\"",
                "Copying".green().bold(),
                entry.to.display().to_string()
            );
            match fs::copy(entry.from, entry.to) {
                Ok(v) => v,
                Err(e) => {
                    error!("{e}");
                    error_list.push(e);
                    continue;
                }
            };
            copied += 1;
        }
        verified += 1;
        pb.inc(1);
    }
    pb.finish();
    t.join().unwrap();
    multi.remove(&pb);

    println!(
        "{} {} out of {} files. Copied {} files. ({} errors occured)",
        "Verified".green().bold(),
        HumanCount(verified).to_string(),
        HumanCount(real_count).to_string(),
        HumanCount(copied).to_string(),
        HumanCount(error_list.len() as u64).to_string(),
    );
}

fn _count_matches(conn: &Transaction, id: i64) -> Result<usize> {
    let mut stmt = conn.prepare("SELECT COUNT(*) FROM Files WHERE backup_id = ?1")?;
    let count: i64 = stmt.query_row([id], |row| row.get(0))?;
    Ok(count as usize)
}

pub fn revert(conn: &Transaction, id: u64, multithread: bool) {
    let mut stmt = conn
        .prepare("SELECT source, dest FROM Backups WHERE id = ?1")
        .unwrap();
    let mut iter = stmt
        .query_map([id as i64], |row| {
            Ok((row.get(0).unwrap(), row.get(1).unwrap()))
        })
        .unwrap();

    let source_str: String;
    let dest_str: String;
    match iter.next() {
        Some(v) => {
            let v = v.unwrap();
            source_str = v.1;
            dest_str = v.0;
        }
        None => {
            eprintln!("Couldn't find {id}");
            return;
        }
    }
    drop(iter);
    drop(stmt);

    _copy(conn, multithread, source_str.into(), dest_str.into());
}

pub fn delete(conn: &Transaction, id: u64) {
    let mut stmt = conn
        .prepare("SELECT dest FROM Backups WHERE id = ?1")
        .unwrap();
    let mut iter = stmt
        .query_map([id as i64], |row| Ok(row.get(0).unwrap()))
        .unwrap();

    let dest_str: String;
    match iter.next() {
        Some(v) => {
            let v = v.unwrap();
            dest_str = v;
        }
        None => {
            eprintln!("Couldn't find {id}");
            return;
        }
    }
    drop(iter);
    drop(stmt);

    match fs::remove_dir_all(&dest_str) {
        Ok(_) => {}
        Err(e) => {
            eprintln!("{} {}", "Error:".red().bold(), e);
        }
    };
    println!("Deleted {}", dest_str);
    _delete_entry(conn, id);
}

pub fn soft_delete(conn: &Transaction, id: u64) {
    if _delete_entry(conn, id) {
        println!("Deleted {}", id);
        return;
    }
    eprintln!("Couldn't find \"{}\".", id);
}

pub fn list(conn: &Transaction) {
    let mut stmt = conn
        .prepare("SELECT id, source, dest, compression FROM Backups")
        .unwrap();
    let iter = stmt
        .query_map((), |row| {
            Ok(BackupEntry {
                id: row.get::<usize, i64>(0).unwrap() as u64,
                from: row.get::<usize, String>(1).unwrap().into(),
                to: row.get::<usize, String>(2).unwrap().into(),
                compression: row.get(3).unwrap_or(None),
            })
        })
        .unwrap();

    for entry in iter {
        let entry = entry.unwrap();

        println!(
            "{}: {}\n    {}: {}\n    {}: {}",
            "ID".bold(),
            entry.id,
            "Source".bold(),
            entry.from.display().to_string(),
            "Destination".bold(),
            entry.to.display().to_string()
        );
    }
}

fn _delete_entry(conn: &Transaction, id: u64) -> bool {
    match conn
        .execute("DELETE FROM Backups WHERE id = ?1", [id as i64])
        .unwrap()
    {
        0 => false,
        _ => true,
    }
}
