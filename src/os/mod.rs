
use crate::set_order::SetOrder;

#[cfg(unix)]
mod unix_specific;
#[cfg(windows)]
mod windows_specific;

#[cfg(unix)]
use unix_specific::{get_file_consume_action_simple as gfcas, get_file_equals_arg_simple as gfeas, get_set_order_options as gsoo, get_file_name_filters as gfnf};
#[cfg(windows)]
use windows_specific::{complex_cmd_config as ccc, complex_parse_file_metadata_filter as cpfmf};
use crate::file_filters::FileMetadataFilter;
use crate::file_set_refiner::FileEqualsChecker;

pub struct  SetOrderOption {
    pub name: &'static str,
    pub help: String,
    pub implementation: Box<dyn SetOrder>
}

macro_rules! simple_component_arg {
    ($name: ident, $cname: path) => {
        pub struct $name {
            pub name: &'static str,
            pub short: Option<char>,
            pub long: &'static str,
            pub help: String,
            pub default: bool,
            pub action: Box<dyn $cname + Send>
        }
    };
}

simple_component_arg!(SimpleFileConsumeActionArg, crate::file_action::FileConsumeAction);
simple_component_arg!(SimpleFileEqualCheckerArg, FileEqualsChecker);
simple_component_arg!(FileNameFilterArg, crate::file_filters::FileNameFilter);

macro_rules! delegating_impl {
    ($fnname: ident, $rtype: ty, $delegate: ident, $def: expr) => {
        #[cfg(any(unix))]
        pub fn $fnname() -> $rtype {
            $delegate()
        }
        #[cfg(not(any(unix)))]
        pub fn $fnname() -> $rtype {
            $def
        }
    };
}

fn make_no_hidden<T>(mapper: impl FnOnce(&'static str, Option<char>, &'static str, String, bool) -> T) -> T {
    mapper("nohidden", None, "nohidden", String::from("do not search hidden files and directories for duplicates"), false)
}

delegating_impl!(get_set_order_options, Vec<SetOrderOption>, gsoo, Vec::new());
delegating_impl!(get_file_consumer_simple, Vec<SimpleFileConsumeActionArg>, gfcas, Vec::new());
delegating_impl!(get_file_equals_simple, Vec<SimpleFileEqualCheckerArg>, gfeas, Vec::new());
delegating_impl!(get_file_name_filters, Vec<FileNameFilterArg>, gfnf, Vec::new());

pub fn complex_cmd_config(command: clap::Command) -> clap::Command {
    #[cfg(any(windows))]
    return ccc(command);
    #[cfg(not(any(windows)))]
    return command;
}

pub fn complex_parse_file_metadata_filters(matches: &clap::ArgMatches) -> Vec<Box<dyn FileMetadataFilter + Send>>{
    #[cfg(any(windows))]
    return cpfmf(matches);
    #[cfg(not(any(windows)))]
    return Vec::new();
}