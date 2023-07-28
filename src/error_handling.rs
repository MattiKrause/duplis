#[macro_export]
macro_rules! handle_file_error {
    ($file_path: expr, $err: expr) => {
        match $err.kind() {
            std::io::ErrorKind::NotFound => crate::error_handling::report_file_missing(&$file_path),
            std::io::ErrorKind::PermissionDenied => log::info!(target: "file_error", "cannot access file {}(permission denied)", $file_path.display()),
            _ => log::warn!(target: "file_error", "unexpected error while accessing file {}: {}", $file_path.display(), $err)
        };
    };
}

#[macro_export]
macro_rules! handle_file_op {
    ($result: expr, $file_path: expr, $handle_action: expr) => {
        match $result {
            Ok(result) => result,
            Err(err) => {
                crate::handle_file_error!($file_path, err);
                $handle_action
            }
        }
    };
}

#[macro_export]
macro_rules! handle_file_modified {
    ($file_path: expr) => { log::warn!(target: "file_error", "file {} was modified while still being processed; The file will not be processed further", $file_path.display()) };
}

#[derive(Copy, Clone, Debug)]
pub struct AlreadyReportedError;

pub fn report_file_missing(path: impl AsRef<std::path::Path>) {
    log::trace!(target: "file_error", "file {} disappeared while being processed", path.as_ref().display())
}