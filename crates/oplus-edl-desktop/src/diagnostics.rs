use oplus_edl_core::{OperationResult, events::Event, model::SessionId, result::Item};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    io,
    path::{Path, PathBuf},
    sync::{Arc, mpsc},
    thread,
};

pub(crate) struct PendingExport {
    destination: PathBuf,
    receipt: mpsc::Receiver<io::Result<()>>,
}

impl PendingExport {
    pub(crate) fn spawn(
        destination: PathBuf,
        write: impl FnOnce(&Path) -> io::Result<()> + Send + 'static,
    ) -> io::Result<Self> {
        let path = destination.clone();
        let (sender, receipt) = mpsc::channel();
        thread::Builder::new()
            .name("oplus-edl-diagnostics".into())
            .spawn(move || {
                let _ = sender.send(write(&path));
            })?;
        Ok(Self {
            destination,
            receipt,
        })
    }

    pub(crate) fn poll(&self) -> Option<String> {
        match self.receipt.try_recv() {
            Ok(Ok(())) => Some(format!("已导出到 {}", self.destination.display())),
            Ok(Err(error)) => Some(format!("导出失败 · {error}")),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some("导出任务未返回结果".into()),
        }
    }
}

struct Phase {
    name: &'static str,
    operation_id: Option<u64>,
    session_generation: Option<SessionId>,
}

pub(crate) struct Snapshot {
    phases: Vec<Phase>,
    dropped_logs: u64,
    result: Option<Arc<OperationResult>>,
}

impl Snapshot {
    pub(crate) fn capture(
        logs: &VecDeque<Event>,
        dropped_logs: u64,
        result: Option<Arc<OperationResult>>,
    ) -> Self {
        let phases = logs
            .iter()
            .map(|event| Phase {
                name: event.phase,
                operation_id: event.operation_id,
                session_generation: event.session_generation,
            })
            .collect();
        Self {
            phases,
            dropped_logs,
            result,
        }
    }

    pub(crate) fn write(self, path: &Path) -> io::Result<()> {
        let phases: Vec<_> = self
            .phases
            .into_iter()
            .map(|phase| {
                json!({
                    "phase": phase.name,
                    "operation_id": phase.operation_id.map(|id| id.to_string()),
                    "session_generation": phase.session_generation.map(|id| id.0.to_string())
                })
            })
            .collect();
        let result = self.result.map(|result| {
        json!({
            "operation": result.command,
            "outcome": result.status,
            "stop_reason": result.stop_reason,
            "input_validity": result.input_validity,
            "session": result.session,
            "items": result.items.iter().map(item).collect::<Vec<_>>(),
            "errors": result.errors.iter().map(error).collect::<Vec<_>>(),
            "warnings": result.warnings.iter().map(error).collect::<Vec<_>>(),
            "outputs": result.outputs.iter().map(|output| json!({
                "path": output.path.display().to_string(), "kind": output.kind, "state": output.state
            })).collect::<Vec<_>>(),
        })
    });
        let directory = path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let mut file = tempfile::NamedTempFile::new_in(directory)?;
        serde_json::to_writer_pretty(
            file.as_file_mut(),
            &json!({
                "schema_version": 1, "phases": phases, "dropped_logs": self.dropped_logs, "result": result,
            }),
        )?;
        file.as_file().sync_all()?;
        file.persist_noclobber(path)?;
        Ok(())
    }
}

// Redact free text from devices, cloud responses, and local configuration errors.
fn error(error: &oplus_edl_core::result::Error) -> Value {
    json!({
        "category": error.category,
        "code": if error.cause.is_some() { Some("response_wait_expired") } else { error.code },
    })
}

fn item(item: &Item) -> Value {
    let ranges: Vec<_> = item
        .ranges
        .iter()
        .map(|range| {
            let reason = range.reason.as_deref().map(|reason| match reason {
                "byte_difference_observed"
                | "read_refused"
                | "unknown address"
                | "unknown expected content after patch or expression write"
                | "verification stopped"
                | "verification source changed" => reason,
                reason if reason.starts_with("read failed:") => "read_failed",
                reason if reason.starts_with("expected input:") => "expected_input_failed",
                _ => "redacted",
            });
            json!({
                "start_byte": range.start_byte,
                "length_bytes": range.length_bytes,
                "verification": range.verification,
                "reason": reason,
            })
        })
        .collect();
    json!({
        "id": item.id.to_string(), "operation": item.operation, "lun": item.lun,
        "start_byte": item.start_byte, "transfer_bytes": item.transfer_bytes,
        "transferred_bytes": item.transferred_bytes, "execution": item.execution,
        "write": item.write, "verification": item.verification,
        "ranges": ranges,
        "errors": item.errors.iter().map(error).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use oplus_edl_core::result::VerificationState;
    #[test]
    fn export_contains_phase_history_and_never_exports_untrusted_log_text() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("diagnostics.json");
        let logs = VecDeque::from([Event::info("initialization", "fixture access token")]);
        Snapshot::capture(&logs, 7, None).write(&target).unwrap();
        let text = std::fs::read_to_string(&target).unwrap();
        assert!(!text.contains("fixture access token"));
        assert!(text.contains("initialization"));
        let value: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["dropped_logs"], 7);
        assert!(Snapshot::capture(&logs, 7, None).write(&target).is_err());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), text);
    }

    #[test]
    fn export_preserves_verification_ranges_and_redacts_free_text_reasons() {
        use oplus_edl_core::result::Range;
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("diagnostics.json");
        let mut result = OperationResult::new(Some("restore"), Some("best-effort"));
        let mut item = Item::read(0, Some("boot".into()), 32768, 1024);
        for (start, reason) in [
            (32768, "byte_difference_observed"),
            (33280, "read failed: private fixture token"),
        ] {
            item.ranges.push(Range {
                start_byte: Some(start.to_string()),
                length_bytes: Some("512".into()),
                verification: VerificationState::Incomplete,
                reason: Some(reason.into()),
            });
        }
        result.items.push(item);
        result.summarize();
        Snapshot::capture(&VecDeque::new(), 0, Some(Arc::new(result)))
            .write(&target)
            .unwrap();
        let text = std::fs::read_to_string(&target).unwrap();
        assert!(!text.contains("private fixture token"));
        let value: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(
            value["result"]["items"][0]["ranges"],
            json!([
                {"start_byte":"32768","length_bytes":"512","verification":"incomplete","reason":"byte_difference_observed"},
                {"start_byte":"33280","length_bytes":"512","verification":"incomplete","reason":"read_failed"},
            ])
        );
    }
}
