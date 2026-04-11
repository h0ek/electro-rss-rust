use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

static LOG_FILE: OnceLock<Mutex<File>> = OnceLock::new();

pub fn init_logger(path: &Path) {
    if let Ok(file) = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)
    {
        let _ = LOG_FILE.set(Mutex::new(file));
    }
}

pub fn log_info(msg: &str) {
    write_line("INFO", msg);
}

pub fn log_error(msg: &str) {
    write_line("ERROR", msg);
}

fn write_line(level: &str, msg: &str) {
    if let Some(lock) = LOG_FILE.get() {
        if let Ok(mut file) = lock.lock() {
            let _ = writeln!(file, "[{}] {}", level, msg);
        }
    }
}
