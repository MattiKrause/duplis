use std::sync::Arc;
use clap::{arg, Arg, ArgAction, ArgGroup, value_parser, ValueHint};
use clap::builder::{PossibleValue, PossibleValuesParser};

use crate::LinkedPath;
use crate::os::{SetOrderOption, SimpleFileConsumeActionArg};
use crate::set_consumer::{DeleteFileAction, DryRun, FileConsumeAction, FileSetConsumer, InteractiveEachChoice, ReplaceWithHardLinkFileAction, UnconditionalAction};
use crate::set_order::{CreateTimeSetOrder, ModTimeSetOrder, NameAlphabeticSetOrder, NoopSetOrder, SetOrder};

pub struct ExecutionPlan {
    pub dirs: Vec<Arc<LinkedPath>>,
    pub recursive_dirs: Vec<Arc<LinkedPath>>,
    pub follow_symlinks: bool,
    pub order_set: Vec<Box<dyn SetOrder>>,
    pub action: Box<dyn FileSetConsumer>,
}

fn assemble_command_info() -> clap::Command {
    let mut command = clap::Command::new("rrem_fast")
        .arg(arg!(dirs: <DIRS> "The directories which should be searched for duplicates")
            .value_hint(ValueHint::DirPath)
            .value_parser(value_parser!(std::path::PathBuf))
            .action(ArgAction::Append)
            .required(false)
        )
        .arg(arg!(uncond: -u --immediate "Execute the specified action without asking")
            .action(ArgAction::SetTrue)
            .group("action_mode")
            .group("action_mode_action")
        )
        .arg(arg!(iact: -i --interactive "Execute the specified action after confirmation on the console")
            .action(ArgAction::SetTrue)
            .group("action_mode")
            .group("action_mode_action")
        )
        .arg(arg!(recurse: -r --recurse "search all listed directories recursively")
            .action(ArgAction::SetTrue)
        )
        .arg(arg!(setorder: -o --orderby <ORDERINGS>)
            .action(ArgAction::Append)
            .value_delimiter(',')
            .value_parser(set_order_parser())
            .help("set the order in which the elements of equal file sets are ordered; the smallest is considered the original; may contain multiple orderings in decreasing importance; some orderings may be prefixed with r to reverse(example rmodtime)")
            .required(false)
        )
        .group(ArgGroup::new("action_mode_action").requires("file_action"))
        .group(ArgGroup::new("file_action").requires("action_mode_action"));
    for (name, short, long, help, _) in get_file_consume_action_args(){
        command = command.arg(Arg::new(name).short(short).long(long).help(help).action(ArgAction::SetTrue).group("file_action"));
    }
    command
}

fn parse_directories(matches: &clap::ArgMatches) -> Vec<Arc<LinkedPath>> {
    matches.get_many::<std::path::PathBuf>("dirs")
        .map(|paths| paths.map(LinkedPath::from_path_buf).collect::<Vec<_>>())
        .unwrap_or_else(|| vec![LinkedPath::root(".")])
}

fn parse_set_order(matches: &clap::ArgMatches) -> Vec<Box<dyn SetOrder>> {
    match matches.get_many::<String>("setorder") {
        Some(options) => {
            let variants = get_set_order_options();
            options.map(|sname| variants.iter().find(|(name, _, _)| name == sname).unwrap().2.boxed_dyn_clone()).collect::<Vec<_>>()
        }
        None => {
            let default: Box<dyn SetOrder> = Box::new(ModTimeSetOrder::new(false));
            vec![default]
        }
    }
}

fn get_set_order_options() -> Vec<(&'static str, String, Box<dyn SetOrder>)> {
    let default_order_options: Vec<(&'static str, Box<dyn SetOrder>, &'static str)> = vec![
        ("modtime", Box::new(ModTimeSetOrder::new(false)), "Order the files from least recently to most recently modified"),
        ("rmodtime", Box::new(ModTimeSetOrder::new(true)), "Order the files from most recently to least recently modified"),
        ("createtime", Box::new(CreateTimeSetOrder::new(false)), "Order the files from oldest to newest"),
        ("rcreatetime", Box::new(CreateTimeSetOrder::new(true)), "Order the files from newest to oldest"),
        ("alphabetic", Box::new(NameAlphabeticSetOrder::new(false)), "Order the files alphabetically ascending(may behave strangely with chars that are not ascii letters or digits)"),
        ("ralphabetic", Box::new(NameAlphabeticSetOrder::new(true)), "Order the files alphabetically descending(risks and side effects of 'alphabetic' apply)"),
        ("as_is", Box::new(NoopSetOrder::new()), "Do not order the files; the order is thus non-deterministic and not reproducible")
    ];
    let default_order_options= default_order_options.into_iter()
        .map(|(name, action, help)| (name, String::from(help), action));

    let os_options = crate::os::get_set_order_options().into_iter()
        .map(|SetOrderOption { name, help, implementation }| (name, help, implementation));

    default_order_options.chain(os_options).collect::<Vec<_>>()
}

fn get_file_consume_action_args() -> Vec<(&'static str, char, &'static str, String, Box<dyn FileConsumeAction>)> {
    let mut default: Vec<(_, _, _, _, Box<dyn FileConsumeAction>)> = vec![
        ("isdel", 'd', "delete", String::from("Delete duplicated files"), Box::new(DeleteFileAction::default())),
        ("rehl", 'l', "rehardlink", String::from("Replace duplicated files with a hard link"), Box::new(ReplaceWithHardLinkFileAction::default())),
    ];
    let os_specific = crate::os::get_file_consumer_simple()
        .into_iter()
        .map(|SimpleFileConsumeActionArg { name, short, long, help, action }| (name, short, long, help, action));
    default.extend(os_specific);
    default

}

fn set_order_parser() -> clap::builder::ValueParser {
    let values = get_set_order_options()
        .into_iter()
        .map(|(name, help, _)| PossibleValue::new(name.clone()).help(help))
        .collect::<Vec<_>>();

    PossibleValuesParser::new(values)
        .into()
}


pub fn parse() -> Result<ExecutionPlan, ()> {
    let matches = assemble_command_info()
        .get_matches();
    //let x = matches.get_many::<usize>("oi").unwrap();

    let mut dirs = parse_directories(&matches);
    let mut rec_dirs = vec![];

    let recurse = matches.get_flag("recurse");

    let set_ordering = parse_set_order(&matches);

    let file_action: Option<Box<dyn FileConsumeAction>> = get_file_consume_action_args()
        .into_iter()
        .map(|(name, _, _, _, i)| (name, i))
        .find(|(name, _)| matches.get_flag(name))
        .map(|(_, i)| i);

    let file_set_consumer: Box<dyn FileSetConsumer> = if matches.get_flag("uncond") {
        Box::new(UnconditionalAction::new(file_action.expect("file action should be present per command config")))
    } else if matches.get_flag("iact") {
        Box::new(InteractiveEachChoice::for_console(file_action.expect("file action should be present per command config")))
    } else {
        Box::new(DryRun::for_console())
    };

    let (dirs, recursive_dirs) = if recurse {
        rec_dirs.append(&mut dirs);
        (dirs, rec_dirs)
    } else {
        (dirs, rec_dirs)
    };

    let plan = ExecutionPlan {
        dirs,
        recursive_dirs,
        follow_symlinks: false,
        order_set: set_ordering,
        action: file_set_consumer,
    };
    Ok(plan)
}