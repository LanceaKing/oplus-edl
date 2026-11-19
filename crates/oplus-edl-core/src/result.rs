use crate::{
    input::Changed,
    model::{Backend, InitializationStep, PartitionSnapshot, SessionId, StopReason},
    session::{ResponseWaitExpired, find_source, response_wait_expired},
};
use serde::Serialize;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Success,
    Stopped,
    VerificationIncomplete,
    Unsupported,
    InputError,
    InputChanged,
    ExecutionFailed,
    UnknownWrite,
}
#[derive(Clone, Debug, Serialize)]
pub struct Error {
    pub category: Status,
    pub message: String,
    pub code: Option<&'static str>,
    #[serde(skip)]
    pub path: Option<PathBuf>,
    #[serde(skip)]
    pub cause: Option<ResponseWaitExpired>,
    #[serde(skip)]
    pub io_kind: Option<std::io::ErrorKind>,
}
impl Error {
    pub fn new(category: Status, message: impl Into<String>) -> Self {
        Self {
            category,
            message: message.into(),
            code: None,
            path: None,
            cause: None,
            io_kind: None,
        }
    }

    pub(crate) fn from_source(category: Status, source: &anyhow::Error) -> Self {
        Self {
            category,
            message: format!("{source:#}"),
            code: (source.downcast_ref::<crate::service::SubmitError>()
                == Some(&crate::service::SubmitError::StaleSnapshot))
            .then_some("stale_preparation"),
            path: source
                .downcast_ref::<Changed>()
                .map(|changed| changed.0.clone()),
            cause: response_wait_expired(source).then_some(ResponseWaitExpired),
            io_kind: find_source::<std::io::Error>(source).map(std::io::Error::kind),
        }
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct Target {
    pub device: Option<String>,
    pub backend: Option<String>,
    pub memory: Option<String>,
    pub luns: Vec<u8>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Coverage {
    pub limited: bool,
    pub reason: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionState {
    NotAttempted,
    Done,
    Failed,
    Skipped,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WriteState {
    NotApplicable,
    NotAttempted,
    Acknowledged,
    Failed,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationState {
    NotApplicable,
    NotRequested,
    Matched,
    Mismatch,
    Incomplete,
}

#[derive(Clone, Debug, Serialize)]
pub struct Range {
    pub start_byte: Option<String>,
    pub length_bytes: Option<String>,
    pub verification: VerificationState,
    pub reason: Option<String>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Item {
    pub id: usize,
    pub operation: String,
    pub lun: Option<u8>,
    #[serde(skip)]
    pub slot: Option<u8>,
    pub partition: Option<String>,
    #[serde(skip)]
    pub source: Option<PathBuf>,
    pub xml_node: Option<usize>,
    pub start_byte: Option<String>,
    pub source_bytes: Option<String>,
    pub partition_bytes: Option<String>,
    pub transfer_bytes: Option<String>,
    pub transferred_bytes: Option<String>,
    pub execution: ExecutionState,
    pub write: WriteState,
    pub verification: VerificationState,
    pub ranges: Vec<Range>,
    pub reason: Option<String>,
    pub errors: Vec<Error>,
}
impl Item {
    pub fn read(lun: u8, partition: Option<String>, start: u64, size: u64) -> Self {
        Self {
            id: 0,
            operation: "read".into(),
            lun: Some(lun),
            slot: None,
            partition,
            source: None,
            xml_node: None,
            start_byte: Some(start.to_string()),
            source_bytes: None,
            partition_bytes: Some(size.to_string()),
            transfer_bytes: Some(size.to_string()),
            transferred_bytes: Some("0".into()),
            execution: ExecutionState::NotAttempted,
            write: WriteState::NotApplicable,
            verification: VerificationState::NotApplicable,
            ranges: vec![],
            reason: None,
            errors: vec![],
        }
    }
    pub fn mutation_failure(&mut self, failure: &qdl::types::MutationFailure) {
        use qdl::types::MutationConfirmation;
        let (write, category) = match failure.confirmation {
            MutationConfirmation::NotSent => (WriteState::NotAttempted, Status::ExecutionFailed),
            MutationConfirmation::Rejected => (WriteState::Failed, Status::ExecutionFailed),
            MutationConfirmation::Unknown => (WriteState::Unknown, Status::UnknownWrite),
        };
        self.write = write;
        self.verification = VerificationState::NotApplicable;
        self.fail_source(category, &failure.cause);
    }
    pub fn reject(&mut self, category: Status, message: impl Into<String>) {
        self.execution = ExecutionState::NotAttempted;
        self.errors.push(Error::new(category, message));
    }
    pub fn fail(&mut self, category: Status, message: impl Into<String>) {
        self.execution = ExecutionState::Failed;
        self.errors.push(Error::new(category, message));
    }
    pub(crate) fn fail_source(&mut self, category: Status, source: &anyhow::Error) {
        self.execution = ExecutionState::Failed;
        self.errors.push(Error::from_source(category, source));
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionDisposition {
    #[default]
    None,
    Reusable,
    Closed,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct SessionResult {
    pub generation: Option<SessionId>,
    pub disposition: SessionDisposition,
    pub reason: Option<&'static str>,
    pub completed_initialization_steps: Vec<InitializationStep>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputState {
    Complete,
    Partial,
}

#[derive(Clone, Debug, Serialize)]
pub struct RetainedOutput {
    pub path: PathBuf,
    pub kind: &'static str,
    pub state: OutputState,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InputValidity {
    Valid,
    Changed,
    #[default]
    Unchecked,
    NotApplicable,
}

#[derive(Clone, Debug, Serialize)]
pub struct DeviceCandidate {
    pub key: String,
    pub backend: Backend,
}

#[derive(Clone, Debug, Serialize)]
pub struct CloudAssets {
    pub loader: PathBuf,
    pub digest: PathBuf,
    pub signature: PathBuf,
    pub manifest: PathBuf,
    pub profile_id: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(untagged)]
pub enum ResultData {
    Candidates {
        candidates: Vec<DeviceCandidate>,
    },
    Snapshot {
        snapshot: PartitionSnapshot,
    },
    Preparation {
        preparation: Box<crate::package::Preparation>,
    },
    Assets {
        assets: CloudAssets,
    },
    Identity {
        identity_path: PathBuf,
        created: bool,
    },
}

#[derive(Clone, Debug, Serialize)]
pub struct OperationResult {
    pub command: Option<String>,
    pub verify_mode: Option<String>,
    pub target: Option<Target>,
    pub coverage: Coverage,
    pub candidates: Vec<String>,
    pub status: Status,
    pub items: Vec<Item>,
    pub errors: Vec<Error>,
    pub operation_id: Option<u64>,
    pub stop_reason: Option<StopReason>,
    pub session: SessionResult,
    pub warnings: Vec<Error>,
    pub outputs: Vec<RetainedOutput>,
    pub data: Option<ResultData>,
    pub input_validity: InputValidity,
    #[serde(skip)]
    pub(crate) unusable: bool,
}
pub type Report = OperationResult;

impl OperationResult {
    pub fn new(command: Option<&str>, verify: Option<&str>) -> Self {
        Self {
            command: command.map(Into::into),
            verify_mode: verify.map(Into::into),
            target: None,
            coverage: Coverage {
                limited: true,
                reason: Some("storage scope not discovered".into()),
            },
            candidates: vec![],
            status: Status::Success,
            items: vec![],
            errors: vec![],
            operation_id: None,
            stop_reason: None,
            session: SessionResult::default(),
            warnings: vec![],
            outputs: vec![],
            data: None,
            input_validity: InputValidity::Unchecked,
            unusable: false,
        }
    }
    pub fn error(&mut self, category: Status, message: impl Into<String>) {
        self.errors.push(Error::new(category, message));
        self.summarize();
    }
    pub(crate) fn error_source(&mut self, category: Status, source: &anyhow::Error) {
        self.errors.push(Error::from_source(category, source));
        self.summarize();
    }

    pub(crate) fn output(&mut self, path: PathBuf, kind: &'static str, state: OutputState) {
        if let Some(existing) = self.outputs.iter_mut().find(|output| output.path == path) {
            existing.state = state;
        } else {
            self.outputs.push(RetainedOutput { path, kind, state });
        }
    }
    pub fn summarize(&mut self) {
        if self
            .errors
            .iter()
            .chain(self.items.iter().flat_map(|i| &i.errors))
            .any(|error| error.category == Status::InputChanged)
        {
            self.input_validity = InputValidity::Changed;
        }
        self.status = self
            .errors
            .iter()
            .chain(self.items.iter().flat_map(|i| &i.errors))
            .map(|e| e.category)
            .max()
            .unwrap_or(Status::Success)
            .max(if self.stop_reason.is_some() {
                Status::Stopped
            } else {
                Status::Success
            });
        for (index, item) in self.items.iter_mut().enumerate() {
            item.id = index + 1;
        }
    }
}
