
use crate::set_order::SetOrder;

#[cfg(unix)]
mod unix_specific;

#[cfg(unix)]
use unix_specific::{get_file_consume_action_simple as gfcas, get_file_equals_arg_simple as gfeas, get_set_order_options as gsoo};
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
            pub short: char,
            pub long: &'static str,
            pub help: String,
            pub action: Box<dyn $cname>
        }
    };
}

simple_component_arg!(SimpleFileConsumeActionArg, crate::file_action::FileConsumeAction);
simple_component_arg!(SimpleFieEqualCheckerArg, FileEqualsChecker);

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

delegating_impl!(get_set_order_options, Vec<SetOrderOption>, gsoo, Vec::new());
delegating_impl!(get_file_consumer_simple, Vec<SimpleFileConsumeActionArg>, gfcas, Vec::new());
delegating_impl!(get_file_equals_simple, Vec<SimpleFieEqualCheckerArg>, gfeas, Vec::new());

