use log::{LevelFilter, SetLoggerError};

pub struct Logger {
    max_level: LevelFilter,
}

pub fn init_with_level(level: LevelFilter) -> Result<(), SetLoggerError> {
    log::set_max_level(level);
    log::set_boxed_logger(Box::new(Logger::new(level)))?;
    Ok(())
}

impl Logger {
    pub fn new(max_level: LevelFilter) -> Self {
        Self { max_level }
    }
}

impl log::Log for Logger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= self.max_level
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }

        let should_log = record
            .file()
            .map(|x| x.contains("blink-w"))
            .unwrap_or(false);
        if !should_log {
            return;
        }

        let msg = format!("{}", record.args());
        println!("{msg}");
    }

    fn flush(&self) {}
}
