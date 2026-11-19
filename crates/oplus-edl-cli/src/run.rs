use crate::{
    args::{
        Cli, CloudCommand, Command, ConnectionArgs, FlashArgs, FlashCommand, PackageArgs,
        PackageCommand, RestoreCommand, SelectionArgs,
    },
    render::{OutputMode, OutputWriter},
};
use oplus_edl_core::{
    CoreService, OperationResult, StopReason, SubmitError,
    events::{Event, JobStatus},
    model::{
        CloudInput, Connection, Decision, FlashInput, IdentityMode, LoaderInput, OperationRequest,
        PackageArtifacts, PackageInput, PartitionSelection, PartitionTargets, Request, Scope,
        SessionConfig,
    },
    result::{Report, SessionDisposition, Status},
};
use std::{
    io::{self, Write},
    path::PathBuf,
    sync::{Arc, Mutex, mpsc},
    thread,
    time::{Duration, Instant},
};

const DELIVERY_TIMEOUT: Duration = Duration::from_secs(5);
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

fn cloud_input(config: PathBuf, output: PathBuf, create_identity: bool) -> io::Result<CloudInput> {
    let base_directory = std::env::current_dir()?;
    Ok(CloudInput {
        config: base_directory.join(config),
        output: base_directory.join(output),
        base_directory: Some(base_directory),
        identity_mode: if create_identity {
            IdentityMode::CreateNew
        } else {
            IdentityMode::OpenExisting
        },
    })
}

fn connection(args: ConnectionArgs, cloud: Option<CloudInput>) -> Connection {
    Connection::Open(SessionConfig {
        backend: args.backend,
        device: args.device,
        memory: args.memory,
        sector_size: args.sector_size,
        loader: match cloud {
            Some(input) => LoaderInput::Cloud(input),
            None => LoaderInput::Local {
                loader: args.loader,
                signed_digest: args.signed_digest,
                digest_signature: args.digest_signature,
            },
        },
        wait_timeout: args.wait_timeout.map(Duration::from_secs),
    })
}

fn partition_selection(luns: Vec<u8>, args: SelectionArgs) -> PartitionSelection {
    PartitionSelection {
        luns,
        partitions: if args.all {
            PartitionTargets::All
        } else {
            PartitionTargets::Selected(args.partition)
        },
    }
}

impl From<PackageArgs> for PackageInput {
    fn from(args: PackageArgs) -> Self {
        Self {
            root: args.package,
            nv_id: args.nv_id,
            config: args.flash_config,
        }
    }
}

fn request(command: Command) -> io::Result<(Request, bool)> {
    let (connection, operation, yes) = match command {
        Command::Devices { backend } => return Ok((Request::Discover { backend }, true)),
        Command::Partitions {
            connection: args,
            lun,
        } => (
            connection(args, None),
            OperationRequest::Inspect {
                scope: if lun.is_empty() {
                    Scope::All
                } else {
                    Scope::Selected(lun)
                },
            },
            true,
        ),
        Command::Backup {
            connection: args,
            output,
            selection,
            lun,
            overwrite,
        } => (
            connection(args, None),
            OperationRequest::Backup {
                output,
                selection: partition_selection(lun, selection),
                overwrite,
            },
            true,
        ),
        Command::Restore(command) => match command {
            RestoreCommand::Image {
                connection: args,
                image,
                partition,
                write,
            } => (
                connection(args, None),
                OperationRequest::Restore {
                    from: None,
                    image: Some(image),
                    selection: PartitionSelection {
                        luns: vec![],
                        partitions: PartitionTargets::Selected(vec![partition]),
                    },
                    verify: write.verify,
                },
                write.yes,
            ),
            RestoreCommand::Directory {
                connection: args,
                from,
                selection,
                lun,
                write,
            } => (
                connection(args, None),
                OperationRequest::Restore {
                    from: Some(from),
                    image: None,
                    selection: partition_selection(lun, selection),
                    verify: write.verify,
                },
                write.yes,
            ),
        },
        Command::Flash(command) => {
            let (common, input) = match command {
                FlashCommand::Xml {
                    common,
                    program,
                    patch,
                    raw_size,
                } => (
                    common,
                    FlashInput::Xml {
                        programs: program,
                        patches: patch,
                        raw_size,
                    },
                ),
                FlashCommand::Package {
                    common,
                    package,
                    prepare_output,
                    prepared,
                    ..
                } => {
                    let artifacts = match (prepare_output, prepared) {
                        (Some(path), None) => PackageArtifacts::Generate(path),
                        (None, Some(path)) => PackageArtifacts::Reopen(path),
                        _ => unreachable!("validated preparation source"),
                    };
                    (
                        common,
                        FlashInput::Package {
                            input: package.into(),
                            artifacts,
                        },
                    )
                }
            };
            let FlashArgs {
                connection: args,
                write,
                set_bootable,
                reboot,
            } = common;
            let cloud = match (args.cloud_config, args.cloud_output) {
                (Some(config), Some(output)) => {
                    Some(cloud_input(config, output, args.create_identity)?)
                }
                _ => None,
            };
            (
                connection(args.local, cloud),
                OperationRequest::Flash {
                    input,
                    verify: write.verify,
                    lun: None,
                    set_bootable,
                    reboot,
                },
                write.yes,
            )
        }
        Command::Package(PackageCommand::Prepare { package, output }) => (
            Connection::Offline,
            OperationRequest::PreparePackage {
                input: package.into(),
                output,
            },
            true,
        ),
        Command::Cloud(CloudCommand::Fetch {
            config,
            output,
            create_identity,
        }) => (
            Connection::Offline,
            OperationRequest::CloudFetch(cloud_input(config, output, create_identity)?),
            true,
        ),
    };
    Ok((
        Request::Run {
            connection,
            operation,
        },
        yes,
    ))
}

pub fn run_with_service<W: Write + Send + 'static>(
    cli: Cli,
    service: CoreService,
    writer: W,
    register_signal: impl FnOnce(Arc<dyn Fn() + Send + Sync>),
) -> i32 {
    run_and_observe(cli, service, writer, register_signal, |_| {})
}

fn run_and_observe<W: Write + Send + 'static>(
    cli: Cli,
    service: CoreService,
    writer: W,
    register_signal: impl FnOnce(Arc<dyn Fn() + Send + Sync>),
    observe: impl FnOnce(&OperationResult),
) -> i32 {
    let mode = OutputMode {
        json: cli.json,
        quiet: cli.quiet,
    };
    let operation = cli.command.name();
    let verify = cli.command.verify();
    let (request, yes) = match request(cli.command) {
        Ok(request) => request,
        Err(error) => {
            return reject(
                writer,
                mode,
                operation,
                verify,
                Status::InputError,
                &error.to_string(),
            );
        }
    };
    let handle = match service.submit_with_writes_enabled(request, yes) {
        Ok(handle) => Arc::new(handle),
        Err(error) => {
            let status = match error {
                SubmitError::Unsupported => Status::Unsupported,
                SubmitError::WorkerStart => Status::ExecutionFailed,
                _ => Status::InputError,
            };
            return reject(writer, mode, operation, verify, status, &error.to_string());
        }
    };
    let relay = handle.clone();
    register_signal(Arc::new(move || {
        relay.request_stop_with_reason(StopReason::Signal)
    }));
    let mut renderer = Renderer::start(writer, mode, Some((operation, handle.operation_id())));
    let mut failed = renderer.is_none();
    if failed {
        handle.request_stop_with_reason(StopReason::ConsumerLost);
    }
    let mut last_sample = Instant::now();
    let mut last_progress = None;
    let mut result = loop {
        if let Some(renderer) = renderer.as_mut()
            && !failed
        {
            if let Some(reason) = renderer.status() {
                handle.request_stop_with_reason(reason);
                failed = true;
            } else if !mode.quiet && !renderer.pending {
                let mut frame = None;
                if last_sample.elapsed() >= PROGRESS_INTERVAL {
                    last_sample = Instant::now();
                    if let Some(status) = handle.status()
                        && last_progress.replace(status.progress) != Some(status.progress)
                        && (status.progress.completed_bytes.is_some()
                            || status.progress.total_bytes.is_some())
                    {
                        frame = Some(Frame::Progress(status));
                    }
                }
                if let Some(frame) = frame.or_else(|| handle.poll_event().map(Frame::Event))
                    && renderer.send(frame).is_err()
                {
                    handle.request_stop_with_reason(StopReason::ConsumerLost);
                    failed = true;
                }
            }
        }
        if let Some(summary) = handle.prepared_summary() {
            let _ = handle.decide(
                summary.id,
                if yes {
                    Decision::Approve
                } else {
                    Decision::Decline
                },
            );
        }
        if (failed || mode.quiet || renderer.as_ref().is_none_or(|renderer| !renderer.pending))
            && let Some(result) = handle.try_take_result()
        {
            break result;
        }
        thread::sleep(Duration::from_millis(5));
    };
    if service.close_idle() && result.session.disposition == SessionDisposition::Reusable {
        result.session.disposition = SessionDisposition::Closed;
        result.session.reason = Some("cli_finished");
    }
    observe(&result);
    if failed {
        74
    } else {
        renderer
            .unwrap()
            .finish(Frame::Terminal(Box::new(result)))
            .unwrap_or(74)
    }
}

fn reject<W: Write + Send + 'static>(
    writer: W,
    mode: OutputMode,
    operation: &'static str,
    verify: Option<&str>,
    status: Status,
    message: &str,
) -> i32 {
    let mut report = Report::new(Some(operation), verify);
    report.error(status, message);
    deliver_result(writer, mode, report)
}

pub fn deliver_result(
    writer: impl Write + Send + 'static,
    mode: OutputMode,
    report: OperationResult,
) -> i32 {
    deliver(writer, mode, Frame::Terminal(Box::new(report)))
}

pub fn deliver_information(
    writer: impl Write + Send + 'static,
    mode: OutputMode,
    topic: &'static str,
    text: String,
) -> i32 {
    deliver(writer, mode, Frame::Information(topic, text))
}

fn deliver(writer: impl Write + Send + 'static, mode: OutputMode, frame: Frame) -> i32 {
    Renderer::start(writer, mode, None)
        .and_then(|renderer| renderer.finish(frame))
        .unwrap_or(74)
}

enum Frame {
    Event(Event),
    Progress(JobStatus),
    Terminal(Box<OperationResult>),
    Information(&'static str, String),
}

struct ActivityWriter<W> {
    writer: W,
    activity: Arc<Mutex<Instant>>,
}

impl<W: Write> Write for ActivityWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let written = self.writer.write(buf)?;
        if written != 0 {
            *self.activity.lock().unwrap() = Instant::now();
        }
        Ok(written)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }
}

struct Renderer {
    sender: mpsc::Sender<Frame>,
    receipt: mpsc::Receiver<io::Result<Option<i32>>>,
    activity: Arc<Mutex<Instant>>,
    pending: bool,
}

impl Renderer {
    fn start<W: Write + Send + 'static>(
        writer: W,
        mode: OutputMode,
        identity: Option<(&'static str, u64)>,
    ) -> Option<Self> {
        let activity = Arc::new(Mutex::new(Instant::now()));
        let (sender, frames) = mpsc::channel();
        let (completed, receipt) = mpsc::channel();
        let writer = ActivityWriter {
            writer,
            activity: activity.clone(),
        };
        thread::Builder::new()
            .name("oplus-edl-output".into())
            .spawn(move || {
                let mut output = OutputWriter::new(mode, writer);
                if let Some((operation, id)) = identity {
                    output.identify(operation, id);
                }
                for frame in frames {
                    let result = match frame {
                        Frame::Event(event) => output.write_event(&event),
                        Frame::Progress(status) => output.write_progress(status),
                        Frame::Terminal(result) => {
                            let _ = completed.send(output.finish(&result).map(Some));
                            break;
                        }
                        Frame::Information(topic, text) => {
                            let _ = completed.send(output.informational(topic, &text).map(Some));
                            break;
                        }
                    };
                    let failed = result.is_err();
                    if completed.send(result.map(|_| None)).is_err() || failed {
                        break;
                    }
                }
            })
            .ok()?;
        Some(Self {
            sender,
            receipt,
            activity,
            pending: false,
        })
    }

    fn send(&mut self, frame: Frame) -> Result<(), mpsc::SendError<Frame>> {
        *self.activity.lock().unwrap() = Instant::now();
        self.pending = true;
        self.sender.send(frame)
    }

    fn status(&mut self) -> Option<StopReason> {
        if !self.pending {
            return None;
        }
        match self.receipt.try_recv() {
            Ok(Ok(_)) => {
                self.pending = false;
                None
            }
            Ok(Err(error)) if error.kind() == io::ErrorKind::BrokenPipe => {
                Some(StopReason::ConsumerLost)
            }
            Ok(Err(_)) => Some(StopReason::OutputFailed),
            Err(mpsc::TryRecvError::Disconnected) => Some(StopReason::ConsumerLost),
            Err(mpsc::TryRecvError::Empty) if self.remaining().is_zero() => {
                Some(StopReason::OutputFailed)
            }
            Err(mpsc::TryRecvError::Empty) => None,
        }
    }

    fn remaining(&self) -> Duration {
        DELIVERY_TIMEOUT.saturating_sub(self.activity.lock().unwrap().elapsed())
    }

    fn wait(&self) -> Option<Option<i32>> {
        loop {
            match self.receipt.recv_timeout(self.remaining()) {
                Ok(result) => return result.ok(),
                Err(mpsc::RecvTimeoutError::Timeout) if !self.remaining().is_zero() => continue,
                Err(_) => return None,
            }
        }
    }

    fn finish(mut self, frame: Frame) -> Option<i32> {
        if self.pending {
            self.wait()?;
        }
        self.send(frame).ok()?;
        self.wait()?
    }
}

#[cfg(test)]
#[path = "../../oplus-edl-core/tests/support/mod.rs"]
mod scripted_device;

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use oplus_edl_core::result::WriteState;
    use oplus_edl_core::{HostServices, model::Backend, test_support::service_with_stop_clock};

    struct Stalled(Option<mpsc::Receiver<()>>);

    impl Write for Stalled {
        fn write(&mut self, data: &[u8]) -> io::Result<usize> {
            if let Some(release) = self.0.take() {
                let _ = release.recv_timeout(Duration::from_secs(10));
            }
            Ok(data.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    struct BrokenPipe;

    impl Write for BrokenPipe {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn informational_delivery_uses_the_bounded_renderer() {
        let (release, blocked) = mpsc::channel();
        let start = Instant::now();
        let code = deliver_information(
            Stalled(Some(blocked)),
            OutputMode::default(),
            "help",
            "test help".into(),
        );
        let _ = release.send(());
        assert_eq!(code, 74);
        assert!(start.elapsed() >= DELIVERY_TIMEOUT && start.elapsed() < Duration::from_secs(10));
    }

    fn check_active_output_loss(writer: impl Write + Send + 'static, expected: StopReason) {
        let root = tempfile::tempdir().unwrap();
        let loader = root.path().join("fixture.bin");
        std::fs::write(&loader, b"abcd").unwrap();
        let (enumerating, entered) = mpsc::channel();
        let (release, pending) = mpsc::channel();
        let (stop, accepted) = mpsc::channel();
        let limit = Duration::from_secs(10);
        let thread = thread::spawn(move || {
            let owner = thread::current().id();
            let clock = Arc::new(move || {
                if thread::current().id() == owner {
                    let _ = stop.send(());
                }
                Instant::now()
            });
            let pending = Mutex::new(pending);
            let host = HostServices::default().with_backend(
                Backend::Usb,
                move || {
                    enumerating.send(()).unwrap();
                    pending.lock().unwrap().recv_timeout(limit).unwrap();
                    Ok(vec![])
                },
                |_, _| panic!("the fixture must not open a channel"),
            );
            let service = service_with_stop_clock(host, clock);
            let cli = Cli::parse_from([
                "oplus-edl",
                "partitions",
                "--loader",
                loader.to_str().unwrap(),
                "--wait-timeout",
                "0",
                "--json",
            ]);
            let mut terminal = None;
            let code = run_and_observe(
                cli,
                service,
                writer,
                |_| {},
                |result| {
                    terminal = Some((result.status, result.stop_reason, result.session.generation));
                },
            );
            (code, terminal)
        });
        entered.recv_timeout(limit).unwrap();
        accepted.recv_timeout(limit).unwrap();
        release.send(()).unwrap();
        let (code, terminal) = thread.join().unwrap();
        assert_eq!(code, 74);
        assert_eq!(terminal, Some((Status::Stopped, Some(expected), None)));
    }

    #[test]
    fn broken_pipe_requests_consumer_lost_and_waits_for_the_real_result() {
        check_active_output_loss(BrokenPipe, StopReason::ConsumerLost);
    }

    #[test]
    fn stalled_output_requests_output_failed_and_waits_for_the_real_result() {
        let (release, blocked) = mpsc::channel();
        check_active_output_loss(Stalled(Some(blocked)), StopReason::OutputFailed);
        let _ = release.send(());
    }

    struct WaitForBrokenPipe(mpsc::Receiver<()>);

    impl Write for WaitForBrokenPipe {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            self.0.recv_timeout(Duration::from_secs(5)).unwrap();
            Err(io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn in_flight_program_output_loss_preserves_the_first_stop_and_wire_outcome() {
        use super::scripted_device::*;
        for timely in [true, false] {
            let root = tempfile::tempdir().unwrap();
            let loader = root.path().join("loader.elf");
            let image = root.path().join("boot.bin");
            let xml = root.path().join("raw.xml");
            std::fs::write(&loader, b"abcd").unwrap();
            std::fs::write(&image, [0x31; 512]).unwrap();
            let program = r#"<program filename="boot.bin" label="boot" SECTOR_SIZE_IN_BYTES="512" physical_partition_number="0" start_sector="64" num_partition_sectors="1"/>"#;
            std::fs::write(&xml, format!("<data>{program}{program}</data>")).unwrap();
            let (terminal, pause) = pause_response(if timely { bytes(ACK) } else { Input::Eof });
            let mut script = initialization(false);
            script.extend([response(RAW_ACK), terminal]);
            let (host, trace) = scripted_host(script);
            let (fail, writer) = mpsc::channel();
            let (registered, signal) = mpsc::channel();
            let (stop, accepted) = mpsc::channel();
            let cli = Cli::parse_from([
                "oplus-edl",
                "flash",
                "xml",
                "--program",
                xml.to_str().unwrap(),
                "--loader",
                loader.to_str().unwrap(),
                "--memory",
                "emmc",
                "--verify",
                "none",
                "--yes",
                "--json",
            ]);
            let driver = thread::spawn(move || {
                let owner = thread::current().id();
                let clock = Arc::new(move || {
                    if thread::current().id() == owner {
                        let _ = stop.send(());
                    }
                    Instant::now()
                });
                let service = service_with_stop_clock(host, clock);
                let mut terminal = None;
                let code = run_and_observe(
                    cli,
                    service,
                    WaitForBrokenPipe(writer),
                    |handler| registered.send(handler).unwrap(),
                    |result| terminal = Some(result.clone()),
                );
                (code, terminal.unwrap())
            });
            let handler = signal.recv_timeout(Duration::from_secs(4)).unwrap();
            pause.arrive();
            if !timely {
                handler();
            }
            fail.send(()).unwrap();
            if timely {
                accepted.recv_timeout(Duration::from_secs(4)).unwrap();
            }
            pause.release();
            let (code, terminal) = driver.join().unwrap();
            assert_eq!(code, 74);
            assert_eq!(
                terminal.stop_reason,
                Some(if timely {
                    StopReason::ConsumerLost
                } else {
                    StopReason::Signal
                })
            );
            assert_eq!(
                terminal.status,
                if timely {
                    Status::Stopped
                } else {
                    Status::UnknownWrite
                }
            );
            assert_eq!(
                terminal.items[0].write,
                if timely {
                    WriteState::Acknowledged
                } else {
                    WriteState::Unknown
                }
            );
            assert_eq!(terminal.items[0].transferred_bytes.as_deref(), Some("512"));
            assert_eq!(terminal.items[1].write, WriteState::NotAttempted);
            assert_eq!(
                wire_commands(&trace)
                    .iter()
                    .map(|cmd| cmd.name.as_str())
                    .collect::<Vec<_>>(),
                ["configure", "program"]
            );
        }
    }
}
