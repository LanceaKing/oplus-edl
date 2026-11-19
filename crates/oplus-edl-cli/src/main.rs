use clap::Parser;
use oplus_edl_cli::{
    OutputMode,
    args::{Cli, early_output},
    deliver_information, deliver_result, run_with_service,
};
use oplus_edl_core::{
    CoreService, HostServices,
    result::{Report, Status},
};
use std::{
    ffi::OsString,
    process::ExitCode,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

static STOP_SIGNAL: AtomicBool = AtomicBool::new(false);

#[cfg(unix)]
fn install_signal() -> std::io::Result<()> {
    extern "C" fn stop(_: libc::c_int) {
        STOP_SIGNAL.store(true, Ordering::Relaxed);
    }
    // The handler only writes to a process-local atomic flag.
    unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_flags = libc::SA_RESTART;
        action.sa_sigaction = stop as *const () as usize;
        libc::sigemptyset(&mut action.sa_mask);
        if libc::sigaction(libc::SIGINT, &action, std::ptr::null_mut()) != 0 {
            return Err(std::io::Error::last_os_error());
        }
    }
    Ok(())
}

#[cfg(windows)]
fn install_signal() -> std::io::Result<()> {
    use windows_sys::Win32::System::Console::{
        CTRL_BREAK_EVENT, CTRL_C_EVENT, SetConsoleCtrlHandler,
    };
    unsafe extern "system" fn stop(event: u32) -> i32 {
        if matches!(event, CTRL_C_EVENT | CTRL_BREAK_EVENT) {
            STOP_SIGNAL.store(true, Ordering::Relaxed);
            1
        } else {
            0
        }
    }
    if unsafe { SetConsoleCtrlHandler(Some(stop), 1) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args_os().collect();
    let code = match Cli::try_parse_from(&args) {
        Ok(cli) => {
            if let Err(error) = install_signal() {
                let mut result = Report::new(Some(cli.command.name()), cli.command.verify());
                result.error(
                    Status::ExecutionFailed,
                    format!("install stop signal handler: {error}"),
                );
                deliver_result(
                    std::io::stdout(),
                    OutputMode {
                        json: cli.json,
                        quiet: cli.quiet,
                    },
                    result,
                )
            } else {
                let done = Arc::new(AtomicBool::new(false));
                let code = run_with_service(
                    cli,
                    CoreService::new(HostServices::native()),
                    std::io::stdout(),
                    {
                        let done = done.clone();
                        move |relay| {
                            if STOP_SIGNAL.swap(false, Ordering::Relaxed) {
                                relay();
                            }
                            thread::spawn(move || {
                                while !done.load(Ordering::Acquire) {
                                    if STOP_SIGNAL.swap(false, Ordering::Relaxed) {
                                        relay();
                                    }
                                    thread::sleep(Duration::from_millis(10));
                                }
                            });
                        }
                    },
                );
                done.store(true, Ordering::Release);
                code
            }
        }
        Err(error) => render_parse_error(&args, error),
    };
    ExitCode::from(code as u8)
}

fn render_parse_error(args: &[OsString], error: clap::Error) -> i32 {
    let (mode, operation) = early_output(args);
    let writer = std::io::stdout();
    use clap::error::ErrorKind;
    match error.kind() {
        ErrorKind::DisplayHelp => deliver_information(writer, mode, "help", error.to_string()),
        ErrorKind::DisplayVersion => {
            deliver_information(writer, mode, "version", error.to_string())
        }
        _ => {
            let mut report = Report::new(operation, None);
            report.error(Status::InputError, error.to_string());
            deliver_result(writer, mode, report)
        }
    }
}
