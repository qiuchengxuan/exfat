mod cat;
mod list;
mod touch;

use clap::Parser;

#[derive(Debug, clap::Args)]
struct List {
    /// Block device or file that formatted with ExFAT
    device: String,
    /// Specify path to list, default to root directory
    #[clap(default_value = "/")]
    path: String,
}

#[derive(Debug, clap::Args)]
struct Cat {
    /// Block device or file that formatted with ExFAT
    device: String,
    /// Specify path to concatenate
    path: String,
}

#[derive(Debug, clap::Args)]
struct Touch {
    /// Block device or file that formatted with ExFAT
    device: String,
    /// Specify path to touch
    path: String,
}

#[derive(Debug, clap::Subcommand)]
enum Action {
    /// List file and directory in specified path
    List(List),
    /// Concatenate file and print on the standard output
    Cat(Cat),
    /// Change file timestamps
    Touch(Touch),
}

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    #[clap(subcommand)]
    action: Action,
}

fn main() {
    log::set_max_level(log::LevelFilter::Debug);
    simple_log::console("debug").ok();
    let args = Args::parse();
    let result = match args.action {
        Action::List(args) => list::list(args.device, args.path),
        Action::Cat(args) => cat::cat(args.device, args.path),
        Action::Touch(args) => touch::touch(args.device, args.path),
    };
    if let Some(error) = result.err() {
        eprintln!("{}", error);
        std::process::exit(1);
    }
}
