#[cfg(test)]
mod tests {
    use crate::_copy;
    use rand::Rng;
    use rusqlite::Connection;
    use std::fs;
    use std::fs::File;
    use std::io::Write;

    const FILE_SIZE: usize = 1024 * 1024 * 16;
    const FILE_SIZE_S: usize = 1024 * 1024;
    #[test]
    fn create_backup_singlethread() {
        let mut conn = Connection::open_in_memory().unwrap();
        let tx = conn.transaction().unwrap();

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

        fs::create_dir_all("test/test_singlethread/source").unwrap();
        fs::create_dir_all("test/test_singlethread/dest").unwrap();

        let mut f = File::create("test/test_singlethread/source/big_file").unwrap();
        let mut rng = rand::thread_rng();
        let mut buf = Vec::with_capacity(FILE_SIZE);
        for _ in 0..=FILE_SIZE {
            buf.push(rng.gen());
        }
        f.write_all(&*buf).unwrap();
        f.flush().unwrap();

        _copy(
            &tx,
            false,
            "test/test_singlethread/source".into(),
            "test/test_singlethread/dest".into(),
        );
    }

    #[test]
    fn create_backup_multithread() {
        let mut conn = Connection::open_in_memory().unwrap();
        let tx = conn.transaction().unwrap();

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

        fs::create_dir_all("test/test_multithread/source").unwrap();
        fs::create_dir_all("test/test_multithread/dest").unwrap();

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

        _copy(
            &tx,
            true,
            "test/test_multithread/source".into(),
            "test/test_multithread/dest".into(),
        );
    }
}
