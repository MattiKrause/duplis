
use crate::set_order::SetOrder;

#[cfg(unix)]
mod unix_specific;
#[cfg(unix)]
use unix_specific::{get_set_order_options as gsoo, get_file_consume_action_simple as gfcas, get_file_equals_arg_simple as gfeas};
use crate::file_set_refiner::FileEqualsChecker;
use crate::set_consumer::FileConsumeAction;

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

simple_component_arg!(SimpleFileConsumeActionArg, FileConsumeAction);
simple_component_arg!(SimpleFieEqualCheckerArg, FileEqualsChecker);


pub fn get_set_order_options() -> Vec<SetOrderOption> {
    gsoo()
}

pub fn get_file_consumer_simple() -> Vec<SimpleFileConsumeActionArg> {
    gfcas()
}
pub fn get_file_equals_simple() -> Vec<SimpleFieEqualCheckerArg> {
    gfeas()
}