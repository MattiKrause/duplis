# Duplis
## Overview
Duplis is a tool to find duplicated files. It is accessed using its command line interface where
options and directories to search can be specified.
---
The __Unix__ help message(Some features may not be available on your platform):

```
$ duplis --help

Find duplicate files. You can not only check based on content, but also other(potentially platform dependant) stuff like permissions.
 By default this program simply outputs equal files, in order to actually do something, you need to specify an action like delete


Usage: duplis [OPTIONS] <DIRS|--readin>

Arguments:
  [DIRS]...
          The directories which should be searched for duplicates

Options:
  -r, --recurse
          search all listed directories recursively(requires dirs to be given via cli)

  -s, --symlink
          follow symlinks to files and directories during discovery(requires dirs to be given  via cli)

      --readin
          reads the files which should be tested for duplication from stdin

  -u, --immediate
          Execute the specified action without asking

  -i, --interactive
          Execute the specified action after confirmation on the console

      --wout[=<STRUCTURE>]
          Write all duplicates pairwise to stdout

          Possible values:
          - pairwise: print duplicates in format $original,$duplicate\n
          - setwise:  print entire duplicate sets, with set members separated by comma and sets separated by \n

  -d, --delete
          Delete duplicated files

  -l, --rehardlink
          Replace duplicated files with a hard link

  -L, --resymlink
          replace duplicate files with a symlink

  -t, --threads[=<NUM_THREADS>]
          Use multi-threading(optionally provide the number of threads)

  -o, --orderby <ORDERINGS>
          Set the order in which the elements of equal file sets are ordered
          The smallest is considered the original
          May contain multiple orderings in decreasing importance
          Some orderings may be prefixed with r to reverse(example rmodtime)

          Possible values:
          - modtime:     Order the files from least recently to most recently modified
          - rmodtime:    Order the files from most recently to least recently modified
          - createtime:  Order the files from oldest to newest
          - rcreatetime: Order the files from newest to oldest
          - alphabetic:  Order the files alphabetically ascending(may behave strangely with chars that are not ascii letters or digits)
          - ralphabetic: Order the files alphabetically descending(risks and side effects of 'alphabetic' apply)
          - as_is:       Do not order the files; the order is thus non-deterministic and not reproducible

      --minsize <SIZE>
          Only consider files with >= $minsize bytes

      --maxsize <SIZE>
          Only consider files with < $maxsize bytes

  -Z, --nonzero
          Only consider non-zero sized files

      --extbl <EXTENSIONS>
          files with these extensions are not processed(~ means no extension), extensions must be given without preceding dot("txt" not ".txt")

      --extwl <EXTENSIONS>
          ONLY files with these extensions are processed(~ means no extension), extensions must be given without preceding dot("txt" not ".txt")

      --pathbl <PATHS>
          files with these paths as prefix will not be processed

      --pathblloc <FILES>
          points to files which serve as blacklists for path prefixes(like pathbl), the files must contain a list of \n separated utf-8  encoded paths

  -c, --nocontenteq
          do not compare files byte-by-byte(only by hash)

  -p, --permeq
          do not  consider files with different permissions different files

      --loginfo <INFO>
          update the log targets(+$TARGET turns on, ~$TARGET turns off)
          
          [possible values: ~user_interaction_err, +user_interaction_err, ~file_format_err, +file_format_err, ~config_err, +config_err, ~fatal_action_failure, +fatal_action_failure, ~action_success, +action_success, ~file_discovery_err, +file_discovery_err, ~file_error, +file_error, ~file_set_err, +file_set_err]

      --setloginfo <INFO>
          set the log targets to be logged
          
          [possible values: user_interaction_err, file_format_err, config_err, fatal_action_failure, action_success, file_discovery_err, file_error, file_set_err, ~]

  -h, --help
          Print help (see a summary with '-h')

```
---
## Installation
1. Download [Rust and Cargo](https://github.com/rust-lang/rust)
2. Run `cargo build --release`
3. The resulting executable resides in `./target/release/duplis` 