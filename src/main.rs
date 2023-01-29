use std::ffi::{CString, OsStr, OsString};
use std::os::unix::ffi::OsStrExt;

use clap::Parser;

/// Run a program on the (discrete) GPU.
#[derive(Parser)]
struct Args {
    // TODO: this is going to be used for logging, at some point.
    /// A name for the game, this is used to build the filename for the log file.
    game_name: String,
    /// The arguments of the command to run, including the binary. E.g., `space-game --aliens`.
    command: Vec<OsString>,
}

fn main() {
    let args = Args::parse();
    if args.command.is_empty() {
        eprintln!("Need at least 1 argument for the command to run.");
        std::process::exit(1);
    }
    println!("== Start ==");
    println!("CWD: {:?}", std::env::current_dir());
    println!("Arguments:");
    for (idx, arg) in args.command.iter().enumerate() {
        println!("  argv[{}] = {:?}", idx, arg);
    }

    let mut to_exec_args = Vec::<CString>::new();
    to_exec_args.push(string_to_cstring("pvkrun".to_owned()));
    for arg in args.command.iter() {
        to_exec_args.push(os_str_to_cstring(arg));
    }
    nix::unistd::execvp(&to_exec_args[0], &to_exec_args).unwrap();
}

fn os_str_to_cstring(s: &OsStr) -> CString {
    let mut bytes = s.as_bytes().to_owned();
    bytes.push(0);
    CString::from_vec_with_nul(bytes).unwrap()
}

fn string_to_cstring(s: String) -> CString {
    let mut bytes = s.into_bytes();
    bytes.push(0);
    CString::from_vec_with_nul(bytes).unwrap()
}
