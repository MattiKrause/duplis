use std::borrow::Cow;
use std::path::Path;
use crate::error_handling::AlreadyReportedError;
use crate::{handle_file_op, Recoverable};

pub trait FileConsumeAction {
    /// consumes the file pointed to by `path`
    /// The `original` may be used for multiple calls of consume, so this should be kept in mind
    /// the file pointed to by `original` can be assumed to exist
    fn consume(&mut self, path: &Path, original: Option<&Path>) -> FileConsumeResult;
    /// return true if this requires an original file, for example because it links to the original file
    fn requires_original(&self) -> bool;
    /// short description of this consumer like 'delete' or 'replace with hardlink'
    fn short_name(&self) -> Cow<str>;
    /// short description of not execution this action like 'keep'
    fn short_opposite(&self) -> Cow<str>;
}

pub type FileConsumeResult = Result<(), Recoverable<AlreadyReportedError, AlreadyReportedError>>;

/// print the files that are given to this action
#[derive(Default)]
pub struct DebugFileAction {
    // make file only constructable with new method
    _p: ()
}
/// delete the given file
#[derive(Default)]
pub struct DeleteFileAction {
    // make file only constructable with new method
    _p: ()
}
/// replace the file with a hard link to the 'original' file
#[derive(Default)]
pub struct ReplaceWithHardLinkFileAction {
    // make file only constructable with new method
    _p: ()
}

/// report a successful file action
#[macro_export]
macro_rules! report_file_action {
    ($text: literal, $($r: expr),*) => {log::info!(target: $crate::error_handling::ACTION_SUCCESS_TARGET, $text, $($r),*)};
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
        handle_file_op!(std::fs::remove_file(path), path, return Err(Recoverable::Recoverable(AlreadyReportedError)));
        report_file_action!("deleted file {}", path.display());
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
        handle_file_op!(std::fs::remove_file(path), path, return Err(Recoverable::Recoverable(AlreadyReportedError)));
        if let Err(err) = std::fs::hard_link(original, path) {
            log::error!(target: crate::error_handling::ACTION_FATAL_FAILURE_TARGET, "FATAL ERROR: failed to create hard link to {} from {} due to error {err}", path.display(), original.display());
            // Something is absolutely not right here, continuing means risk of data loss
            return Err(Recoverable::Fatal(AlreadyReportedError));
        }
        report_file_action!("replaced {} with a hard link to {}", path.display(), original.display());
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