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
    if matches!(cli.command, Command::Unlock | Command::Lock) {
        report.error(Status::Unsupported, "no validated model-specific method");
        return;
    }
    let config = match device::configuration(cli) {
        Ok(c) => c,
        Err(e) => {
            report.error(Status::InputError, e.to_string());
            return;
        }
    };
    let prepared_flash = if let Command::Flash {
        program,
        patch,
        set_bootable,
        reboot,
        verify,
        ..
    } = &cli.command
    {
        let candidates = match oplus_edl::program_xml::parse(
            program,
            patch,
            config.storage_sector_size as u64,
            false,
        ) {
            Ok(v) => v,
            Err(e) => {
                report.error(
                    if e.is::<oplus_edl::program_xml::Unsupported>() {
                        Status::Unsupported
                    } else {
                        Status::InputError
                    },
                    e.to_string(),
                );
                return;
            }
        };
        if cli.lun.is_some_and(|lun| {
            candidates.iter().any(|c| c.lun != lun) || set_bootable.is_some_and(|n| n != lun)
        }) {
            report.error(
                Status::InputError,
                "XML/final action LUN conflicts with --lun",
            );
            return;
        }
        let writes = match oplus_edl::write::Writes::flash(
            candidates,
            config.storage_sector_size as u64,
            *set_bootable,
            *reboot,
        ) {
            Ok(v) => v,
            Err(e) => {
                report.error(Status::Unsupported, e.to_string());
                return;
            }
        };
        if let Err(e) = writes.preflight(*verify) {
            writes.reject(report, Status::Unsupported, &e.to_string());
            return;
        }
        Some(writes)
    } else {
        None
    };
    if let Command::Restore {
        from,
        image,
        partition,
        ..
    } = &cli.command
    {
        if image.is_some() && partition.len() != 1 {
            report.error(
                Status::InputError,
                "single image requires exactly one partition",
            );
            return;
        }
        if from.as_ref().is_some_and(|p| !p.is_dir())
            || image.as_ref().is_some_and(|p| !p.is_file())
        {
            report.error(
                Status::InputError,
                "restore source is missing or has the wrong file type",
            );
            return;
        }
    }
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
        report.error(Status::InputError, "--loader is required");
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
        "{}: device {} backend {:?} memory {}; sector size {} ({})",
        cli.command.name(),
        selected,
        cli.backend,
        config.storage_type,
        config.storage_sector_size,
        if cli.sector_size.is_some() {
            "user supplied"
        } else {
            "media default, not measured"
        }
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
    if let Some(writes) = prepared_flash {
        if let Some(target) = &mut report.target {
            target.luns = writes.luns();
        }
        report.coverage.reason = Some("explicit XML ranges only; no full-device scan".into());
        if let Command::Flash { verify, yes, .. } = &cli.command {
            execute_writes(writes, &mut channel, *verify, *yes, report);
        }
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
    match &cli.command {
        Command::Backup {
            output,
            partition,
            overwrite,
        } => backup::directory(&mut channel, &luns, partition, output, *overwrite, report),
        Command::Restore {
            from,
            image,
            partition,
            verify,
            yes,
        } => {
            let prepared = oplus_edl::write::prepare_restore(
                &mut channel,
                &luns,
                from.as_deref(),
                image.as_deref(),
                partition,
                cli.lun,
                report,
            );
            match prepared {
                Ok(writes) => execute_writes(writes, &mut channel, *verify, *yes, report),
                Err(e) => report.error(Status::InputError, e.to_string()),
            }
        }
        _ => unreachable!(),
    }
}

fn execute_writes(
    writes: oplus_edl::write::Writes,
    channel: &mut impl qdl::types::QdlChan,
    verify: oplus_edl::cli::Verify,
    yes: bool,
    report: &mut Report,
) {
    writes.confirm_and_execute(
        channel,
        verify,
        yes,
        &mut std::io::stdin().lock(),
        &mut std::io::stderr().lock(),
        report,
    );
}
