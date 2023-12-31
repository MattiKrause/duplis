use crate::error_handling::AlreadyReportedError;
use crate::file_action::FileConsumeAction;
use crate::util::{path_contains_comma, ChoiceInputReader};
use crate::{
    handle_file_op, in_err_map, out_err_map, report_file_missing, HashedFile, Recoverable,
};
use std::path::PathBuf;

pub trait FileSetConsumer {
    /// first element of set is the 'original',
    /// the set is a least of size 2
    fn consume_set(&mut self, set: Vec<HashedFile>) -> Result<(), AlreadyReportedError>;
}

/// execute given [`FileConsumeAction`] without user input
pub struct UnconditionalAction {
    running_buf: PathBuf,
    original_buf: PathBuf,
    action: Box<dyn FileConsumeAction>,
}

/// execute given [`FileConsumeAction`] after asking user
pub struct InteractiveEachChoice<R, W> {
    running_buf: PathBuf,
    original_buf: PathBuf,
    choice_buf: String,
    action: Box<dyn FileConsumeAction>,
    read: R,
    write: W,
}

/// simply print all files that would be affected by an action
pub struct DryRun<W> {
    path_buf: PathBuf,
    write: W,
}

pub struct MachineReadableEach<W> {
    written_before: bool,
    writer: W,
    path_bufs: (PathBuf, PathBuf),
}
pub struct MachineReadableSet<W> {
    written_before: bool,
    writer: W,
    path_bufs: (PathBuf, PathBuf),
}

impl Default for DryRun<std::io::Stdout> {
    fn default() -> Self {
        Self {
            path_buf: PathBuf::new(),
            write: std::io::stdout(),
        }
    }
}

impl<W> DryRun<W> {
    pub fn new() -> Self
    where
        Self: Default,
    {
        Self::default()
    }
    pub fn new_with(write: W) -> Self {
        Self {
            path_buf: PathBuf::new(),
            write,
        }
    }
}

impl DryRun<std::io::Stdout> {
    pub fn for_console() -> Self {
        Self::new()
    }
}

macro_rules! warn_path_contains_comma {
    ($path: expr) => {
        log::warn!(
            target: crate::error_handling::FORMAT_ERR_TARGET,
            "path {} contains a ',' and cannot be written in machine readable format",
            $path.display()
        );
    };
}

impl<W: std::io::Write> FileSetConsumer for DryRun<W> {
    fn consume_set(&mut self, set: Vec<HashedFile>) -> Result<(), AlreadyReportedError> {
        set[0].file_path.write_full_to_buf(&mut self.path_buf);
        write!(
            self.write,
            "keeping {}, dry-deleting ",
            self.path_buf.display()
        )
        .map_err(out_err_map!())?;
        let mut write_sep = false;
        for file in &set[1..] {
            if write_sep {
                write!(self.write, ", ").map_err(out_err_map!())?;
            }
            write_sep = true;
            file.file_path.write_full_to_buf(&mut self.path_buf);
            write!(self.write, "{}", self.path_buf.display()).map_err(out_err_map!())?;
        }
        writeln!(self.write).map_err(out_err_map!())?;
        Ok(())
    }
}

impl UnconditionalAction {
    pub fn new(action: Box<dyn FileConsumeAction>) -> Self {
        Self {
            running_buf: PathBuf::new(),
            original_buf: PathBuf::new(),
            action,
        }
    }
}

impl FileSetConsumer for UnconditionalAction {
    fn consume_set(&mut self, mut set: Vec<HashedFile>) -> Result<(), AlreadyReportedError> {
        let original_buf = loop {
            let Some(file) = set.get(0) else { return Ok(()) };
            file.file_path.write_full_to_buf(&mut self.original_buf);
            if self.original_buf.exists() {
                break &self.original_buf;
            } else {
                report_file_missing!(&self.original_buf);
                set.remove(0);
            }
        };
        for file in &set[1..] {
            file.file_path.write_full_to_buf(&mut self.running_buf);
            if !self.running_buf.exists() {
                report_file_missing!(&self.running_buf);
                continue;
            }
            if let Err(Recoverable::Fatal(AlreadyReportedError {})) =
                self.action.consume(&self.running_buf, Some(original_buf))
            {
                log::error!(
                    target: crate::error_handling::FILE_SET_ERR_TARGET,
                    "aborting '{}' due to previous error",
                    self.action.short_name()
                );
                return Err(AlreadyReportedError);
            };
        }
        Ok(())
    }
}

impl InteractiveEachChoice<std::io::Stdin, std::io::Stdout> {
    pub fn for_console(action: Box<dyn FileConsumeAction>) -> Self {
        Self::new(std::io::stdin(), std::io::stdout(), action)
    }
}

impl<R, W> InteractiveEachChoice<R, W> {
    pub fn new(read: R, write: W, action: Box<dyn FileConsumeAction>) -> Self {
        Self {
            running_buf: PathBuf::new(),
            original_buf: PathBuf::new(),
            choice_buf: String::new(),
            action,
            read,
            write,
        }
    }
}

impl<R: ChoiceInputReader, W: std::io::Write> FileSetConsumer for InteractiveEachChoice<R, W> {
    fn consume_set(&mut self, mut set: Vec<HashedFile>) -> Result<(), AlreadyReportedError> {
        let original_buf = loop {
            let Some(file) = set.get(0) else { return Ok(()) };
            file.file_path.write_full_to_buf(&mut self.original_buf);
            if self.original_buf.exists() {
                break &self.original_buf;
            } else {
                report_file_missing!(&self.original_buf);
                set.remove(0);
            }
        };
        for file in &set[1..] {
            file.file_path.write_full_to_buf(&mut self.running_buf);
            if !self.running_buf.exists() {
                report_file_missing!(&self.running_buf);
                continue;
            }
            writeln!(
                self.write,
                "{} {}?",
                self.action.short_name().as_ref(),
                self.running_buf.display()
            )
            .map_err(out_err_map!())?;
            let execute_action = loop {
                self.write.flush().map_err(out_err_map!())?;
                self.choice_buf.clear();
                self.read
                    .read_remaining(&mut self.choice_buf)
                    .map_err(in_err_map!())?;
                if self.choice_buf.is_empty() {
                    log::error!(
                        target: crate::error_handling::INTERACTION_ERR_TARGET,
                        "cannot accept input in interactive mode since the input is closed"
                    );
                    return Err(AlreadyReportedError);
                }
                let choice = self.choice_buf.trim();

                if choice.eq_ignore_ascii_case("y") | choice.eq_ignore_ascii_case("yes") {
                    break true;
                } else if choice.eq_ignore_ascii_case("n") | choice.eq_ignore_ascii_case("no") {
                    break false;
                } else {
                    writeln!(
                        self.write,
                        "unrecognised answer; only y(es) and n(o) are accepted"
                    )
                    .map_err(out_err_map!())?;
                }
            };

            if execute_action {
                if let Err(Recoverable::Fatal(AlreadyReportedError {})) =
                    self.action.consume(&self.running_buf, Some(original_buf))
                {
                    log::error!(
                        target: crate::error_handling::FILE_SET_ERR_TARGET,
                        "aborting '{}' due to previous error",
                        self.action.short_name()
                    );
                    return Err(AlreadyReportedError);
                };
            }
        }
        Ok(())
    }
}

impl<W: std::io::Write> MachineReadableEach<W> {
    pub fn new(writer: W) -> Self {
        Self {
            written_before: false,
            writer,
            path_bufs: (PathBuf::new(), PathBuf::new()),
        }
    }
}

impl MachineReadableEach<std::io::Stdout> {
    pub fn for_console() -> Self {
        Self::new(std::io::stdout())
    }
}

impl<W: std::io::Write> FileSetConsumer for MachineReadableEach<W> {
    fn consume_set(&mut self, mut set: Vec<HashedFile>) -> Result<(), AlreadyReportedError> {
        let (orig_path, tmp_path) = &mut self.path_bufs;
        let Some(orig_path) = find_nocomma_original(&mut set, orig_path) else { return Ok(()) };
        for file in &set[1..] {
            file.file_path.write_full_to_buf(tmp_path);

            let tmp_path = handle_file_op!(std::fs::canonicalize(&*tmp_path), tmp_path, continue);
            if path_contains_comma(&tmp_path) {
                warn_path_contains_comma!(&tmp_path);
                continue;
            }
            if self.written_before {
                writeln!(self.writer).map_err(out_err_map!())?;
            }
            write!(
                self.writer,
                "{},{}",
                orig_path.display(),
                tmp_path.display()
            )
            .map_err(out_err_map!())?;
            self.written_before = true;
        }

        Ok(())
    }
}

impl<W: std::io::Write> MachineReadableSet<W> {
    pub fn new(writer: W) -> Self {
        Self {
            written_before: false,
            writer,
            path_bufs: (PathBuf::new(), PathBuf::new()),
        }
    }
}

impl MachineReadableSet<std::io::Stdout> {
    pub fn for_console() -> Self {
        Self::new(std::io::stdout())
    }
}

impl<W: std::io::Write> FileSetConsumer for MachineReadableSet<W> {
    fn consume_set(&mut self, mut set: Vec<HashedFile>) -> Result<(), AlreadyReportedError> {
        let (orig_path, tmp_path) = &mut self.path_bufs;
        let mut first = true;
        let Some(orig_path) = find_nocomma_original(&mut set, orig_path) else { return Ok(()) };
        if self.written_before {
            writeln!(self.writer).map_err(out_err_map!())?;
        }
        for file in &set[1..] {
            file.file_path.write_full_to_buf(tmp_path);
            let tmp_path = handle_file_op!(tmp_path.canonicalize(), tmp_path, continue);
            if path_contains_comma(&tmp_path) {
                warn_path_contains_comma!(&tmp_path);
                continue;
            }
            let empty_path = PathBuf::new();
            let prev_path = if first { &orig_path } else { &empty_path };
            write!(
                self.writer,
                "{},{}",
                prev_path.display(),
                tmp_path.display()
            )
            .map_err(out_err_map!())?;
            first = false;
            self.written_before = true;
        }
        Ok(())
    }
}

fn find_nocomma_original(set: &mut Vec<HashedFile>, orig_path: &mut PathBuf) -> Option<PathBuf> {
    let buf = loop {
        let Some(first) = set.get(0) else { return None };
        first.file_path.write_full_to_buf(orig_path);
        let orig_path = handle_file_op!(orig_path.canonicalize(), orig_path, {
            set.remove(0);
            continue;
        });
        if path_contains_comma(&orig_path) {
            warn_path_contains_comma!(&orig_path);
            set.remove(0);
            continue;
        }

        break orig_path;
    };
    Some(buf)
}
