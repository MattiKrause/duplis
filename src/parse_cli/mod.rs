mod parse_file_size;


use std::ffi::OsString;
use std::num::{NonZeroU32, NonZeroUsize};
use std::sync::Arc;
use clap::{arg, Arg, ArgAction, ArgGroup, value_parser, ValueHint};
use clap::builder::{OsStr, PossibleValue, PossibleValuesParser, ValueParser};

use crate::file_action::{DeleteFileAction, FileConsumeAction, ReplaceWithHardLinkFileAction};
use crate::file_filters::{FileFilter, FileMetadataFilter, MaxSizeFileFilter, MinSizeFileFilter};
use crate::file_set_refiner::{FileContentEquals, FileEqualsChecker};

use crate::os::{SetOrderOption, SimpleFieEqualCheckerArg, SimpleFileConsumeActionArg};
use crate::parse_cli::parse_file_size::{FileSize, FileSizeValueParser};
use crate::set_consumer::{DryRun, FileSetConsumer, InteractiveEachChoice, MachineReadableEach, MachineReadableSet, UnconditionalAction};
use crate::set_order::{CreateTimeSetOrder, ModTimeSetOrder, NameAlphabeticSetOrder, NoopSetOrder, SetOrder};
use crate::util::LinkedPath;

pub struct ExecutionPlan {
    pub dirs: Vec<Arc<LinkedPath>>,
    pub recursive_dirs: Vec<Arc<LinkedPath>>,
    pub follow_symlinks: bool,
    pub file_equals: Vec<Box<dyn FileEqualsChecker + Send>>,
    pub order_set: Vec<Box<dyn SetOrder + Send>>,
    pub action: Box<dyn FileSetConsumer>,
    pub file_filter: FileFilter,
    pub num_threads: NonZeroU32
}

const ACTION_MODE_GROUP: &str = "action_mode";
const ACTION_MODE_ACTION_GROUP: &str = "file_action_action";
const FILE_ACTION_GROUP: &str = "file_action";

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
            .group(ACTION_MODE_GROUP)
            .group(ACTION_MODE_ACTION_GROUP)
        )
        .arg(arg!(iact: -i --interactive "Execute the specified action after confirmation on the console")
            .action(ArgAction::SetTrue)
            .group(ACTION_MODE_GROUP)
            .group(ACTION_MODE_ACTION_GROUP)
        )
        .arg(arg!(machine_readable: --wout "Write all duplicates pairwise to stdout")
            .value_parser([
                PossibleValue::new("pairwise").help("print duplicates in format $original,$duplicate\\n"),
                PossibleValue::new("setwise").help("print entire duplicate sets, with set members separated by comma and sets separated by \\n")
            ])
            .require_equals(true)
             .num_args(0..=1)
            .action(ArgAction::Set)
            .default_missing_value(OsStr::from("pairwise"))
            .group(ACTION_MODE_GROUP)
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
        .arg(arg!(minfsize: --minsize <SIZE> "Only consider files with >= $minsize bytes")
            .action(ArgAction::Set)
            .required(false)
            .value_parser(ValueParser::from(FileSizeValueParser))
        )
        .arg(arg!(maxfsize: --maxsize <SIZE> "Only consider files with < $maxsize bytes")
            .action(ArgAction::Set)
            .required(false)
            .value_parser(ValueParser::from(FileSizeValueParser))
        )
        .arg(arg!(nonzerof: -Z --nonzero "Only consider non-zero sized files").action(ArgAction::SetTrue).required(false))
        .arg(arg!(followsymlink: -s --symlink "Follow symlinks to files and directories").action(ArgAction::SetTrue).required(false))
        .arg(arg!(numthreads: -t -threads <NUM_THREADS>"Use multi-threading(optionally provide the number of threads)").action(ArgAction::Set).required(false).require_equals(true).num_args(0..=1).value_parser(value_parser!(u32)).default_missing_value(OsString::from("0")))
        .group(ArgGroup::new(ACTION_MODE_ACTION_GROUP).requires(FILE_ACTION_GROUP))
        .group(ArgGroup::new(FILE_ACTION_GROUP).requires(ACTION_MODE_ACTION_GROUP));
    for (name, short, long, help, _) in get_file_consume_action_args(){
        command = command.arg(Arg::new(name).short(short).long(long).help(help).action(ArgAction::SetTrue).group(FILE_ACTION_GROUP));
    }
    for (name, short, long, help, _) in get_file_equals_args() {
        command = command.arg(Arg::new(name).short(short).long(long).help(help).action(ArgAction::SetTrue))
    }
    command
}

fn parse_directories(matches: &clap::ArgMatches) -> Vec<Arc<LinkedPath>> {
    matches.get_many::<std::path::PathBuf>("dirs")
        .map(|paths| paths.map(|buf| buf.as_path()).map(LinkedPath::from_path_buf).collect::<Vec<_>>())
        .unwrap_or_else(|| vec![LinkedPath::root(".")])
}

fn parse_set_order(matches: &clap::ArgMatches) -> Vec<Box<dyn SetOrder + Send>> {
    let mut order = match matches.get_many::<String>("setorder") {
        Some(options) => {
            let variants = get_set_order_options();
            options.map(|sname| variants.iter().find(|(name, _, _)| name == sname).unwrap().2.dyn_clone()).collect::<Vec<_>>()
        }
        None => {
            let default: Box<dyn SetOrder + Send> = Box::new(ModTimeSetOrder::new(false));
            vec![default]
        }
    };
    order.reverse();
    order
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

fn get_file_consume_action_args() -> Vec<(&'static str, char, &'static str, String, Box<dyn FileConsumeAction + Send>)> {
    let mut default: Vec<(_, _, _, _, Box<dyn FileConsumeAction + Send>)> = vec![
        ("isdel", 'd', "delete", String::from("Delete duplicated files"), Box::new(DeleteFileAction::default())),
        ("rehl", 'l', "rehardlink", String::from("Replace duplicated files with a hard link"), Box::new(ReplaceWithHardLinkFileAction::default())),
    ];
    let os_specific = crate::os::get_file_consumer_simple()
        .into_iter()
        .map(|SimpleFileConsumeActionArg { name, short, long, help, action }| (name, short, long, help, action));
    default.extend(os_specific);
    default
}

fn get_file_equals_args() -> Vec<(&'static str, char, &'static str, String, Box<dyn FileEqualsChecker + Send>)>{
    let mut default: Vec<(_, _, _, _, Box<dyn FileEqualsChecker + Send>)> = vec![
        ("contenteq", 'c', "contenteq", String::from("compare files byte-by-byte"), Box::new(FileContentEquals::default()))
    ];
    let os_specific = crate::os::get_file_equals_simple()
        .into_iter()
        .map(|SimpleFieEqualCheckerArg { name, short, long, help, action }| (name,short, long, help, action));
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

    let file_filter = {
        let mut metadata_filter: Vec<Box<dyn FileMetadataFilter>> = Vec::new();
        if let Some(filter) = matches.get_one::<FileSize>("maxfsize") {
            metadata_filter.push(Box::new(MaxSizeFileFilter::new(filter.0)))
        }
        if let Some(filter) = matches.get_one::<FileSize>("minfsize") {
            metadata_filter.push(Box::new(MinSizeFileFilter::new(filter.0.saturating_sub(1))))
        }
        if matches.get_flag("nonzerof") {
            metadata_filter.push(Box::new(MinSizeFileFilter::new(0)))
        }
        FileFilter(Box::new([]), metadata_filter.into_boxed_slice())
    };

    let num_threads = match matches.get_one::<u32>("numthreads") {
        Some(0) => {
            u32::try_from(std::thread::available_parallelism().map_or(1, NonZeroUsize::get).saturating_mul(2)).unwrap_or(u32::MAX)
        },
        Some(num) => *num,
        None => 1
    };

    let recurse = matches.get_flag("recurse");
    let follow_symlinks = matches.get_flag("followsymlink");

    let set_ordering = parse_set_order(&matches);

    let file_action: Option<Box<dyn FileConsumeAction + Send>> = get_file_consume_action_args()
        .into_iter()
        .map(|(name, _, _, _, i)| (name, i))
        .find(|(name, _)| matches.get_flag(name))
        .map(|(_, i)| i);

    let file_equals = get_file_equals_args()
        .into_iter()
        .map(|(name, _, _, _, i)| (name, i))
        .filter(|(name, _)|matches.get_flag(name))
        .map(|(_, i)| i)
        .collect::<Vec<_>>();

    let file_set_consumer: Box<dyn FileSetConsumer> = if matches.get_flag("uncond") {
        Box::new(UnconditionalAction::new(file_action.expect("file action should be present because of command config")))
    } else if matches.get_flag("iact") {
        Box::new(InteractiveEachChoice::for_console(file_action.expect("file action should be present because of command config")))
    } else if let Some(kind) = matches.get_one::<String>("machine_readable") {
        match kind.as_str() {
            "pairwise" => Box::new(MachineReadableEach::for_console()),
            "setwise" => Box::new(MachineReadableSet::for_console()),
            _ => panic!("invalid maschine-reable-out config {kind}")
        }
    }else {
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
        follow_symlinks,
        file_equals,
        order_set: set_ordering,
        action: file_set_consumer,
        file_filter,
        num_threads: NonZeroU32::new(num_threads).unwrap(),
    };
    Ok(plan)
}