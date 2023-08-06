macro_rules! declare_log_targets {
    ($($name: ident = $value: literal;)*) => {
        $(pub static $name: &str = $value;)*

        pub fn get_all_log_targets() -> Vec<&'static str> {
            vec![$($name),*]
        }
    };
}

declare_log_targets! {
    INTERACTION_ERR_TARGET = "user_interaction_err";
    FORMAT_ERR_TARGET = "file_format_err";
    CONFIG_ERR_TARGET = "config_err";
    ACTION_FATAL_FAILURE_TARGET = "fatal_action_failure";
    ACTION_SUCCESS_TARGET = "action_success";
    DISCOVERY_ERR_TARGET = "file_discovery_err";
    FILE_ERR_TARGET = "file_error";
    FILE_SET_ERR_TARGET = "file_set_err";
}

#[macro_export]
macro_rules! report_file_missing {
    ($path: expr) => {
        log::trace!(
            target: $crate::error_handling::FILE_ERR_TARGET,
            "file {} disappeared while being processed",
            $path.display()
        )
    };
}

#[macro_export]
macro_rules! handle_file_error {
    ($file_path: expr, $err: expr) => {
        match $err.kind() {
            std::io::ErrorKind::NotFound => $crate::report_file_missing!(&$file_path),
            std::io::ErrorKind::PermissionDenied => log::info!(
                target: $crate::error_handling::FILE_ERR_TARGET,
                "cannot access file {}(permission denied)",
                $file_path.display()
            ),
            _ => log::warn!(
                target: $crate::error_handling::FILE_ERR_TARGET,
                "unexpected error while accessing file {}: {}",
                $file_path.display(),
                $err
            ),
        };
    };
}

#[macro_export]
macro_rules! handle_file_op {
    ($result: expr, $file_path: expr, $handle_action: expr) => {
        match $result {
            Ok(result) => result,
            Err(err) => {
                $crate::handle_file_error!($file_path, err);
                $handle_action
            }
        }
    };
}

#[macro_export]
macro_rules! handle_file_modified {
    ($file_path: expr) => { log::warn!(target: $crate::error_handling::FILE_ERR_TARGET, "file {} was modified while still being processed; The file will not be processed further", $file_path.display()) };
}

/// in case the out-stream of the printing consumers fails
#[macro_export]
macro_rules! out_err_map {
    () => {
        |err| {
            log::error!(
                target: $crate::error_handling::INTERACTION_ERR_TARGET,
                "cannot write out in interactive mode: {err}; aborting"
            );
            AlreadyReportedError
        }
    };
}

/// in case the in-stream of the interactive consumers fails
#[macro_export]
macro_rules! in_err_map {
    () => {
        |err| {
            log::error!(
                target: $crate::error_handling::INTERACTION_ERR_TARGET,
                "cannot accept input in interactive mode: {err}; aborting"
            );
            AlreadyReportedError
        }
    };
}

#[derive(Copy, Clone, Debug)]
pub struct AlreadyReportedError;
