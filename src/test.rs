#[cfg(test)]
mod tests {
    extern crate test;

    use crate::_copy;
    use colored::Colorize;
    use ini::Ini;
    use rand::Rng;
    use std::fs;
    use std::fs::File;
    use std::io::Write;
    use test::Bencher;

    const FILE_SIZE: usize = 1024 * 1024 * 16;
    const FILE_SIZE_S: usize = 1024 * 1024;
    #[bench]
    fn create_backup_singlethread(b: &mut Bencher) {
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

        fs::create_dir_all("test/test_singlethread/source").unwrap();
        fs::create_dir_all("test/test_singlethread/dest").unwrap();
        let args = vec![
            "test/test_singlethread/source".to_string(),
            "test/test_singlethread/dest".to_string(),
        ];

        let mut f = File::create("test/test_singlethread/source/big_file").unwrap();
        let mut rng = rand::thread_rng();
        let mut buf = Vec::with_capacity(FILE_SIZE);
        for _ in 0..=FILE_SIZE {
            buf.push(rng.gen());
        }
        f.write_all(&*buf).unwrap();
        f.flush().unwrap();

        b.iter(|| {
            _copy(
                config_dir.clone(),
                &mut config,
                &args,
                &mut "test/test_singlethread/source".to_string(),
                "test/test_singlethread/dest".to_string(),
            );
        });
    }

    #[bench]
    fn create_backup_multithread(b: &mut Bencher) {
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

        fs::create_dir_all("test/test_multithread/source").unwrap();
        fs::create_dir_all("test/test_multithread/dest").unwrap();
        let args = vec![
            "test/test_multithread/source".to_string(),
            "test/test_multithread/dest".to_string(),
            "--multi-thread".to_string(),
        ];

        for i in 1..=16 {
            let mut f =
                File::create(format!("test/test_multithread/source/small_file{}", i)).unwrap();
            let mut rng = rand::thread_rng();
            let mut buf = Vec::with_capacity(FILE_SIZE_S);
            for _ in 0..=FILE_SIZE_S {
                buf.push(rng.gen());
            }
            f.write_all(&*buf).unwrap();
            f.flush().unwrap();
        }

        b.iter(|| {
            _copy(
                config_dir.clone(),
                &mut config,
                &args,
                &mut "test/test_multithread/source".to_string(),
                "test/test_multithread/dest".to_string(),
            );
        });
    }
}
