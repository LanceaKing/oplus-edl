use crate::model::{SessionId, StopReason};

#[derive(Clone, Debug)]
pub struct Event {
    pub phase: &'static str,
    pub message: String,
    pub operation_id: Option<u64>,
    pub session_generation: Option<SessionId>,
    pub dropped_logs: u64,
}

impl Event {
    pub fn info(phase: &'static str, message: impl Into<String>) -> Self {
        Self {
            phase,
            message: message.into(),
            operation_id: None,
            session_generation: None,
            dropped_logs: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Progress {
    pub phase: &'static str,
    pub completed_bytes: Option<u64>,
    pub total_bytes: Option<u64>,
}

#[derive(Clone, Copy, Debug)]
pub struct JobStatus {
    pub operation_id: u64,
    pub session_generation: Option<SessionId>,
    pub progress: Progress,
    pub stop_reason: Option<StopReason>,
    pub dropped_logs: u64,
}
