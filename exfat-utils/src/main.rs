#[macro_use]
extern crate log;

mod append;
mod cat;
pub(crate) mod filepath;
mod list;
mod put;
mod remove;
mod sdmmc;
mod touch;
mod truncate;

use std::fmt::Debug;

use clap::Parser;
use exfat::error::Error;
use exfat::io::std::FileIO;
use exfat::{DateTime, ExFAT};

#[derive(Debug, clap::Args)]
struct List {
    /// Specify path to list, default to root directory
    #[clap(default_value = "/")]
    path: String,
}

#[derive(Debug, clap::Args)]
struct Cat {
    /// Specify path to concatenate
    path: String,
}

#[derive(Debug, clap::Args)]
struct Touch {
    /// Specify path to touch
    path: String,
}

#[derive(Debug, clap::Args)]
struct Append {
    /// Specify path to touch
    path: String,
    /// Specify source file to append
    source: String,
}

#[derive(Debug, clap::Args)]
struct Truncate {
    /// Specify path to touch
    path: String,
    /// Specify size to truncate
    size: u64,
}

#[derive(Debug, clap::Args)]
struct Put {
    path: String,
    source: String,
}

#[derive(Debug, clap::Args)]
struct Remove {
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
    /// Append to file
    Append(Append),
    /// Truncate file
    Truncate(Truncate),
    /// Put file
    Put(Put),
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
    /// Block device, SPI device or file
    #[clap(short, long)]
    device: String,
    /// Specify chip-select GPIO pin
    #[clap(long)]
    cs: Option<u16>,
    /// Specify partition index
    #[clap(long)]
    partition: Option<u8>,
    #[clap(subcommand)]
    action: Action,
}

#[no_mangle]
fn exfat_datetime_now() -> DateTime {
    let now = chrono::Utc::now();
    now.into()
}

fn action<E, IO>(io: IO, action: Action) -> Result<(), Error<E>>
where
    E: std::fmt::Debug,
    IO: exfat::io::IO<Error = E>,
{
    let mut exfat = ExFAT::new(io)?;
    exfat.validate_checksum()?;
    let mut root = exfat.root_directory()?;
    root.validate_upcase_table_checksum()?;

    match action {
        Action::List(args) => list::list(&mut root, &args.path),
        Action::Cat(args) => cat::cat(&mut root, &args.path),
        Action::Touch(args) => touch::touch(&mut root, &args.path),
        Action::Append(args) => append::append(&mut root, &args.path, &args.source),
        Action::Truncate(args) => truncate::truncate(&mut root, &args.path, args.size),
        Action::Put(args) => put::put(&mut root, &args.path, &args.source),
        Action::Remove(args) => remove::remove(&mut root, &args.path),
    }
}

fn display_error<E: std::fmt::Display>(error: E) -> () {
    eprintln!("{}", error);
    ()
}

fn debug_error<E: std::fmt::Debug>(error: E) -> () {
    eprintln!("{:?}", error);
    ()
}

fn run(args: Args) -> Result<(), ()> {
    if args.device.starts_with("/dev/spidev") {
        let cs = args.cs.ok_or("CS is required for SPI device").map_err(display_error)?;
        let mut sdmmc = sdmmc::SDMMC::new(&args.device, cs).map_err(display_error)?;
        if let Some(partition) = args.partition {
            sdmmc.set_patition(partition as usize).map_err(display_error)?;
        }
        action(sdmmc, args.action).map_err(debug_error)
    } else {
        let file = FileIO::open(&args.device).map_err(display_error)?;
        action(file, args.action).map_err(display_error)
    }
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
    if run(args).is_err() {
        std::process::exit(1);
    }
}
