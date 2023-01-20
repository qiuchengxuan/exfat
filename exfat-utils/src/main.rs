mod cat;
pub(crate) mod filepath;
mod list;
mod remove;
mod touch;
mod truncate;

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

#[derive(Debug, clap::Args)]
struct Truncate {
    /// Block device or file that formatted with ExFAT
    device: String,
    /// Specify path to touch
    path: String,
    /// Specify size to truncate
    size: u64,
}

#[derive(Debug, clap::Args)]
struct Remove {
    /// Block device or file that formatted with ExFAT
    device: String,
    /// Specify path to delete
    path: String,
}

#[derive(Debug, clap::Subcommand)]
enum Action {
    /// List file and directory in specified path
    #[clap(name = "ls")]
    List(List),
    /// Concatenate file and print on the standard output
    Cat(Cat),
    /// Change file timestamps
    Touch(Touch),
    /// Truncate file
    Truncate(Truncate),
    /// Remove file
    #[clap(name = "rm")]
    Remove(Remove),
}

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    #[clap(short, long)]
    quiet: bool,
    #[clap(short, action = clap::ArgAction::Count)]
    verbosity: u8,
    #[clap(subcommand)]
    action: Action,
}

fn main() {
    let args = Args::parse();
    let level = match (args.quiet, args.verbosity) {
        (true, _) => log::LevelFilter::Off,
        (_, 0) => log::LevelFilter::Info,
        (_, 1) => log::LevelFilter::Debug,
        (_, _) => log::LevelFilter::Trace,
    };
    log::set_max_level(level);
    env_logger::builder().filter(None, level).target(env_logger::Target::Stdout).init();
    let result = match args.action {
        Action::List(args) => list::list(&args.device, &args.path),
        Action::Cat(args) => cat::cat(&args.device, &args.path),
        Action::Touch(args) => touch::touch(&args.device, &args.path),
        Action::Truncate(args) => truncate::truncate(&args.device, &args.path, args.size),
        Action::Remove(args) => remove::remove(&args.device, &args.path),
    };
    if let Some(error) = result.err() {
        eprintln!("{}", error);
        std::process::exit(1);
    }
}
