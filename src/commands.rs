use crate::{_copy, SEPARATOR};
use colored::Colorize;
use ini::Ini;
use std::fs;
use std::path::PathBuf;

pub fn revert(config_dir: PathBuf, mut config: Ini, args: &Vec<String>) {
    let i = args
        .iter()
        .position(|v| v == &"revert".to_string())
        .unwrap();
    let hash = match args.get(i + 1) {
        Some(v) => v,
        None => {
            eprintln!("Please enter a valid ID.");
            return;
        }
    };

    let mut setter = config.with_section(Some("Backups"));
    let mut vec = match setter.get(hash) {
        Some(v) => v.split(SEPARATOR),
        None => {
            eprintln!("Couldn't find \"{}\".", hash);
            return;
        }
    };
    let mut dest_str = PathBuf::from(vec.nth(0).unwrap().to_string());
    dest_str.pop();
    let dest_str = dest_str.to_str().unwrap().to_string();
    let mut source_str = vec.nth(0).unwrap().to_string(); // Getting the first element again because .nth() consumes all preceding and the returned element

    _copy(config_dir, &mut config, args, &mut source_str, dest_str);
}

pub fn delete(mut config_dir: PathBuf, mut config: Ini, args: &Vec<String>) {
    let i = args
        .iter()
        .position(|v| v == &"delete".to_string())
        .unwrap();
    let hash = match args.get(i + 1) {
        Some(v) => v,
        None => {
            eprintln!("Please enter a valid ID.");
            return;
        }
    };

    let mut setter = config.with_section(Some("Backups"));
    let dest = match setter.get(hash) {
        Some(v) => v.split(SEPARATOR).collect::<Vec<&str>>()[1],
        None => {
            eprintln!("Couldn't find \"{}\".", hash);
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
    _delete_entry(&mut config_dir, &mut config, hash);
}

pub fn soft_delete(mut config_dir: PathBuf, mut config: Ini, args: &Vec<String>) {
    let i = args
        .iter()
        .position(|v| v == &"soft-delete".to_string())
        .unwrap();
    let hash = match args.get(i + 1) {
        Some(v) => v,
        None => {
            eprintln!("Please enter a valid ID.");
            return;
        }
    };

    if _delete_entry(&mut config_dir, &mut config, hash) {
        println!("Deleted {}", hash);
        return;
    }
    eprintln!("Couldn't find \"{}\".", hash);
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

fn _delete_entry(config_dir: &mut PathBuf, config: &mut Ini, hash: &String) -> bool {
    if config.with_section(Some("Backups")).get(hash).is_some() {
        config.with_section(Some("Backups")).delete(hash);
    } else {
        return false;
    }
    config.write_to_file(config_dir.join("config.ini")).unwrap();
    true
}
