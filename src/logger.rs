use std::{
    fs::OpenOptions,
    io::{self, Write},
    path::Path,
    sync::{Mutex, OnceLock},
};

#[derive(Debug)]
pub struct Logger {
    file: Mutex<Option<std::fs::File>>,
    verbose: Mutex<bool>,
}

impl Logger {
    pub fn new() -> Self {
        Self {
            file: Mutex::new(None),
            verbose: Mutex::new(false),
        }
    }

    pub fn init<P: AsRef<Path>>(&self, path: P, verbose: bool) -> io::Result<()> {
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        if let Ok(mut guard) = self.file.lock() {
            *guard = Some(file);
        }
        if let Ok(mut v) = self.verbose.lock() {
            *v = verbose;
        }
        Ok(())
    }

    pub fn log(&self, msg: &str) {
        if let Ok(mut guard) = self.file.lock() {
            if let Some(f) = guard.as_mut() {
                let _ = writeln!(f, "{msg}");
            }
        }
    }
}

static LOGGER: OnceLock<Logger> = OnceLock::new();

pub fn global() -> &'static Logger {
    LOGGER.get_or_init(Logger::new)
}

pub fn log_debug(msg: &str) {
    global().log(msg);
}
