use crate::{_copy, SEPARATOR};
use colored::Colorize;
use ini::Ini;
use std::fs;
use std::path::PathBuf;

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
