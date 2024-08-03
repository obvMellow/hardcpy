use crate::{_copy, _pb_update, SEPARATOR};
use colored::Colorize;
use indicatif::{HumanCount, MultiProgress, ProgressBar, ProgressStyle};
use indicatif_log_bridge::LogWrapper;
use ini::Ini;
use log::{error, info};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::Read;
use std::path::PathBuf;

pub fn verify(config: Ini, id: String) {
    let mut error_list = Vec::new();
    let mut verified = 0;
    let mut copied = 0;
    let iter = config.section(Some(format!("Backup.{}", id))).unwrap();
    let multi = MultiProgress::new();
    let logger = colog::default_builder().build();
    LogWrapper::new(multi.clone(), logger).try_init().unwrap();

    let pb = multi.add(ProgressBar::new(iter.len() as u64));

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

    for (k, v) in iter {
        let mut split = v.split(SEPARATOR);
        let to = split.nth(0).unwrap();
        info!("{} \"{to}\"", "Verifying".green().bold());
        let mut read_from = match File::open(to) {
            Ok(v) => v,
            Err(_) => {
                info!("\n{} {k}", "Copying".blue().bold());
                match fs::copy(k, to) {
                    Ok(v) => v,
                    Err(e) => {
                        error!("{e}");
                        error_list.push(e);
                        continue;
                    }
                };
                copied += 1;
                File::open(to).unwrap()
            }
        };
        let mut hasher = Sha256::new();

        let file_size = read_from.metadata().unwrap().len();
        let max_buf_size = 1024 * 1024 * 1024 * 4;
        let buf_size = file_size.min(max_buf_size);
        let mut buf = Vec::with_capacity(buf_size as usize);
        while read_from.read_to_end(&mut buf).unwrap() > 0 {
            hasher.update(&buf);
            if buf_size == file_size {
                break;
            }
        }

        let hash = format!("{:X}", hasher.finalize());
        if hash != split.nth(0).unwrap() {
            info!("\n{} \"{to}\"", "Copying".green().bold());
            match fs::copy(k, to) {
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
        HumanCount(iter.len() as u64).to_string(),
        HumanCount(copied).to_string(),
        HumanCount(error_list.len() as u64).to_string(),
    );
}

pub fn revert(config_dir: PathBuf, mut config: Ini, id: String, multithread: bool) {
    let mut setter = config.with_section(Some("Backups"));
    let mut vec = match setter.get(&id) {
        Some(v) => v.split(SEPARATOR),
        None => {
            eprintln!("Couldn't find \"{}\".", id);
            return;
        }
    };
    let mut dest_str = PathBuf::from(vec.nth(0).unwrap().to_string());
    dest_str.pop();
    let dest_str = dest_str.to_str().unwrap().to_string();
    let source_str = vec.nth(0).unwrap().to_string(); // Getting the first element again because .nth() consumes all preceding and the returned element

    _copy(
        config_dir,
        &mut config,
        multithread,
        source_str.into(),
        dest_str.into(),
    );
}

pub fn delete(mut config_dir: PathBuf, mut config: Ini, id: String) {
    let mut setter = config.with_section(Some("Backups"));
    let dest = match setter.get(&id) {
        Some(v) => v.split(SEPARATOR).collect::<Vec<&str>>()[1],
        None => {
            eprintln!("Couldn't find \"{}\".", id);
            return;
        }
    };

    match fs::remove_dir_all(dest) {
        Ok(_) => {}
        Err(e) => {
            eprintln!("{} {}", "Error:".red().bold(), e);
        }
    };
    println!("Deleted {}", dest);
    _delete_entry(&mut config_dir, &mut config, &id);
}

pub fn soft_delete(mut config_dir: PathBuf, mut config: Ini, id: String) {
    if _delete_entry(&mut config_dir, &mut config, &id) {
        println!("Deleted {}", id);
        return;
    }
    eprintln!("Couldn't find \"{}\".", id);
}

pub fn list(config: Ini) {
    for (sec, prop) in &config {
        if sec == Some("Backups") {
            for (k, v) in prop.iter() {
                let v: Vec<&str> = v.split(SEPARATOR).collect();
                println!(
                    "{} {k}\n   {} {}\n   {} {}",
                    "ID:".bold(),
                    "Source:".bold(),
                    v[0],
                    "Destination:".bold(),
                    v[1]
                );
            }
        }
    }
}

fn _delete_entry(config_dir: &mut PathBuf, config: &mut Ini, id: &String) -> bool {
    if config.with_section(Some("Backups")).get(id).is_some() {
        config.with_section(Some("Backups")).delete(id);
    } else {
        return false;
    }
    config.write_to_file(config_dir.join("config.ini")).unwrap();
    true
}
