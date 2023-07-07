use std::borrow::Cow;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use crate::{BoxErr, HashedFile, Recoverable};

pub trait FileSetConsumer {
    // first element of set is the 'original'
    fn consume_set(&mut self, set: Vec<HashedFile>) -> Result<(), BoxErr>;
}

pub type FileConsumeResult = Result<(), Recoverable<(), ()>>;
pub trait FileConsumeAction {
    fn consume(&mut self, path: &Path, original: Option<&Path>) -> FileConsumeResult;
    fn requires_original(&self) -> bool;
    fn short_name(&self) -> Cow<str>;
    fn short_opposite(&self) -> Cow<str>;
}

pub trait ChoiceInputReader {
    fn read_remaining(&mut self, buf: &mut String) -> std::io::Result<()>;
}

pub struct UnconditionalAction {
    running_buf: PathBuf,
    original_buf: PathBuf,
    action: Box<dyn FileConsumeAction>,
}

pub struct InteractiveEachChoice<R, W> {
    running_buf: PathBuf,
    original_buf: PathBuf,
    choice_buf: String,
    action: Box<dyn FileConsumeAction>,
    read: R,
    write: W,
}

pub struct InteractiveSetChoice {
    path_buf: PathBuf,
    input_buf: String,
    action: Box<dyn FileConsumeAction>,
}


pub struct DryRun<W> {
    path_buf: PathBuf,
    write: W,
}

#[derive(Default)]
pub struct DebugFileAction;
pub struct DeleteFileAction;
pub struct ReplaceWithHardLinkFileAction;

impl Default for DryRun<std::io::Stdout> {
    fn default() -> Self {
        Self {
            path_buf: PathBuf::new(),
            write: std::io::stdout(),
        }
    }
}

impl<W> DryRun<W> {
    pub fn new() -> Self where Self: Default {
        Self::default()
    }
    pub fn new_with(write: W) -> Self {
        Self { path_buf: PathBuf::new(), write }
    }
}

impl DryRun<std::io::Stdout> {
    pub fn for_console() -> Self {
        Self::new()
    }
}

macro_rules! out_err_map {
    () => { |err| {
        log::error!("cannot write out in interactive mode: {err}; aborting");
        BoxErr::from(format!("cannot write out in interactive mode: {err}; aborting"))
    }};
}

macro_rules! in_err_map {
    () => { |err| {
        log::error!("cannot accept input in interactive mode: {err}; aborting");
        BoxErr::from(format!("cannot accept input in interactive mode: {err}; aborting"))
    }};
}

impl<W: std::io::Write> FileSetConsumer for DryRun<W> {
    fn consume_set(&mut self, set: Vec<HashedFile>) -> Result<(), BoxErr> {
        self.path_buf.clear();
        set[0].file_path.push_full_to_buf(&mut self.path_buf);
        write!(self.write, "keeping {}, deleting ", self.path_buf.display()).map_err(out_err_map!())?;
        let mut write_sep = false;
        for file in &set[1..] {
            if write_sep {
                write!(self.write, ", ").map_err(out_err_map!())?;
            }
            write_sep = true;
            self.path_buf.clear();
            file.file_path.push_full_to_buf(&mut self.path_buf);
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
    fn consume_set(&mut self, set: Vec<HashedFile>) -> Result<(), BoxErr> {
        self.original_buf.clear();
        set[0].file_path.push_full_to_buf(&mut self.original_buf);
        let original_buf = &self.original_buf;
        for file in &set[1..] {
            self.running_buf.clear();
            file.file_path.push_full_to_buf(&mut self.running_buf);
            if let Err(Recoverable::Fatal(())) = self.action.consume(&self.running_buf, Some(&original_buf)) {
                return Err(format!("consume set failed").into())
            };
        }
        Ok(())
    }
}

impl ChoiceInputReader for std::io::Stdin {
    fn read_remaining(&mut self, buf: &mut String) -> std::io::Result<()> {
        use std::io::BufRead;
        self.lock().read_line(buf).map(|_| ())
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
    fn consume_set(&mut self, set: Vec<HashedFile>) -> Result<(), BoxErr> {
        self.original_buf.clear();
        set[0].file_path.push_full_to_buf(&mut self.original_buf);
        for file in &set[1..] {
            self.running_buf.clear();
            file.file_path.push_full_to_buf(&mut self.running_buf);
            write!(self.write, "{} {}? ", self.action.short_name().as_ref(), self.running_buf.display()).map_err(out_err_map!())?;
            let execute_action = loop {
                self.write.flush().map_err(out_err_map!())?;
                self.choice_buf.clear();
                self.read.read_remaining(&mut self.choice_buf).map_err(in_err_map!())?;
                let choice = self.choice_buf.trim();

                if choice.eq_ignore_ascii_case("y") | choice.eq_ignore_ascii_case("yes") {
                    break true;
                } else if choice.eq_ignore_ascii_case("n") | choice.eq_ignore_ascii_case("no") {
                    break false;
                } else {
                    println!("unrecognised answer; only y(es) and n(o) are accepted");
                }
            };

            if execute_action {
                if let Err(Recoverable::Fatal(())) = self.action.consume(&self.running_buf, Some(&self.original_buf)) {
                    return Err(format!("set consumer failed").into())
                };
            }
        }
        Ok(())
    }
}

impl FileConsumeAction for DebugFileAction {
    fn consume(&mut self, path: &Path, original: Option<&Path>) -> FileConsumeResult {
        dbg!(path, original);
        Ok(())
    }

    fn requires_original(&self) -> bool {
        false
    }

    fn short_name(&self) -> Cow<str> {
        Cow::Borrowed("debug print")
    }

    fn short_opposite(&self) -> Cow<str> {
        Cow::Borrowed("ignore")
    }
}

impl FileConsumeAction for DeleteFileAction {
    fn consume(&mut self, path: &Path, _original: Option<&Path>) -> FileConsumeResult {
        if let Err(err) =  std::fs::remove_file(path) {
            return match err.kind() {
                ErrorKind::NotFound => Ok(()),
                ErrorKind::PermissionDenied => {
                    log::info!("failed to delete {} due to lacking permissions", path.display());
                    Err(Recoverable::Recoverable(()))
                }
                _ => {
                    log::warn!("failed to delete {} due to error {err}", path.display());
                    Err(Recoverable::Recoverable(()))
                }
            }
        };
        Ok(())
    }

    fn requires_original(&self) -> bool {
        false
    }

    fn short_name(&self) -> Cow<str> {
        Cow::Borrowed("delete")
    }

    fn short_opposite(&self) -> Cow<str> {
        Cow::Borrowed("keep")
    }
}

impl FileConsumeAction for ReplaceWithHardLinkFileAction {
    fn consume(&mut self, path: &Path, original: Option<&Path>) -> FileConsumeResult {
        let original = original.expect("original required");
        if let Err(err) = std::fs::remove_file(path) {
            match err.kind() {
                ErrorKind::NotFound => {}
                ErrorKind::PermissionDenied => {
                    log::info!("failed to delete file {} in order to replace it with a hard link due to lacking permissions", path.display());
                    return Err(Recoverable::Recoverable(()));
                }
                _ => {
                    log::warn!("failed ot delete file {} in order to replace it with a hard link due error {err}", path.display());
                }
            }
        };
        if let Err(err) = std::fs::hard_link(original, path) {
            log::error!("FATAL ERROR: failed to create hard link to {} from {} due to error {err}", path.display(), original.display());
            // Something is absolutely not right here, continuing means risk of data loss
            return Err(Recoverable::Fatal(()));
        }
        Ok(())
    }

    fn requires_original(&self) -> bool {
        true
    }

    fn short_name(&self) -> Cow<str> {
        Cow::Borrowed("replace with hardlink")
    }

    fn short_opposite(&self) -> Cow<str> {
        Cow::Borrowed("keep")
    }
}