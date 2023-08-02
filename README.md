# Duplis
## Overview
Duplis is a tool to find duplicated files. It is accessed using its command line interface where
options and directories to search can be specified.
---
The __Unix__ help message(Some features may not be available on your platform):
```bash
$ duplis --help
Find duplicate files. You can not only check based on content, but also other(potentially platform dependant) stuff like permissions.
 By default this program simply outputs equal files, in order to actually do something, you need to specify an action like delete


Usage: duplis [OPTIONS] [DIRS]...

Arguments:
  [DIRS]...
          The directories which should be searched for duplicates(Defaults to '.')

Options:
  -u, --immediate
          Execute the specified action without asking

  -i, --interactive
          Execute the specified action after confirmation on the console

      --wout[=<STRUCTURE>]
          Write all duplicates pairwise to stdout

          Possible values:
          - pairwise: print duplicates in format $original,$duplicate\n
          - setwise:  print entire duplicate sets, with set members separated by comma and sets separated by \n

  -r, --recurse
          search all listed directories recursively

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

  -s, --symlink
          Follow symlinks to files and directories

  -t, --threads[=<NUM_THREADS>]
          Use multi-threading(optionally provide the number of threads)

  -d, --delete
          Delete duplicated files

  -l, --rehardlink
          Replace duplicated files with a hard link

  -L, --resymlink
          replace duplicate files with a symlink

  -c, --contenteq
          compare files byte-by-byte

  -p, --permeq
          consider files with different permissions different files

  -h, --help
          Print help (see a summary with '-h')
```
---
## Installation
1. Download [Rust and Cargo](https://github.com/rust-lang/rust)
2. Run `cargo build --release`
3. The resulting executable resides in `./target/release/duplis` 