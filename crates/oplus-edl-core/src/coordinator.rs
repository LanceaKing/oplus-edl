use crate::{
    backup, device, firehose,
    host::HostServices,
    input::{Changed, Sources},
    model::{
        CloudInput, Connection, FlashInput, LoaderInput, OperationRequest, PackageArtifacts,
        PartitionSnapshot, PreparationId, PreparedSummary, Request, Scope, SessionConfig, Verify,
    },
    package::{self, Preparation},
    project::PackageGuard,
    result::{
        CloudAssets, DeviceCandidate, Error, InputValidity, Item, Report, ResultData, Status,
        Target,
    },
    service::Job,
    session::{Session, SessionChannel, Stopped, check_start, response_wait_expired},
    storage,
    write::{self, Writes},
};
use anyhow::{Context, Result, bail, ensure};
use qdl::types::{FirehoseConfiguration, FirehoseStorageType, QdlChan};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

fn record_error(
    report: &mut Report,
    category: Status,
    error: &anyhow::Error,
    errors_before: usize,
) {
    if error.is::<Stopped>() {
        return;
    }
    let recorded = report.errors.len()
        + report
            .items
            .iter()
            .map(|item| item.errors.len())
            .sum::<usize>();
    if recorded == errors_before {
        if response_wait_expired(error) {
            report.unusable = true;
            report.error_source(Status::ExecutionFailed, error);
        } else if error.is::<Changed>() {
            report.error_source(Status::InputChanged, error);
        } else {
            report.error_source(category, error);
        }
    }
}

fn input_status(error: &anyhow::Error) -> Status {
    if error.is::<Changed>() {
        Status::InputChanged
    } else if error.is::<crate::program_xml::Unsupported>() {
        Status::Unsupported
    } else {
        Status::InputError
    }
}

fn recorded_errors(report: &Report) -> usize {
    report.errors.len()
        + report
            .items
            .iter()
            .map(|item| item.errors.len())
            .sum::<usize>()
}

pub(crate) fn run_device(
    request: Request,
    host: &HostServices,
    session: &mut Option<Session>,
    job: &Job,
) -> (Report, Option<PartitionSnapshot>) {
    if let Err(error) = job.reserve_paths(&request) {
        let command = match &request {
            Request::Discover { .. } => "devices",
            Request::Connect(_) => "connect",
            Request::Inspect(..) => "partitions",
            Request::Run { operation, .. } => operation.name(),
        };
        let mut report = Report::new(Some(command), None);
        report.error_source(Status::InputError, &error);
        return (report, None);
    }
    match request {
        Request::Discover { backend } => {
            let mut report = Report::new(Some("devices"), None);
            report.input_validity = InputValidity::NotApplicable;
            if check_start(Some(job)).is_ok() {
                match host.enumerate(backend) {
                    Ok(candidates) => {
                        report.candidates.clone_from(&candidates);
                        report.data = Some(ResultData::Candidates {
                            candidates: candidates
                                .into_iter()
                                .map(|key| DeviceCandidate { key, backend })
                                .collect(),
                        });
                    }
                    Err(error) => report.error_source(Status::ExecutionFailed, &error),
                }
            }
            (report, None)
        }
        Request::Connect(config) => {
            let mut report = Report::new(Some("connect"), None);
            let result = device::configuration(config.backend, config.memory, config.sector_size)
                .and_then(|geometry| {
                    open_session(config, geometry, host, session, job, &mut report)
                });
            if let Err(error) = result {
                let category = if report.unusable {
                    Status::ExecutionFailed
                } else {
                    Status::InputError
                };
                record_error(&mut report, category, &error, 0);
            }
            (report, None)
        }
        Request::Inspect(id, scope) => {
            let mut report = Report::new(Some("partitions"), None);
            let Some(session) = session.as_mut().filter(|session| session.id == id) else {
                report.error(Status::InputError, "session is no longer active");
                return (report, None);
            };
            let snapshot = inspect(session, scope, job, &mut report);
            (report, snapshot)
        }
        Request::Run {
            connection,
            operation,
        } => run_operation(connection, operation, host, session, job),
    }
}

fn run_operation(
    connection: Connection,
    operation: OperationRequest,
    host: &HostServices,
    session: &mut Option<Session>,
    job: &Job,
) -> (Report, Option<PartitionSnapshot>) {
    let verify = match &operation {
        OperationRequest::Restore { verify, .. } | OperationRequest::Flash { verify, .. } => {
            Some(*verify)
        }
        _ => None,
    };
    let mut report = Report::new(Some(operation.name()), verify.map(Verify::as_str));
    let configured = match &connection {
        Connection::Open(config) => {
            device::configuration(config.backend, config.memory, config.sector_size)
        }
        Connection::Existing(id) => session
            .as_ref()
            .filter(|session| session.id == *id)
            .map(|session| session.channel.fh_cfg)
            .context("session is no longer active"),
        Connection::Offline => unreachable!("offline request uses the other slot"),
    };
    let geometry = match configured {
        Ok(geometry) => geometry,
        Err(error) => {
            report.error_source(Status::InputError, &error);
            return (report, None);
        }
    };
    if check_start(Some(job)).is_err() {
        return (report, None);
    }
    let prepared = match &operation {
        OperationRequest::Flash {
            input,
            verify,
            set_bootable,
            reboot,
            lun,
        } => {
            let prepared = prepare_flash(
                input,
                geometry.storage_sector_size,
                *lun,
                *set_bootable,
                *reboot,
                &mut report,
                job,
            );
            match prepared {
                Ok(plan) => {
                    if let Err(error) = plan.writes.preflight(*verify) {
                        plan.writes
                            .reject(&mut report, Status::Unsupported, &error.to_string());
                        return (report, None);
                    }
                    Some(plan)
                }
                Err(error) => {
                    let category = if matches!(
                        input,
                        FlashInput::Package {
                            artifacts: PackageArtifacts::Reopen(_),
                            ..
                        }
                    ) {
                        Status::InputError
                    } else {
                        input_status(&error)
                    };
                    record_error(&mut report, category, &error, 0);
                    return (report, None);
                }
            }
        }
        OperationRequest::Restore {
            from,
            image,
            selection,
            ..
        } => {
            if !valid_restore(
                from.as_deref(),
                image.as_deref(),
                selection.partitions.single_target().is_some(),
            ) {
                report.error(
                    Status::InputError,
                    "restore source is missing or the selection is invalid",
                );
                return (report, None);
            }
            None
        }
        _ => None,
    };
    if verify.is_some() && !job.writes_enabled() {
        if let Some(plan) = prepared {
            plan.writes.reject(
                &mut report,
                Status::InputError,
                "--yes is required for a write operation",
            );
        } else {
            report.error(
                Status::InputError,
                "--yes is required for a write operation",
            );
        }
        return (report, None);
    }
    let opening = matches!(connection, Connection::Open(_));
    if let Connection::Open(config) = connection
        && let Err(error) = open_session(config, geometry, host, session, job, &mut report)
    {
        let category = if report.unusable {
            Status::ExecutionFailed
        } else {
            Status::InputError
        };
        record_error(&mut report, category, &error, 0);
        return (report, None);
    }
    let session = session.as_mut().expect("active session checked");
    report.session.generation = Some(session.id);
    report.target = Some(session.target.clone());
    report.input_validity = InputValidity::Valid;
    if let OperationRequest::Inspect { scope } = operation {
        let snapshot = inspect(session, scope, job, &mut report);
        return (report, snapshot);
    }
    let mut channel = SessionChannel {
        inner: &mut session.channel,
        job,
    };
    match operation {
        OperationRequest::Flash {
            verify,
            set_bootable,
            ..
        } => {
            let PlannedFlash {
                mut writes,
                package,
            } = prepared.expect("flash plan");
            let before = recorded_errors(&report);
            let preflight = prepare_device_flash(
                &mut channel,
                session.digest,
                set_bootable,
                package.as_ref().map(|(guard, _)| guard),
                job,
                &mut report,
            );
            match preflight {
                Ok(mode) => {
                    writes.set_wire_mode(mode);
                    if let Some(target) = &mut report.target {
                        target.luns = writes.luns();
                    }
                    let preparation = package.map(|(_, preparation)| preparation);
                    report.coverage.reason = Some(if preparation.is_some() {
                        "package project and selected ranges checked".into()
                    } else {
                        "explicit XML ranges only; package project matching unavailable".into()
                    });
                    execute(
                        writes,
                        &mut channel,
                        verify,
                        job,
                        &mut report,
                        session.id,
                        preparation,
                    );
                }
                Err(error) => record_error(&mut report, Status::InputError, &error, before),
            }
        }
        OperationRequest::Backup {
            output,
            selection,
            overwrite,
        } => {
            job.progress("read", None);
            let before = recorded_errors(&report);
            match resolve_luns(
                &mut channel,
                selection.explicit_luns(),
                !opening,
                job,
                &mut report,
            ) {
                Ok(luns) => backup::directory_controlled(
                    &mut channel,
                    &luns,
                    &selection,
                    &output,
                    overwrite,
                    Some(job),
                    &mut report,
                ),
                Err(error) => record_error(&mut report, Status::Unsupported, &error, before),
            }
        }
        OperationRequest::Restore {
            from,
            image,
            selection,
            verify,
        } => {
            job.progress("preparation", None);
            let before = recorded_errors(&report);
            let prepared = resolve_luns(
                &mut channel,
                selection.explicit_luns(),
                !opening,
                job,
                &mut report,
            )
            .and_then(|luns| {
                write::prepare_restore_controlled(
                    &mut channel,
                    &luns,
                    Some(&luns),
                    (from.as_deref(), image.as_deref()),
                    &selection.partitions,
                    Some(job),
                    &mut report,
                )
            });
            match prepared {
                Ok(writes) => execute(
                    writes,
                    &mut channel,
                    verify,
                    job,
                    &mut report,
                    session.id,
                    None,
                ),
                Err(error) => record_error(&mut report, Status::InputError, &error, before),
            }
        }
        _ => unreachable!("non-device operation"),
    }
    (report, None)
}

fn valid_restore(from: Option<&Path>, image: Option<&Path>, single_target: bool) -> bool {
    match (from, image) {
        (Some(path), None) => path.is_dir(),
        (None, Some(path)) => single_target && path.is_file(),
        _ => false,
    }
}

struct MaterialPaths {
    loader: PathBuf,
    digest: Option<PathBuf>,
    signature: Option<PathBuf>,
}

fn material(
    input: &LoaderInput,
    host: &HostServices,
    job: &Job,
    report: &mut Report,
) -> Result<MaterialPaths> {
    check_start(Some(job))?;
    match input {
        LoaderInput::Local {
            loader,
            signed_digest,
            digest_signature,
        } => Ok(MaterialPaths {
            loader: loader.clone().context("--loader is required")?,
            digest: signed_digest.clone(),
            signature: digest_signature.clone(),
        }),
        LoaderInput::Cloud(input) => {
            let assets = acquire(input, host, job, report)?;
            Ok(MaterialPaths {
                loader: assets.loader,
                digest: Some(assets.digest),
                signature: Some(assets.signature),
            })
        }
    }
}

#[cfg(feature = "cloud")]
fn acquire(
    input: &CloudInput,
    host: &HostServices,
    job: &Job,
    report: &mut Report,
) -> Result<CloudAssets> {
    job.progress("acquisition", None);
    job.log("acquisition", "Acquiring cloud loader assets");
    let input = input.clone();
    let host = host.clone();
    let control = job.clone();
    let (result, outputs) = job.acquire_on_worker(move || {
        let mut acquired = Report::new(None, None);
        let result =
            crate::cloud::download_controlled(&input, &host, Some(&control), &mut acquired);
        (result, acquired.outputs)
    })?;
    report.outputs.extend(outputs);
    result.context("cloud acquisition")
}

#[cfg(not(feature = "cloud"))]
fn acquire(_: &CloudInput, _: &HostServices, _: &Job, _: &mut Report) -> Result<CloudAssets> {
    bail!("cloud acquisition unavailable")
}

fn open_session(
    config: SessionConfig,
    geometry: FirehoseConfiguration,
    host: &HostServices,
    session: &mut Option<Session>,
    job: &Job,
    report: &mut Report,
) -> Result<()> {
    let paths = material(&config.loader, host, job, report)?;
    let mut sources = Sources::default();
    let mut bind_digest = |path: Option<&Path>| {
        path.map(|path| sources.open(path).and_then(device::read_signed_digest))
            .transpose()
    };
    let digest = bind_digest(paths.digest.as_deref()).context("signed digest")?;
    let signature = bind_digest(paths.signature.as_deref()).context("DIGEST signature")?;
    let loader = sources.open(&paths.loader)?;
    crate::session::check_dispatch(Some(job), "loader source bound")?;
    let loaded = crate::programmer::load(loader);
    sources.check()?;
    let mut images =
        loaded.with_context(|| format!("read programmer {}", paths.loader.display()))?;
    ensure!(
        images.iter().flatten().any(|bytes| !bytes.is_empty()),
        "empty loader"
    );
    report.input_validity = InputValidity::Valid;
    let key = select_device(&config, host, job, report)?;
    sources.check()?;
    let target = Target {
        device: Some(key.clone()),
        backend: Some(config.backend.as_str().into()),
        memory: Some(geometry.storage_type.to_string()),
        luns: vec![],
    };
    job.release_previous_session(session)?;
    report.target = Some(target.clone());
    job.progress("initialization", None);
    sources.check()?;
    crate::session::check_dispatch(Some(job), "device open")?;
    let channel = match host
        .open(config.backend, &key, geometry)
        .context("open device")
    {
        Ok(channel) => channel,
        Err(error) => {
            report.error_source(Status::ExecutionFailed, &error);
            return Err(error);
        }
    };
    let id = job.reserve_session_id();
    report.session.generation = Some(id);
    *session = Some(Session {
        id,
        channel,
        target,
        digest: signature.is_some(),
    });
    let mut channel = SessionChannel {
        inner: &mut session.as_mut().unwrap().channel,
        job,
    };
    let result = sources.check().and_then(|()| {
        device::initialize_controlled(
            &mut channel,
            &mut images,
            digest.as_deref(),
            signature.as_deref(),
            Some(job),
            &mut report.session.completed_initialization_steps,
            |message| job.log("initialization", message),
        )
    });
    if let Err(error) = &result {
        report.unusable = true;
        if error.is::<Stopped>() {
            report.session.reason = Some("initialization_stopped");
        } else if error.is::<Changed>() {
            report.session.reason = Some("initialization_source_changed");
        }
    }
    result
}

fn select_device(
    config: &SessionConfig,
    host: &HostServices,
    job: &Job,
    report: &mut Report,
) -> Result<String> {
    if config.backend == crate::model::Backend::Serial {
        ensure!(
            config.device.is_some(),
            "serial requires an explicit --device"
        );
    }
    ensure!(
        config.device.as_deref() != Some(""),
        "empty device selector"
    );
    let started = job.now();
    job.progress("discovery", None);
    let deadline = config.wait_timeout.filter(|timeout| !timeout.is_zero());
    let elapsed = || job.now().saturating_duration_since(started);
    if config.wait_timeout.is_some() {
        job.log("discovery", "Waiting for a matching device");
    }
    loop {
        check_start(Some(job))?;
        ensure!(
            deadline.is_none_or(|timeout| elapsed() < timeout),
            "timed out waiting for a matching device"
        );
        let keys = host
            .enumerate(config.backend)
            .context("enumerate devices")?;
        report.candidates.clone_from(&keys);
        let elapsed = elapsed();
        ensure!(
            deadline.is_none_or(|timeout| elapsed < timeout),
            "timed out waiting for a matching device"
        );
        let mut matching = keys
            .iter()
            .filter(|key| config.device.as_ref().is_none_or(|wanted| *key == wanted));
        if let Some(key) = matching.next() {
            if matching.next().is_some() {
                let message = "multiple matching devices; select a unique --device";
                let mut error = Error::new(Status::InputError, message);
                error.code = Some("device_ambiguous");
                report.errors.push(error);
                bail!("{message}");
            }
            return Ok(key.clone());
        }
        let Some(timeout) = config.wait_timeout else {
            bail!("no matching device");
        };
        let interval = Duration::from_millis(250);
        std::thread::sleep(if timeout.is_zero() {
            interval
        } else {
            interval.min(timeout - elapsed)
        });
    }
}

fn resolve_luns(
    channel: &mut impl QdlChan,
    mut luns: Vec<u8>,
    allow_gate: bool,
    job: &Job,
    report: &mut Report,
) -> Result<Vec<u8>> {
    if luns.is_empty() {
        if allow_gate {
            job.lun_query_gate();
        }
        let info = storage::query_info(channel, 0, job, report)?;
        luns = storage::discovered_luns(&info, channel.fh_config().storage_sector_size)?;
    }
    if let Some(target) = &mut report.target {
        target.luns.clone_from(&luns);
    }
    Ok(luns)
}

fn inspect(
    session: &mut Session,
    scope: Scope,
    job: &Job,
    report: &mut Report,
) -> Option<PartitionSnapshot> {
    job.progress("inspection", None);
    report.session.generation = Some(session.id);
    report.target = Some(session.target.clone());
    report.input_validity = InputValidity::Valid;
    let mut channel = SessionChannel {
        inner: &mut session.channel,
        job,
    };
    let before = recorded_errors(report);
    let luns = match resolve_luns(
        &mut channel,
        match &scope {
            Scope::Selected(luns) => luns.clone(),
            Scope::All => vec![],
        },
        true,
        job,
        report,
    ) {
        Ok(luns) => luns,
        Err(error) => {
            record_error(report, Status::Unsupported, &error, before);
            return None;
        }
    };
    let mut partitions = Vec::new();
    report.coverage.reason = Some(format!("GPT scan started for LUNs {luns:?}"));
    for (index, &lun) in luns.iter().enumerate() {
        let Ok(table) = storage::read_gpt_recorded(
            &mut channel,
            lun,
            firehose::WireMode::Direct,
            Some(job),
            report,
        ) else {
            report.coverage.reason = Some(format!(
                "GPT scan incomplete; checked LUNs {:?} of {luns:?}",
                &luns[..index]
            ));
            return None;
        };
        partitions.extend(storage::snapshot_partitions(&table, lun, session.id, 0));
    }
    report.coverage.limited = matches!(scope, Scope::Selected(_));
    let selection = if report.coverage.limited {
        "explicitly selected"
    } else {
        "discovered"
    };
    report.coverage.reason = Some(format!("checked GPTs on {selection} LUNs {luns:?}"));
    Some(PartitionSnapshot {
        generation: session.id,
        revision: 0,
        scope,
        luns,
        partitions,
    })
}

struct PlannedFlash {
    writes: Writes,
    package: Option<(PackageGuard, Preparation)>,
}

fn prepare_flash(
    input: &FlashInput,
    sector: usize,
    lun: Option<u8>,
    set_bootable: Option<u8>,
    reboot: bool,
    report: &mut Report,
    job: &Job,
) -> Result<PlannedFlash> {
    job.progress("preparation", None);
    job.log("preparation", "Preparing flash inputs");
    let (candidates, sources, package) = match input {
        FlashInput::Xml {
            programs,
            patches,
            raw_size,
        } => {
            let mut sources = Sources::default();
            let candidates = crate::program_xml::parse_bound(
                programs,
                patches,
                sector as u64,
                *raw_size,
                &mut sources,
                &|unit| crate::session::check_dispatch(Some(job), unit),
            )?;
            (candidates, sources, None)
        }
        FlashInput::Package { input, artifacts } => {
            let prepared = package::prepare_input(
                input,
                artifacts,
                sector as u64,
                &|unit| crate::session::check_dispatch(Some(job), unit),
                report,
            );
            let prepared = if matches!(artifacts, PackageArtifacts::Reopen(_)) {
                prepared.context("reopen prepared package (prepare in a fresh directory if evidence or bindings differ)")?
            } else {
                prepared?
            };
            let guard = PackageGuard::new(prepared.project, &prepared.candidates)?;
            (
                prepared.candidates,
                prepared.sources,
                Some((guard, prepared.summary)),
            )
        }
    };
    ensure!(
        lun.is_none_or(|lun| candidates.iter().all(|entry| entry.lun == lun)
            && set_bootable.is_none_or(|boot| boot == lun)),
        "XML/final action LUN conflicts with --lun"
    );
    let writes = Writes::flash_bound(candidates, sector as u64, set_bootable, reboot, sources)?;
    writes.check_sources()?;
    check_start(Some(job))?;
    report.input_validity = InputValidity::Valid;
    Ok(PlannedFlash { writes, package })
}

fn prepare_device_flash(
    channel: &mut impl QdlChan,
    digest: bool,
    set_bootable: Option<u8>,
    guard: Option<&PackageGuard>,
    job: &Job,
    report: &mut Report,
) -> Result<firehose::WireMode> {
    job.progress("inspection", None);
    let mut mode = firehose::WireMode::Direct;
    if digest {
        if let Some(lun) = set_bootable
            && matches!(channel.fh_config().storage_type, FirehoseStorageType::Ufs)
        {
            let info = storage::query_info(channel, lun, job, report)?;
            device::check_boot_lun_info(&info, lun).context("boot LUN preflight")?;
        }
        check_start(Some(job))?;
        let sector = channel.fh_config().storage_sector_size as u64;
        let mut data = Vec::new();
        let result = firehose::probe_preview_into(channel, &mut data);
        let mut item = Item::read(0, None, 31 * sector, 5 * sector);
        item.transferred_bytes = Some(data.len().to_string());
        storage::record_read(report, item, &result);
        mode = result?;
    }
    if let Some(guard) = guard {
        guard.validate_recorded(channel, mode, Some(job), report)?;
    }
    Ok(mode)
}

fn execute(
    writes: Writes,
    channel: &mut impl QdlChan,
    verify: Verify,
    job: &Job,
    report: &mut Report,
    session: crate::model::SessionId,
    package: Option<Preparation>,
) {
    if let Err(error) = writes.preflight(verify) {
        writes.reject(report, Status::Unsupported, &error.to_string());
        return;
    }
    if let Err(error) = writes.check_sources() {
        report.items.extend(writes.prepared_items());
        report.error_source(Status::InputChanged, &error);
        return;
    }
    let summary = PreparedSummary {
        id: PreparationId(job.id),
        operation_id: job.id,
        session_generation: session,
        verify,
        items: writes.prepared_items(),
        package,
        snapshot_revision: job.snapshot_revision(),
        target: report.target.clone(),
        coverage: report.coverage.clone(),
    };
    if job.consent(summary) {
        writes.execute_controlled(channel, verify, Some(job), report);
    } else if job.stop().is_some() {
        report.items.extend(writes.prepared_items());
    } else {
        writes.reject(report, Status::InputError, "write confirmation declined");
    }
}

pub(crate) fn run_offline(request: Request, host: &HostServices, job: &Job) -> Report {
    let paths = job.reserve_paths(&request);
    let Request::Run { operation, .. } = request else {
        unreachable!("offline work is a Run");
    };
    let mut report = Report::new(Some(operation.name()), None);
    if let Err(error) = paths {
        report.error_source(Status::InputError, &error);
        return report;
    }
    if check_start(Some(job)).is_err() {
        return report;
    }
    let result = match operation {
        OperationRequest::PreparePackage { input, output } => {
            let input = FlashInput::Package {
                input,
                artifacts: PackageArtifacts::Generate(output),
            };
            prepare_flash(
                &input,
                package::SECTOR_SIZE as usize,
                None,
                None,
                false,
                &mut report,
                job,
            )
            .and_then(|plan| {
                plan.writes.preflight(Verify::BestEffort)?;
                let (_, preparation) = plan.package.expect("package preparation");
                report.data = Some(ResultData::Preparation {
                    preparation: Box::new(preparation),
                });
                Ok(())
            })
        }
        OperationRequest::CloudFetch(input) => {
            acquire(&input, host, job, &mut report).map(|assets| {
                report.input_validity = InputValidity::Valid;
                report.data = Some(ResultData::Assets { assets });
            })
        }
        OperationRequest::CloudIdentity {
            input,
            confirmed_path,
        } => cloud_identity(input, confirmed_path.as_deref(), host, job, &mut report),
        _ => unreachable!("validated offline operation"),
    };
    if let Err(error) = result {
        record_error(&mut report, input_status(&error), &error, 0);
    }
    report
}

#[cfg(feature = "cloud")]
fn cloud_identity(
    input: CloudInput,
    confirmed_path: Option<&Path>,
    host: &HostServices,
    job: &Job,
    report: &mut Report,
) -> Result<()> {
    let path = crate::cloud::installation_identity(
        &input,
        confirmed_path,
        host.cloud_loopback(),
        job,
        report,
    )?;
    let created = confirmed_path.is_some();
    report.input_validity = InputValidity::Valid;
    report.data = Some(ResultData::Identity {
        identity_path: path,
        created,
    });
    Ok(())
}

#[cfg(not(feature = "cloud"))]
fn cloud_identity(
    _: CloudInput,
    _: Option<&Path>,
    _: &HostServices,
    _: &Job,
    _: &mut Report,
) -> Result<()> {
    bail!("cloud acquisition unavailable")
}
