use log::{LevelFilter, Metadata, Record};

/// This logger needs to:
/// - filter by target
/// - print readable
/// - filter by level
/// - write to different `std::io::Write`
/// - be compact in byte-size
///
/// other loggers either fail requirement 1, 2 oder 5
pub struct DuplisLogger {
    disallowed_targets: Vec<String>,
    log_level_filter: LevelFilter,
    write: std::sync::Mutex<Box<dyn std::io::Write + Send>>,
}

impl DuplisLogger {
    pub fn new(
        mut ignore_targets: Vec<String>,
        log_level_filter: LevelFilter,
        write: Box<dyn std::io::Write + Send>,
    ) -> Self {
        ignore_targets.sort_unstable();
        DuplisLogger {
            disallowed_targets: ignore_targets,
            log_level_filter,
            write: std::sync::Mutex::new(write),
        }
    }

    pub fn init(
        ignore_targets: Vec<String>,
        log_level_filter: LevelFilter,
        write: Box<dyn std::io::Write + Send>,
    ) -> Result<(), log::SetLoggerError> {
        log::set_max_level(log_level_filter);
        let logger = Self::new(ignore_targets, log_level_filter, write);
        log::set_boxed_logger(Box::new(logger))
    }
}

impl log::Log for DuplisLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        if metadata.level() > self.log_level_filter {
            return false;
        }
        if self
            .disallowed_targets
            .iter()
            .any(|dt| dt == metadata.target())
        {
            return false;
        }
        true
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }

        let Ok(mut write) = self.write.lock() else { return };
        let _ = writeln!(
            write,
            "[{}]({}): {}",
            record.level(),
            record.target(),
            record.args()
        );
    }

    fn flush(&self) {
        use std::io::Write;
        let Ok(mut write) = self.write.lock() else { return };
        let _ = write.flush();
    }
}
