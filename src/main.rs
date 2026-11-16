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
    let cloud = if let Some(path) = &cli.cloud_config {
        if !matches!(cli.command, Command::Flash { .. }) {
            report.error(
                Status::InputError,
                "cloud acquisition is supported by flash only",
            );
            return;
        }
        match oplus_edl::cloud::download(
            path,
            cli.cloud_output
                .as_deref()
                .expect("clap requires cloud output"),
        ) {
            Ok(material) => Some(material),
            Err(e) => {
                report.error(Status::InputError, format!("cloud acquisition: {e:#}"));
                return;
            }
        }
    } else {
        None
    };
    if matches!(
        cli.command,
        Command::Flash {
            cloud_fetch_only: true,
            ..
        }
    ) {
        let material = cloud.as_ref().expect("clap requires cloud config");
        report.candidates = [&material.loader, &material.digest, &material.signature]
            .map(|p| p.display().to_string())
            .to_vec();
        report.coverage.reason = Some("Cloud assets validated; no device was opened".into());
        return;
    }
    let mut package_guard = None;
    let prepared_flash = if let Command::Flash {
        package,
        nv_id,
        prepare_output,
        flash_config,
        prepare_only,
        raw_size,
        program,
        patch,
        set_bootable,
        reboot,
        verify,
        ..
    } = &cli.command
    {
        let prepared = if let Some(root) = package {
            oplus_edl::package::prepare(
                root,
                nv_id.as_deref().expect("clap requires NV ID"),
                prepare_output.as_deref().expect("clap requires output"),
                flash_config.as_deref(),
                config.storage_sector_size as u64,
            )
            .and_then(|p| {
                package_guard = Some(oplus_edl::project::PackageGuard::new(
                    p.project,
                    &p.candidates,
                )?);
                report.candidates.push(p.manifest.display().to_string());
                Ok(p.candidates)
            })
        } else {
            oplus_edl::program_xml::parse_with_raw_size(
                program,
                patch,
                config.storage_sector_size as u64,
                false,
                *raw_size,
            )
        };
        let candidates = match prepared {
            Ok(v) => v,
            Err(e) => {
                report.error(
                    if e.is::<oplus_edl::program_xml::Unsupported>() {
                        Status::Unsupported
                    } else {
                        Status::InputError
                    },
                    format!("{e:#}"),
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
        if *prepare_only {
            report.coverage.reason = Some(
                "Package prepared offline; device compatibility and flash success are unverified"
                    .into(),
            );
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
    if prepared_flash.is_none() && cli.device.is_none() {
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
    let Some(loader) = cli
        .loader
        .as_ref()
        .or_else(|| cloud.as_ref().map(|m| &m.loader))
    else {
        report.error(Status::InputError, "--loader is required");
        return;
    };
    let signed_digest = match cli
        .signed_digest
        .as_deref()
        .or_else(|| cloud.as_ref().map(|m| m.digest.as_path()))
        .map(device::load_signed_digest)
        .transpose()
    {
        Ok(bytes) => bytes,
        Err(e) => {
            report.error(Status::InputError, format!("signed digest: {e:#}"));
            return;
        }
    };
    let digest_signature = match cli
        .digest_signature
        .as_deref()
        .or_else(|| cloud.as_ref().map(|m| m.signature.as_path()))
        .map(device::load_signed_digest)
        .transpose()
    {
        Ok(bytes) => bytes,
        Err(e) => {
            report.error(Status::InputError, format!("DIGEST signature: {e:#}"));
            return;
        }
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
    let selected = if let Command::Flash { wait_timeout, .. } = &cli.command {
        eprintln!(
            "Waiting for EDL device{}; Ctrl+C to cancel",
            cli.device
                .as_deref()
                .map(|s| format!(" {s}"))
                .unwrap_or_default()
        );
        let timeout = (*wait_timeout != 0).then(|| std::time::Duration::from_secs(*wait_timeout));
        match device::wait_for_edl(cli.backend, cli.device.as_deref(), timeout, |keys| {
            report.candidates = keys.to_vec();
        }) {
            Ok(key) => key,
            Err(e) => {
                report.error(Status::InputError, format!("EDL device selection: {e:#}"));
                return;
            }
        }
    } else {
        let selected = cli
            .device
            .as_deref()
            .expect("explicit device checked above");
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
        selected.to_string()
    };
    let selected = selected.as_str();
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
    if let Err(e) = device::initialize_with_digest_signature(
        &mut channel,
        &mut images,
        signed_digest.as_deref(),
        digest_signature.as_deref(),
        |message| eprintln!("{message}"),
    ) {
        report.error(Status::ExecutionFailed, format!("initialize: {e:#}"));
        return;
    }
    if let Some(mut writes) = prepared_flash {
        let mut wire_mode = oplus_edl::firehose::WireMode::Direct;
        if digest_signature.is_some() {
            if let Command::Flash {
                set_bootable: Some(lun),
                ..
            } = &cli.command
                && let Err(e) = device::validate_preview_boot_lun(&mut channel, *lun)
            {
                report.error(Status::InputError, format!("boot LUN preflight: {e:#}"));
                return;
            }
            match oplus_edl::firehose::probe_preview_mode(&mut channel) {
                Ok(mode) => {
                    wire_mode = mode;
                    eprintln!("Preview read-mode probe acknowledged: {mode:?}");
                    writes.set_wire_mode(mode);
                }
                Err(e) => {
                    report.error(
                        Status::ExecutionFailed,
                        format!("preview read-mode probe: {e:#}"),
                    );
                    return;
                }
            }
        }
        if let Some(guard) = package_guard {
            if let Err(e) = guard.validate(&mut channel, wire_mode) {
                report.error(
                    Status::InputError,
                    format!("package/device preflight: {e:#}"),
                );
                return;
            }
            eprintln!("Package project matches device OCDT; planned ranges checked");
        }
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
