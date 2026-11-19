use crate::{
    coordinator,
    events::{Event, JobStatus, Progress},
    host::HostServices,
    input::{PathUse, request_paths},
    model::{
        Connection, Decision, LoaderInput, OperationRequest, PartitionSelection, PartitionSnapshot,
        PartitionTargets, PreparationId, PreparedSummary, Request, Scope, SessionId, StopReason,
    },
    result::{OperationResult, ResultData, SessionDisposition, Status},
    session::{Session, StopRequest},
};
use std::{
    collections::VecDeque,
    fmt,
    sync::{Arc, Condvar, Mutex, mpsc},
    time::Instant,
};

pub type Clock = Arc<dyn Fn() -> Instant + Send + Sync>;
type Gate = Box<dyn FnOnce() + Send>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SubmitError {
    Busy,
    InvalidRequest,
    StaleSession,
    StaleSnapshot,
    Unsupported,
    WorkerStart,
}
impl fmt::Display for SubmitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Busy => "work slot is busy",
            Self::InvalidRequest => "invalid operation or session scope",
            Self::StaleSession => "session is no longer active",
            Self::StaleSnapshot => "partition layout changed; inspect the device again",
            Self::Unsupported => "requested capability is unavailable",
            Self::WorkerStart => "unable to start the operation worker",
        })
    }
}
impl std::error::Error for SubmitError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    Device,
    Offline,
}
impl Slot {
    fn index(self) -> usize {
        match self {
            Self::Device => 0,
            Self::Offline => 1,
        }
    }
}

struct JobState {
    id: u64,
    reserves_acquisition: bool,
    #[cfg(feature = "cloud")]
    acquisition: Option<mpsc::Sender<crate::host::Task>>,
    writes_enabled: bool,
    stop: Option<StopRequest>,
    close: bool,
    abandoned: bool,
    result: Option<OperationResult>,
    prepared: Option<PreparedSummary>,
    decision: Option<Decision>,
    events: VecDeque<Event>,
    event_bytes: usize,
    dropped_logs: u64,
    session_generation: Option<SessionId>,
    progress: Progress,
    paths: Vec<PathUse>,
}

#[derive(Default)]
struct State {
    slots: [Option<JobState>; 2],
    operation: u64,
    generation: u64,
    revision: u64,
    session: Option<SessionId>,
    pending_releases: usize,
    snapshots: Vec<PartitionSnapshot>,
}

struct Shared {
    state: Mutex<State>,
    signal: Condvar,
    clock: Clock,
    lun_query_gate: Mutex<Option<Gate>>,
    #[cfg(any(test, feature = "test-support"))]
    dispatch_gate: Mutex<Option<(&'static str, Gate)>>,
}

#[derive(Clone)]
pub(crate) struct Job {
    shared: Arc<Shared>,
    slot: Slot,
    pub id: u64,
    connect_only: bool,
    snapshot: Option<Arc<PartitionSnapshot>>,
}

impl Job {
    fn state<'a>(&self, all: &'a State) -> Option<&'a JobState> {
        all.slots[self.slot.index()]
            .as_ref()
            .filter(|j| j.id == self.id)
    }

    fn state_mut<'a>(&self, all: &'a mut State) -> Option<&'a mut JobState> {
        all.slots[self.slot.index()]
            .as_mut()
            .filter(|j| j.id == self.id)
    }

    pub(crate) fn stop(&self) -> Option<StopRequest> {
        self.state(&self.shared.state.lock().unwrap())
            .and_then(|job| job.stop)
    }

    pub(crate) fn now(&self) -> Instant {
        (self.shared.clock)()
    }

    pub(crate) fn writes_enabled(&self) -> bool {
        self.state(&self.shared.state.lock().unwrap())
            .is_some_and(|job| job.writes_enabled)
    }

    #[cfg(feature = "cloud")]
    pub(crate) fn acquire_on_worker<T: Send + 'static>(
        &self,
        acquire: impl FnOnce() -> T + Send + 'static,
    ) -> anyhow::Result<T> {
        use anyhow::Context;
        if self.slot == Slot::Offline {
            return Ok(acquire());
        }
        let worker = self
            .state_mut(&mut self.shared.state.lock().unwrap())
            .and_then(|job| job.acquisition.take())
            .context("acquisition worker was not reserved")?;
        let (reply, receipt) = mpsc::channel();
        worker
            .send(Box::new(move || {
                let _ = reply.send(acquire());
            }))
            .map_err(|_| anyhow::anyhow!("acquisition worker is unavailable"))?;
        receipt.recv().context("acquisition worker lost its result")
    }

    pub(crate) fn reserve_session_id(&self) -> SessionId {
        let mut state = self.shared.state.lock().unwrap();
        state.generation += 1;
        let id = SessionId(state.generation);
        self.state_mut(&mut state).unwrap().session_generation = Some(id);
        id
    }

    pub(crate) fn release_previous_session(
        &self,
        session: &mut Option<Session>,
    ) -> anyhow::Result<()> {
        let mut state = self.shared.state.lock().unwrap();
        if self.state(&state).unwrap().stop.is_some() {
            return Err(crate::session::Stopped.into());
        }
        if let Some(previous) = session.take() {
            state.session = None;
            state.snapshots.clear();
            drop(state);
            drop(previous);
        }
        Ok(())
    }

    pub(crate) fn lun_query_gate(&self) {
        let gate = self.shared.lun_query_gate.lock().unwrap().take();
        if let Some(gate) = gate {
            gate();
        }
    }

    pub(crate) fn dispatch_gate(&self, _command: &str) {
        #[cfg(any(test, feature = "test-support"))]
        {
            let gate = {
                let mut slot = self.shared.dispatch_gate.lock().unwrap();
                if slot.as_ref().is_some_and(|(name, _)| *name == _command) {
                    slot.take()
                } else {
                    None
                }
            };
            if let Some((_, gate)) = gate {
                gate();
            }
        }
    }

    pub(crate) fn log(&self, phase: &'static str, message: &str) {
        let mut state = self.shared.state.lock().unwrap();
        if let Some(job) = self.state_mut(&mut state) {
            let end = message.floor_char_boundary(message.len().min(8 * 1024));
            let mut event = Event::info(phase, &message[..end]);
            event.operation_id = Some(self.id);
            event.session_generation = job.session_generation;
            job.event_bytes += event.message.len();
            job.events.push_back(event);
            while job.events.len() > 256 || job.event_bytes > 512 * 1024 {
                job.event_bytes -= job.events.pop_front().unwrap().message.len();
                job.dropped_logs += 1;
            }
        }
    }

    pub(crate) fn progress(&self, phase: &'static str, counts: Option<(u64, u64)>) {
        let mut state = self.shared.state.lock().unwrap();
        if let Some(job) = self.state_mut(&mut state) {
            job.progress = Progress {
                phase,
                completed_bytes: counts.map(|(completed, _)| completed),
                total_bytes: counts.map(|(_, total)| total),
            };
        }
    }

    pub(crate) fn check_gpt(&self, lun: u8, table: &crate::storage::Table) -> anyhow::Result<()> {
        let Some(snapshot) = &self.snapshot else {
            return Ok(());
        };
        let observed =
            crate::storage::snapshot_partitions(table, lun, snapshot.generation, snapshot.revision);
        if !snapshot.luns.contains(&lun)
            || !snapshot
                .partitions
                .iter()
                .filter(|key| key.lun == lun)
                .eq(observed.iter())
        {
            self.shared.state.lock().unwrap().snapshots.clear();
            return Err(SubmitError::StaleSnapshot.into());
        }
        Ok(())
    }

    pub(crate) fn consent(&self, summary: PreparedSummary) -> bool {
        let mut state = self.shared.state.lock().unwrap();
        let job = self.state_mut(&mut state).unwrap();
        if job.stop.is_some() {
            return false;
        }
        job.prepared = Some(summary);
        job.progress = Progress {
            phase: "awaiting_consent",
            ..Progress::default()
        };
        state = self
            .shared
            .signal
            .wait_while(state, |state| {
                self.state(state)
                    .is_some_and(|job| job.stop.is_none() && job.decision.is_none())
            })
            .unwrap();
        let job = self.state_mut(&mut state).unwrap();
        job.prepared = None;
        job.stop.is_none() && job.decision == Some(Decision::Approve)
    }

    pub(crate) fn snapshot_revision(&self) -> Option<u64> {
        self.snapshot.as_ref().map(|snapshot| snapshot.revision)
    }

    pub(crate) fn reserve_paths(&self, request: &Request) -> anyhow::Result<()> {
        self.bind_paths(request_paths(request))
    }

    pub(crate) fn bind_paths(&self, paths: Vec<PathUse>) -> anyhow::Result<()> {
        let mut state = self.shared.state.lock().unwrap();
        let conflicts = state.slots.iter().flatten().any(|other| {
            other.id != self.id
                && other.result.is_none()
                && other
                    .paths
                    .iter()
                    .any(|other| paths.iter().any(|path| path.conflicts(other)))
        });
        anyhow::ensure!(
            !conflicts,
            "another job uses an overlapping input or output path"
        );
        self.state_mut(&mut state).unwrap().paths.extend(paths);
        Ok(())
    }

    fn finish(
        &self,
        mut result: OperationResult,
        mut device: Option<&mut Option<Session>>,
        snapshot: Option<PartitionSnapshot>,
    ) {
        self.dispatch_gate("terminal commit");
        let mut state = self.shared.state.lock().unwrap();
        let control = self.state(&state).unwrap();
        let stopped_connect =
            self.connect_only && result.session.generation.is_some() && control.stop.is_some();
        if let Some(session) = device.as_mut() {
            if result.unusable || control.close || stopped_connect {
                state.session = None;
                state.snapshots.clear();
                // The worker drops its channel before making the result observable.
                let channel = session.take();
                drop(state);
                drop(channel);
                state = self.shared.state.lock().unwrap();
                if result.session.generation.is_some() {
                    result.session.disposition = SessionDisposition::Closed;
                    result.session.reason = Some(if self.state(&state).unwrap().close {
                        "requested"
                    } else if stopped_connect && !result.unusable {
                        "initialization_stopped"
                    } else {
                        result.session.reason.unwrap_or("exchange_failed")
                    });
                }
            } else if let Some(session) = session.as_ref() {
                if state.session != Some(session.id) {
                    state.snapshots.clear();
                }
                state.session = Some(session.id);
                if result.session.generation.is_some() {
                    result.session.disposition = SessionDisposition::Reusable;
                }
            } else {
                state.session = None;
                state.snapshots.clear();
            }
        }
        result.operation_id = Some(self.id);
        result.stop_reason = self.state(&state).unwrap().stop.map(|stop| stop.reason);
        result.summarize();
        if result.status != Status::Success
            && matches!(
                result.data,
                Some(ResultData::Preparation { .. } | ResultData::Assets { .. })
            )
        {
            result.data = None;
        }
        if let Some(mut snapshot) = snapshot
            && result.status == Status::Success
            && state.session == Some(snapshot.generation)
        {
            // Stop acceptance, revision allocation and terminal commit use this same lock.
            state.revision += 1;
            snapshot.revision = state.revision;
            for partition in &mut snapshot.partitions {
                partition.snapshot_revision = snapshot.revision;
            }
            state.snapshots.retain(|old| old.scope != snapshot.scope);
            state.snapshots.push(snapshot.clone());
            result.data = Some(ResultData::Snapshot { snapshot });
        }
        if result.items.iter().any(|item| {
            matches!(
                item.write,
                crate::result::WriteState::Acknowledged | crate::result::WriteState::Unknown
            ) && matches!(item.operation.as_str(), "program" | "patch")
        }) {
            state.snapshots.clear();
        }
        let job = self.state_mut(&mut state).unwrap();
        #[cfg(feature = "cloud")]
        job.acquisition.take();
        job.result = Some(result);
    }
}

pub struct JobHandle {
    job: Job,
}

impl JobHandle {
    pub fn operation_id(&self) -> u64 {
        self.job.id
    }

    pub fn request_stop(&self) {
        self.request_stop_with_reason(StopReason::User);
    }

    pub fn request_stop_with_reason(&self, reason: StopReason) {
        let mut state = self.job.shared.state.lock().unwrap();
        if let Some(job) = self.job.state_mut(&mut state)
            && job.result.is_none()
            && job.stop.is_none()
        {
            job.stop = Some(StopRequest {
                at: self.job.now(),
                reason,
            });
            self.job.shared.signal.notify_all();
        }
    }

    pub fn prepared_summary(&self) -> Option<PreparedSummary> {
        self.job
            .state(&self.job.shared.state.lock().unwrap())?
            .prepared
            .clone()
    }

    pub fn decide(&self, id: PreparationId, decision: Decision) -> Result<(), SubmitError> {
        let mut state = self.job.shared.state.lock().unwrap();
        let job = self
            .job
            .state_mut(&mut state)
            .ok_or(SubmitError::InvalidRequest)?;
        if job.prepared.as_ref().map(|summary| summary.id) != Some(id)
            || job.decision.is_some()
            || job.stop.is_some()
        {
            return Err(SubmitError::InvalidRequest);
        }
        job.decision = Some(decision);
        self.job.shared.signal.notify_all();
        Ok(())
    }

    pub fn poll_event(&self) -> Option<Event> {
        let mut state = self.job.shared.state.lock().unwrap();
        let job = self.job.state_mut(&mut state)?;
        let mut event = job.events.pop_front()?;
        job.event_bytes -= event.message.len();
        event.dropped_logs = job.dropped_logs;
        Some(event)
    }

    pub fn status(&self) -> Option<JobStatus> {
        let state = self.job.shared.state.lock().unwrap();
        let job = self.job.state(&state)?;
        Some(JobStatus {
            operation_id: job.id,
            session_generation: job.session_generation,
            progress: job.progress,
            stop_reason: job.stop.map(|stop| stop.reason),
            dropped_logs: job.dropped_logs,
        })
    }

    pub fn try_take_result(&self) -> Option<OperationResult> {
        let mut state = self.job.shared.state.lock().unwrap();
        let result = self.job.state_mut(&mut state)?.result.take()?;
        state.slots[self.job.slot.index()] = None;
        Some(result)
    }
}

impl Drop for JobHandle {
    fn drop(&mut self) {
        let mut state = self.job.shared.state.lock().unwrap();
        if let Some(job) = self.job.state_mut(&mut state) {
            job.abandoned = true;
            if job.result.is_none() && job.stop.is_none() {
                job.stop = Some(StopRequest {
                    at: self.job.now(),
                    reason: StopReason::ConsumerLost,
                });
                self.job.shared.signal.notify_all();
            }
        }
    }
}

enum Message {
    Run(Box<Request>, Job),
    Close(Option<mpsc::Sender<()>>),
}

pub struct CoreService {
    host: HostServices,
    shared: Arc<Shared>,
    worker: Mutex<Option<mpsc::Sender<Message>>>,
}

impl CoreService {
    pub fn new(host: HostServices) -> Self {
        Self::with_control(host, Arc::new(Instant::now), None)
    }

    pub(crate) fn with_control(host: HostServices, clock: Clock, gate: Option<Gate>) -> Self {
        Self {
            host,
            shared: Arc::new(Shared {
                state: Mutex::new(State::default()),
                signal: Condvar::new(),
                clock,
                lun_query_gate: Mutex::new(gate),
                #[cfg(any(test, feature = "test-support"))]
                dispatch_gate: Mutex::new(None),
            }),
            worker: Mutex::new(None),
        }
    }

    #[cfg(any(feature = "test-support", all(test, feature = "cloud")))]
    pub(crate) fn set_dispatch_gate(&self, command: &'static str, gate: Gate) {
        *self.shared.dispatch_gate.lock().unwrap() = Some((command, gate));
    }

    pub fn submit(&self, request: Request) -> Result<JobHandle, SubmitError> {
        self.submit_with_writes_enabled(request, true)
    }

    pub fn submit_with_writes_enabled(
        &self,
        request: Request,
        writes_enabled: bool,
    ) -> Result<JobHandle, SubmitError> {
        self.submit_bound(request, writes_enabled, None)
    }

    /// Reserve work against a snapshot that is still current for the selected session.
    pub fn submit_for_snapshot(
        &self,
        request: Request,
        generation: SessionId,
        revision: u64,
    ) -> Result<JobHandle, SubmitError> {
        if !matches!(&request, Request::Run {
            connection: Connection::Existing(id),
            operation: OperationRequest::Backup { .. } | OperationRequest::Restore { .. },
        } if *id == generation)
        {
            return Err(SubmitError::InvalidRequest);
        }
        self.submit_bound(request, true, Some((generation, revision)))
    }

    fn submit_bound(
        &self,
        request: Request,
        writes_enabled: bool,
        binding: Option<(SessionId, u64)>,
    ) -> Result<JobHandle, SubmitError> {
        let slot = self.validate(&request)?;
        let reserves_acquisition = matches!(&request,
            Request::Connect(config) | Request::Run { connection: Connection::Open(config), .. }
            if matches!(config.loader, LoaderInput::Cloud(_))
        );
        let mut worker = self.worker.lock().unwrap();
        let mut state = self.shared.state.lock().unwrap();
        if state.slots[slot.index()].is_some()
            || (slot == Slot::Device && state.pending_releases != 0)
            || (reserves_acquisition && state.slots[Slot::Offline.index()].is_some())
            || (slot == Slot::Offline
                && state.slots[Slot::Device.index()]
                    .as_ref()
                    .is_some_and(|job| job.reserves_acquisition))
        {
            return Err(SubmitError::Busy);
        }
        if let Request::Inspect(id, _)
        | Request::Run {
            connection: Connection::Existing(id),
            ..
        } = &request
            && state.session != Some(*id)
        {
            return Err(SubmitError::StaleSession);
        }
        let snapshot = binding
            .map(|(generation, revision)| {
                state
                    .snapshots
                    .iter()
                    .find(|snapshot| {
                        snapshot.generation == generation && snapshot.revision == revision
                    })
                    .cloned()
                    .map(Arc::new)
                    .ok_or(SubmitError::StaleSnapshot)
            })
            .transpose()?;
        if slot == Slot::Device && worker.is_none() {
            let (sender, receiver) = mpsc::channel();
            let host = self.host.clone();
            let shared = self.shared.clone();
            self.host
                .spawn(Box::new(move || device_worker(host, shared, receiver)))
                .map_err(|_| SubmitError::WorkerStart)?;
            *worker = Some(sender);
        }
        #[cfg(feature = "cloud")]
        let acquisition = if reserves_acquisition {
            let (sender, receiver) = mpsc::channel::<crate::host::Task>();
            self.host
                .spawn(Box::new(move || {
                    if let Ok(task) = receiver.recv() {
                        task();
                    }
                }))
                .map_err(|_| SubmitError::WorkerStart)?;
            Some(sender)
        } else {
            None
        };
        state.operation += 1;
        let job = Job {
            shared: self.shared.clone(),
            slot,
            id: state.operation,
            connect_only: matches!(request, Request::Connect(_)),
            snapshot,
        };
        let (session_generation, phase) = match &request {
            Request::Discover { .. } => (None, "discovery"),
            Request::Connect(_) => (None, "initialization"),
            Request::Inspect(id, _) => (Some(*id), "inspection"),
            Request::Run { connection, .. } => (
                match connection {
                    Connection::Existing(id) => Some(*id),
                    _ => None,
                },
                "preparation",
            ),
        };
        state.slots[slot.index()] = Some(JobState {
            id: job.id,
            reserves_acquisition,
            #[cfg(feature = "cloud")]
            acquisition,
            writes_enabled,
            stop: None,
            close: false,
            abandoned: false,
            result: None,
            prepared: None,
            decision: None,
            events: VecDeque::new(),
            event_bytes: 0,
            dropped_logs: 0,
            session_generation,
            progress: Progress {
                phase,
                ..Progress::default()
            },
            paths: Vec::new(),
        });
        let dispatched = match slot {
            Slot::Device => worker
                .as_ref()
                .unwrap()
                .send(Message::Run(Box::new(request), job.clone()))
                .is_ok(),
            Slot::Offline => {
                let job = job.clone();
                let host = self.host.clone();
                self.host
                    .spawn(Box::new(move || {
                        let result = coordinator::run_offline(request, &host, &job);
                        job.finish(result, None, None);
                    }))
                    .is_ok()
            }
        };
        if !dispatched {
            state.slots[slot.index()] = None;
            return Err(SubmitError::WorkerStart);
        }
        Ok(JobHandle { job })
    }

    fn validate(&self, request: &Request) -> Result<Slot, SubmitError> {
        let (backend, loader, offline) = match request {
            Request::Discover { backend } => (Some(*backend), None, false),
            Request::Connect(config) => (Some(config.backend), Some(&config.loader), false),
            Request::Inspect(_, scope) => {
                validate_scope(scope)?;
                (None, None, false)
            }
            Request::Run {
                connection,
                operation,
            } => {
                if let OperationRequest::Inspect { scope } = operation {
                    validate_scope(scope)?;
                    if !matches!(connection, Connection::Open(_)) {
                        return Err(SubmitError::InvalidRequest);
                    }
                }
                if let OperationRequest::Backup { selection, .. }
                | OperationRequest::Restore { selection, .. } = operation
                {
                    validate_partition_selection(selection)?;
                }
                let cloud = matches!(
                    operation,
                    OperationRequest::CloudFetch(_) | OperationRequest::CloudIdentity { .. }
                );
                let offline = cloud || matches!(operation, OperationRequest::PreparePackage { .. });
                if offline != matches!(connection, Connection::Offline) {
                    return Err(SubmitError::InvalidRequest);
                }
                if cloud && !cfg!(feature = "cloud") {
                    return Err(SubmitError::Unsupported);
                }
                match connection {
                    Connection::Open(config) => (Some(config.backend), Some(&config.loader), false),
                    _ => (None, None, offline),
                }
            }
        };
        if backend.is_some_and(|backend| !self.host.supports(backend))
            || (matches!(loader, Some(LoaderInput::Cloud(_))) && !cfg!(feature = "cloud"))
        {
            return Err(SubmitError::Unsupported);
        }
        Ok(if offline { Slot::Offline } else { Slot::Device })
    }

    pub fn request_close(&self, reason: StopReason) {
        let worker = self.worker.lock().unwrap();
        let mut state = self.shared.state.lock().unwrap();
        if let Some(job) = &mut state.slots[Slot::Device.index()]
            && job.result.is_none()
        {
            job.close = true;
            job.stop.get_or_insert_with(|| StopRequest {
                at: (self.shared.clock)(),
                reason,
            });
            self.shared.signal.notify_all();
        } else {
            state.session = None;
            state.snapshots.clear();
            if state.pending_releases == 0
                && let Some(worker) = worker.as_ref()
                && worker.send(Message::Close(None)).is_ok()
            {
                state.pending_releases += 1;
            }
        }
    }

    pub fn session_is_current(&self, id: SessionId) -> bool {
        self.shared.state.lock().unwrap().session == Some(id)
    }

    pub fn snapshot_is_current(&self, id: SessionId, revision: u64) -> bool {
        let state = self.shared.state.lock().unwrap();
        state.session == Some(id)
            && state
                .snapshots
                .iter()
                .any(|snapshot| snapshot.revision == revision)
    }

    pub fn device_released(&self) -> bool {
        let state = self.shared.state.lock().unwrap();
        state.session.is_none()
            && state.pending_releases == 0
            && state.slots[Slot::Device.index()]
                .as_ref()
                .is_none_or(|job| job.result.is_some())
    }

    /// Release a finished CLI session on its owning thread before rendering.
    pub fn close_idle(&self) -> bool {
        let (done, receipt) = mpsc::channel();
        {
            let worker = self.worker.lock().unwrap();
            let mut state = self.shared.state.lock().unwrap();
            if state.slots[Slot::Device.index()]
                .as_ref()
                .is_some_and(|job| job.result.is_none())
            {
                return false;
            }
            state.session = None;
            state.snapshots.clear();
            if !worker
                .as_ref()
                .is_some_and(|worker| worker.send(Message::Close(Some(done))).is_ok())
            {
                return false;
            }
            state.pending_releases += 1;
        }
        receipt.recv().is_ok()
    }

    pub fn take_abandoned_result(&self, slot: Slot) -> Option<OperationResult> {
        let mut state = self.shared.state.lock().unwrap();
        let job = state.slots[slot.index()].as_mut()?;
        if !job.abandoned {
            return None;
        }
        let result = job.result.take()?;
        state.slots[slot.index()] = None;
        Some(result)
    }

    #[cfg(feature = "test-support")]
    pub(crate) fn committed_snapshot(
        &self,
        id: SessionId,
        scope: &Scope,
    ) -> Option<PartitionSnapshot> {
        let state = self.shared.state.lock().unwrap();
        if state.session != Some(id) {
            return None;
        }
        state
            .snapshots
            .iter()
            .find(|snapshot| &snapshot.scope == scope)
            .cloned()
    }
}

impl Drop for CoreService {
    fn drop(&mut self) {
        self.request_close(StopReason::ConsumerLost);
        let mut state = self.shared.state.lock().unwrap();
        if let Some(job) = &mut state.slots[Slot::Offline.index()]
            && job.result.is_none()
        {
            job.stop.get_or_insert_with(|| StopRequest {
                at: (self.shared.clock)(),
                reason: StopReason::ConsumerLost,
            });
        }
        self.shared.signal.notify_all();
    }
}

fn validate_scope(scope: &Scope) -> Result<(), SubmitError> {
    if let Scope::Selected(luns) = scope
        && (luns.is_empty()
            || luns
                .iter()
                .enumerate()
                .any(|(i, lun)| luns[..i].contains(lun)))
    {
        return Err(SubmitError::InvalidRequest);
    }
    Ok(())
}

fn validate_partition_selection(selection: &PartitionSelection) -> Result<(), SubmitError> {
    let luns = &selection.luns;
    if luns
        .iter()
        .enumerate()
        .any(|(index, lun)| luns[..index].contains(lun))
    {
        return Err(SubmitError::InvalidRequest);
    }
    if let PartitionTargets::Selected(targets) = &selection.partitions
        && (targets.is_empty()
            || targets.iter().enumerate().any(|(index, target)| {
                target.name.is_empty()
                    || targets[..index]
                        .iter()
                        .any(|previous| previous.lun == target.lun && previous.name == target.name)
                    || target
                        .lun
                        .is_some_and(|lun| !luns.is_empty() && !luns.contains(&lun))
            }))
    {
        return Err(SubmitError::InvalidRequest);
    }
    Ok(())
}

fn device_worker(host: HostServices, shared: Arc<Shared>, receiver: mpsc::Receiver<Message>) {
    let mut session: Option<Session> = None;
    for message in receiver {
        match message {
            Message::Run(request, job) => {
                let (result, snapshot) =
                    coordinator::run_device(*request, &host, &mut session, &job);
                job.finish(result, Some(&mut session), snapshot);
            }
            Message::Close(receipt) => {
                session.take();
                shared.state.lock().unwrap().pending_releases -= 1;
                if let Some(receipt) = receipt {
                    let _ = receipt.send(());
                }
            }
        }
    }
}
