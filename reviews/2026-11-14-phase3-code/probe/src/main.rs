use clap::Parser;
use oplus_edl::{
    backup,
    cli::{Cli, Command},
    device,
    output::{Report, Status, Target},
};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args_os().collect();
    let json = args.iter().any(|v| v == "--json");
    let cli = match Cli::try_parse_from(&args) {
        Ok(cli) => cli,
        Err(e)
            if matches!(
                e.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            ) =>
        {
            print!("{e}");
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            let command = args
                .iter()
                .filter_map(|v| v.to_str())
                .find(|v| matches!(*v, "backup" | "restore" | "flash" | "unlock" | "lock"));
            let mut report = Report::new(command, None);
            report.error(Status::InputError, e.to_string());
            report.emit(json);
            return ExitCode::from(report.status.exit_code());
        }
    };
    let mut report = Report::new(Some(cli.command.name()), cli.command.verify());
    run(&cli, &mut report);
    report.summarize();
    report.emit(cli.json);
    ExitCode::from(report.status.exit_code())
}
fn run(cli: &Cli, report: &mut Report) {
    let (output, names, overwrite) = match &cli.command {
        Command::Unlock | Command::Lock => {
            report.error(Status::Unsupported, "no validated model-specific method");
            return;
        }
        Command::Restore { .. } | Command::Flash { .. } => {
            report.error(Status::Unsupported, "public writes disabled until verification and stop policy are implemented (phase 4)");
            return;
        }
        Command::Backup {
            output,
            partition,
            overwrite,
        } => (output, partition, *overwrite),
    };
    let config = match device::configuration(cli) {
        Ok(c) => c,
        Err(e) => {
            report.error(Status::InputError, e.to_string());
            return;
        }
    };
    let Some(selected) = cli.device.as_deref() else {
        match device::candidates(cli.backend) {
            Ok(keys) => {
                report.candidates = keys;
                report.error(
                    Status::InputError,
                    "select an explicit --device; no loader uploaded",
                );
            }
            Err(e) => report.error(Status::ExecutionFailed, format!("enumeration: {e:#}")),
        }
        return;
    };
    let Some(loader) = &cli.loader else {
        report.error(Status::InputError, "--loader is required for backup");
        return;
    };
    let mut images = match qdl::load_programmer_images(loader) {
        Ok(v) if v.iter().flatten().any(|i| !i.is_empty()) => v,
        Ok(_) => {
            report.error(Status::InputError, "empty loader");
            return;
        }
        Err(e) => {
            report.error(Status::InputError, format!("{e:#}"));
            return;
        }
    };
    let keys = match device::candidates(cli.backend) {
        Ok(v) => v,
        Err(e) => {
            report.error(Status::ExecutionFailed, e.to_string());
            return;
        }
    };
    if let Err(e) = qdl::select_and_open(&keys, Some(selected), |_| Ok(())) {
        report.candidates = keys;
        report.error(Status::InputError, e.to_string());
        return;
    }
    report.target = Some(Target {
        device: Some(selected.into()),
        backend: Some(format!("{:?}", config.backend).to_lowercase()),
        memory: Some(config.storage_type.to_string()),
        luns: cli.lun.into_iter().collect(),
    });
    eprintln!(
        "Backup LUN {:?}, partitions {:?}, sector size {} ({}), output {}",
        cli.lun,
        names,
        config.storage_sector_size,
        if cli.sector_size.is_some() {
            "user supplied"
        } else {
            "media default, not measured"
        },
        output.display()
    );
    let mut channel = match device::open(cli.backend, selected, config) {
        Ok(v) => v,
        Err(e) => {
            report.error(Status::ExecutionFailed, format!("open: {e:#}"));
            return;
        }
    };
    if let Err(e) = device::initialize(&mut channel, &mut images) {
        report.error(Status::ExecutionFailed, format!("initialize: {e:#}"));
        return;
    }
    let luns = match cli.lun {
        Some(lun) => vec![lun],
        None => match qdl::firehose_get_storage_info(&mut channel, 0) {
            Ok(info) => {
                match oplus_edl::storage::discovered_luns(&info, channel.fh_cfg.storage_sector_size)
                {
                    Ok(luns) => luns,
                    Err(e) => {
                        report.error(Status::Unsupported, format!("{e:#}"));
                        return;
                    }
                }
            }
            Err(e) => {
                report.error(Status::ExecutionFailed, format!("storage discovery: {e:#}"));
                return;
            }
        },
    };
    if let Some(target) = &mut report.target {
        target.luns = luns.clone();
    }
    backup::directory(&mut channel, &luns, names, output, overwrite, report);
}
