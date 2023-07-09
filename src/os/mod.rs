
use crate::set_order::SetOrder;

#[cfg(unix)]
mod unix_specific;
#[cfg(unix)]
use unix_specific::{get_set_order_options as gsoo, get_file_consume_action_simple as gfcas};
use crate::set_consumer::FileConsumeAction;

pub struct  SetOrderOption {
    pub name: &'static str,
    pub help: String,
    pub implementation: Box<dyn SetOrder>
}

pub struct SimpleFileConsumeActionArg {
    pub name: &'static str,
    pub short: char,
    pub long: &'static str,
    pub help: String,
    pub action: Box<dyn FileConsumeAction>
}

pub fn get_set_order_options() -> Vec<SetOrderOption> {
    gsoo()
}

pub fn get_file_consumer_simple() -> Vec<SimpleFileConsumeActionArg> {
    gfcas()
}