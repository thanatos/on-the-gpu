use std::env;
use std::ffi::{CString, OsStr, OsString};
use std::fs::File;
use std::io::Write;
use std::os::fd::{AsRawFd, BorrowedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use anyhow::Context;
use clap::Parser;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Run a program on the (discrete) GPU.
#[derive(Parser)]
struct Args {
    /// A name for the game, this is used to build the filename for the log file.
    game_name: String,
    #[arg(long)]
    logs: bool,
    /// The arguments of the command to run, including the binary. E.g., `space-game --aliens`.
    command: Vec<OsString>,
    /// Whether and how to run a game on the GPU. Defaults to Vulkan (i.e., under `pkkrun`),
    #[arg(long)]
    gpu: Option<GpuMode>,
}

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
enum GpuMode {
    None,
    /// Run a Vulkan game on the GPU. Wraps it in `pvkrun`.
    Vulkan,
    /// Wrap a game in `primusrun`; this is the newer wrapper.
    Primus,
    /// Wrap a game in `optirun`; this is the older wrapper.
    Optirun,
}

fn main() {
    let args = Args::parse();
    log_run(&args).unwrap();

    if args.command.is_empty() {
        eprintln!("Need at least 1 argument for the command to run.");
        std::process::exit(1);
    }

    let gpu_mode = args.gpu.unwrap_or(GpuMode::Vulkan);

    let cmd_to_run = {
        match gpu_mode {
            GpuMode::None => args.command,
            GpuMode::Vulkan => {
                let mut cmd = Vec::<OsString>::new();
                cmd.push("pvkrun".to_owned().into());
                cmd.extend(args.command);
                cmd
            },
            GpuMode::Primus => {
                let mut cmd = Vec::<OsString>::new();
                cmd.push("primusrun".to_owned().into());
                cmd.extend(args.command);
                cmd
            },
            GpuMode::Optirun => {
                let mut cmd = Vec::<OsString>::new();
                cmd.push("optirun".to_owned().into());
                cmd.extend(args.command);
                cmd
            },
        }
    };

    if args.logs {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .build()
            .unwrap();
        rt.block_on(run_game_with_logs(&args.game_name, cmd_to_run)).unwrap();
    } else {
        print_cmd(
            std::io::stderr(),
            cmd_to_run.iter().map(|a| a.as_os_str()),
        )
        .unwrap();

        let cmd_to_run = cmd_to_run.into_iter().map(|arg| os_str_to_cstring(&arg)).collect::<Vec<_>>();

        nix::unistd::execvp(&cmd_to_run[0], &cmd_to_run).unwrap();
    }
}

fn print_cmd<'a>(
    mut w: impl std::io::Write,
    command: impl IntoIterator<Item = &'a OsStr>,
) -> std::io::Result<()> {
    writeln!(&mut w, "══ Start ══")?;
    writeln!(&mut w, "CWD: {:?}", std::env::current_dir())?;
    writeln!(&mut w, "Arguments:")?;
    for (idx, arg) in command.into_iter().enumerate() {
        writeln!(&mut w, "  argv[{idx}] = {arg:?}")?;
    }
    Ok(())
}

fn os_str_to_cstring(s: &OsStr) -> CString {
    let mut bytes = s.as_bytes().to_owned();
    bytes.push(0);
    CString::from_vec_with_nul(bytes).unwrap()
}

fn get_logs_dir() -> PathBuf {
    let mut p = PathBuf::from(env::var_os("HOME").unwrap());
    p.push("games");
    p.push("logs");
    p
}

fn build_log_filename(base_name: &str, overridden_logs_dir: Option<&Path>) -> PathBuf {
    let now = chrono::offset::Local::now();
    let filename = format!("{}_{}.log.zstd", base_name, now.to_rfc3339());
    match overridden_logs_dir {
        Some(p) => p.join(filename),
        None => {
            let mut p = get_logs_dir();
            p.push(filename);
            p
        }
    }
}

async fn run_game_with_logs(game_name: &str, command: Vec<OsString>) -> anyhow::Result<()> {
    let cmd_bin = &command[0];

    let log_path = build_log_filename(game_name, None);
    let log_file = File::options()
        .create_new(true)
        .write(true)
        .open(log_path)
        .context("failed to open log file for writing")?;
    let log_file = tokio::fs::File::from_std(log_file);
    let mut log_file = async_compression::tokio::write::ZstdEncoder::new(log_file);

    let mut stderr = tokio::io::stderr();

    let mut intro = Vec::<u8>::new();
    print_cmd(&mut intro, command.iter().map(|a| a.as_os_str())).unwrap();
    stderr.write_all(&intro).await?;
    log_file.write_all(&intro).await?;

    let (mut r, w) = tokio_pipe::pipe()?;
    let (cmd_stdout, cmd_stderr) = {
        let w_fd = unsafe { BorrowedFd::borrow_raw(w.as_raw_fd()) };
        let cmd_stdout = Stdio::from(w_fd.try_clone_to_owned().unwrap());
        let cmd_stderr = Stdio::from(w_fd.try_clone_to_owned().unwrap());
        /*
        let cmd_stdout = unsafe { Stdio::from_raw_fd(w_fd) };
        let cmd_stderr = unsafe { Stdio::from_raw_fd(w_fd) };
        */
        (cmd_stdout, cmd_stderr)
    };

    // `w` is closed during this call.
    // This call SIGABRTs
    let mut child = tokio::process::Command::new(cmd_bin)
        .args(&command[1..])
        .stdout(cmd_stdout)
        .stderr(cmd_stderr)
        .spawn()
        .context("failed to spawn child process")?;

    // Close the write end of the pipe. MUST happen after the spawn() call.
    drop(w);

    log_file.write_all(b"Game started.\n").await?;
    tee(&mut r, &mut stderr, &mut log_file).await?;
    child.wait().await?;
    drop(r);
    drop(child);

    Ok(())
}

/// Tee an input to two outputs, like the `tee` command line utility.
async fn tee(
    mut rdr: impl AsyncRead + Unpin,
    mut a: impl AsyncWrite + Unpin,
    mut b: impl AsyncWrite + Unpin,
) -> std::io::Result<()> {
    let mut buf = [0u8; 1024];
    loop {
        let len = rdr.read(&mut buf).await?;
        if len == 0 {
            break;
        }
        let (a_write, b_write) = tokio::join!(a.write_all(&buf[..len]), b.write_all(&buf[..len]),);
        a_write?;
        b_write?;
    }
    a.shutdown().await?;
    b.shutdown().await?;
    Ok(())
}

/// Log that we're alive & actually running.
///
/// Sometimes, it is hard to tell if the thing executing us (e.g., Steam) is even doing that much.
fn log_run(args: &Args) -> anyhow::Result<()> {
    let log_path = {
        let mut p = PathBuf::from(env::var_os("HOME").ok_or_else(|| anyhow::anyhow!("$HOME is unset?"))?);
        p.push("games");
        p.push("logs");
        p.push("on-the-gpu--last-run.log");
        p
    };

    let mut last_log = File::create(&log_path)?;
    writeln!(last_log, "Our arguments:")?;
    for (idx, arg) in env::args().enumerate() {
        writeln!(last_log, "  argv[{idx}] = {arg:?}")?;
    }
    writeln!(last_log, "The command we're to run, as parsed by `clap`:")?;
    for (idx, arg) in args.command.iter().enumerate() {
        writeln!(last_log, "  cmd[{idx}] = {arg:?}")?;
    }
    writeln!(last_log, "Done.")?;
    Ok(())
}
