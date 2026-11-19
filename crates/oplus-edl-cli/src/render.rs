use oplus_edl_core::{
    OperationResult,
    events::{Event, JobStatus, Progress},
    result::{
        Error, ExecutionState, Item, OutputState, ResultData, SessionDisposition, Status,
        VerificationState, WriteState,
    },
};
use serde_json::{Value, json};
use std::{
    io::{self, Write},
    path::Path,
};

#[derive(Clone, Copy, Debug, Default)]
pub struct OutputMode {
    pub json: bool,
    pub quiet: bool,
}

pub struct OutputWriter<W> {
    writer: W,
    mode: OutputMode,
    seq: u64,
    operation: Option<String>,
    operation_id: Option<u64>,
}

impl<W: Write> OutputWriter<W> {
    pub fn new(mode: OutputMode, writer: W) -> Self {
        Self {
            writer,
            mode,
            seq: 0,
            operation: None,
            operation_id: None,
        }
    }

    pub(crate) fn identify(&mut self, operation: &'static str, id: u64) {
        self.operation = Some(operation.into());
        self.operation_id = Some(id);
    }

    fn write_record(&mut self, record: &Value) -> io::Result<()> {
        serde_json::to_writer(&mut self.writer, record)?;
        self.writer.write_all(b"\n")?;
        self.writer.flush()
    }

    pub fn write_event(&mut self, event: &Event) -> io::Result<()> {
        self.write_observation(event, None)
    }

    pub(crate) fn write_progress(&mut self, status: JobStatus) -> io::Result<()> {
        let progress = status.progress;
        let position =
            |count: Option<u64>| count.map_or_else(|| "unknown".into(), |n| n.to_string());
        let mut event = Event::info(
            progress.phase,
            format!(
                "{}: {} / {} bytes",
                progress.phase,
                position(progress.completed_bytes),
                position(progress.total_bytes),
            ),
        );
        event.operation_id = Some(status.operation_id);
        event.session_generation = status.session_generation;
        event.dropped_logs = status.dropped_logs;
        self.write_observation(&event, Some(progress))
    }

    fn write_observation(&mut self, event: &Event, progress: Option<Progress>) -> io::Result<()> {
        if self.mode.quiet {
            return Ok(());
        }
        self.seq += 1;
        if self.mode.json {
            self.write_record(&json!({
                "schema_version": 1, "kind": "event", "seq": self.seq.to_string(),
                "operation_id": event.operation_id.or(self.operation_id).map(|id| id.to_string()),
                "session_generation": event.session_generation.map(|id| id.0.to_string()),
                "operation": self.operation, "phase": event.phase, "level": "info",
                "message": event.message, "item_id": null, "dropped_logs": event.dropped_logs,
                "progress": progress.map(|progress| json!({
                    "completed_bytes": progress.completed_bytes.map(|n| n.to_string()),
                    "total_bytes": progress.total_bytes.map(|n| n.to_string()),
                })),
            }))
        } else {
            writeln!(self.writer, "{}", event.message)?;
            self.writer.flush()
        }
    }

    pub fn finish(mut self, result: &OperationResult) -> io::Result<i32> {
        if self.mode.json {
            self.seq += 1;
            let items: Vec<_> = result.items.iter().map(wire_item).collect();
            let outputs: Vec<_> = result
                .outputs
                .iter()
                .map(|output| {
                    json!({
                        "path": wire_path(&output.path), "kind": output.kind, "state": output.state,
                    })
                })
                .collect();
            let data = match &result.data {
                Some(ResultData::Candidates { candidates }) => json!({"candidates": candidates}),
                Some(ResultData::Snapshot { snapshot }) => {
                    let mut wire = serde_json::to_value(snapshot).expect("serializable snapshot");
                    wire["generation"] = json!(snapshot.generation.0.to_string());
                    wire["revision"] = json!(snapshot.revision.to_string());
                    for key in wire["partitions"]
                        .as_array_mut()
                        .expect("snapshot partitions")
                    {
                        key["generation"] = json!(snapshot.generation.0.to_string());
                        key["snapshot_revision"] = json!(snapshot.revision.to_string());
                    }
                    json!({ "snapshot": wire })
                }
                Some(ResultData::Preparation { preparation }) => {
                    let origin = &preparation.origin;
                    json!({ "preparation": {
                        "origin": {
                            "root": wire_path(&origin.root), "profile": origin.profile,
                            "nv_id": origin.nv_id, "project": origin.project,
                            "definition": wire_path(&origin.definition),
                        },
                        "manifest": wire_path(&preparation.manifest),
                        "super_image": wire_path(&preparation.super_image),
                        "programs": wire_paths(&preparation.programs),
                        "patches": wire_paths(&preparation.patches),
                        "exclusions": preparation.exclusions,
                    } })
                }
                Some(ResultData::Assets { assets }) => {
                    json!({ "assets": {
                        "loader": wire_path(&assets.loader), "digest": wire_path(&assets.digest),
                        "signature": wire_path(&assets.signature), "manifest": wire_path(&assets.manifest),
                        "profile_id": assets.profile_id.to_string(),
                    } })
                }
                Some(ResultData::Identity {
                    identity_path,
                    created,
                }) => {
                    json!({ "identity_path": wire_path(identity_path), "created": created })
                }
                None => Value::Null,
            };
            self.write_record(&json!({
                "schema_version": 1, "kind": "result", "seq": self.seq.to_string(),
                "operation_id": result.operation_id.map(|id| id.to_string()),
                "session_generation": result.session.generation.map(|id| id.0.to_string()),
                "operation": result.command, "outcome": result.status, "target": result.target,
                "coverage": result.coverage, "items": items,
                "errors": result.errors.iter().map(|e| wire_error(e, None)).collect::<Vec<_>>(),
                "warnings": result.warnings.iter().map(|e| wire_error(e, None)).collect::<Vec<_>>(),
                "outputs": outputs, "stop_reason": result.stop_reason, "data": data,
                "input_validity": result.input_validity,
                "session": {
                    "disposition": result.session.disposition, "reason": result.session.reason,
                    "completed_initialization_steps": result.session.completed_initialization_steps,
                },
            }))?;
        } else {
            write_text(result, &mut self.writer)?;
            if let Some(reason) = result.stop_reason {
                writeln!(self.writer, "Stop: {reason:?}")?;
            }
            if result.session.disposition != SessionDisposition::None {
                writeln!(self.writer, "Session: {:?}", result.session.disposition)?;
            }
            if let Some(reason) = result.session.reason {
                writeln!(self.writer, "Session reason: {reason}")?;
            }
            for step in &result.session.completed_initialization_steps {
                writeln!(self.writer, "Initialization complete: {step:?}")?;
            }
            match &result.data {
                Some(ResultData::Snapshot { snapshot }) => {
                    for partition in &snapshot.partitions {
                        writeln!(
                            self.writer,
                            "{}:{} at sector {}, {} sectors",
                            partition.lun,
                            partition.name,
                            partition.start_sector,
                            partition.sectors
                        )?;
                    }
                }
                Some(ResultData::Preparation { preparation }) => {
                    writeln!(
                        self.writer,
                        "Package NV {} for project {}",
                        preparation.origin.nv_id, preparation.origin.project
                    )?;
                    for entry in &preparation.exclusions {
                        writeln!(
                            self.writer,
                            "Excluded {}:{} ({})",
                            entry.lun,
                            entry.label,
                            entry.reason.as_deref().unwrap_or("excluded")
                        )?;
                    }
                }
                Some(ResultData::Assets { assets }) => {
                    writeln!(self.writer, "Cloud profile {}", assets.profile_id)?;
                }
                _ => {}
            }
            self.writer.flush()?;
        }
        Ok(exit_code(result.status))
    }

    pub fn informational(mut self, topic: &str, text: &str) -> io::Result<i32> {
        if self.mode.json {
            self.write_record(&json!({
                "schema_version": 1, "kind": "result", "seq": "1",
                "operation_id": null, "session_generation": null, "operation": null,
                "outcome": "success", "target": null, "coverage": null,
                "items": [], "errors": [], "warnings": [], "outputs": [],
                "stop_reason": null, "input_validity": "not_applicable",
                "session": {"disposition": "none", "reason": null, "completed_initialization_steps": []},
                "data": {"topic": topic, "text": text},
            }))?;
        } else {
            self.writer.write_all(text.as_bytes())?;
            self.writer.flush()?;
        }
        Ok(0)
    }
}

fn write_text(result: &OperationResult, out: &mut impl Write) -> io::Result<()> {
    writeln!(
        out,
        "{}: {:?}",
        result.command.as_deref().unwrap_or("input"),
        result.status
    )?;
    if let Some(reason) = &result.coverage.reason {
        writeln!(out, "Scope: {reason}")?;
    }
    for c in &result.candidates {
        writeln!(out, "Candidate: {c}")?;
    }
    for i in &result.items {
        let slot = i
            .slot
            .map_or_else(String::new, |slot| format!(" slot {slot}"));
        writeln!(
            out,
            "{}: {} LUN {}{slot} partition {:?} source {:?}: {}; write={}; verification={} ({} bytes)",
            i.id,
            i.operation,
            i.lun.map(|n| n.to_string()).as_deref().unwrap_or("unknown"),
            i.partition.as_deref().unwrap_or("-"),
            i.source
                .as_deref()
                .unwrap_or_else(|| std::path::Path::new("-")),
            execution_name(i.execution),
            write_name(i.write),
            verification_name(i.verification),
            i.transferred_bytes.as_deref().unwrap_or("unknown")
        )?;
        if let Some(reason) = &i.reason {
            writeln!(out, "  item {} reason: {reason}", i.id)?;
        }
        for range in &i.ranges {
            writeln!(
                out,
                "  range start={} bytes={}: {} ({})",
                range.start_byte.as_deref().unwrap_or("unknown"),
                range.length_bytes.as_deref().unwrap_or("unknown"),
                verification_name(range.verification),
                range.reason.as_deref().unwrap_or("")
            )?;
        }
        for e in &i.errors {
            writeln!(out, "  item {} {:?}: {}", i.id, e.category, e.message)?;
        }
    }
    for e in &result.errors {
        writeln!(out, "{:?}: {}", e.category, e.message)?;
    }
    for warning in &result.warnings {
        writeln!(out, "Warning {:?}: {}", warning.category, warning.message)?;
    }
    for output in &result.outputs {
        let state = match output.state {
            OutputState::Complete => "complete",
            OutputState::Partial => "partial",
        };
        writeln!(out, "Output {state} ({}): {:?}", output.kind, output.path)?;
    }
    Ok(())
}

fn wire_error(error: &Error, item: Option<usize>) -> Value {
    let code = if error.cause.is_some() {
        json!("response_wait_expired")
    } else if error.io_kind == Some(io::ErrorKind::TimedOut) {
        json!("io_timeout")
    } else if let Some(code) = error.code {
        json!(code)
    } else {
        json!(error.category)
    };
    json!({"code": code, "message": error.message, "item_id": item.map(|id| id.to_string()),
        "category": error.category, "path": error.path.as_deref().map(wire_path)})
}

fn wire_item(item: &Item) -> Value {
    let ranges: Vec<_> = item
        .ranges
        .iter()
        .map(|range| {
            json!({
                "start_byte": range.start_byte, "length_bytes": range.length_bytes,
                "verification": verification_name(range.verification), "reason": range.reason,
            })
        })
        .collect();
    let errors: Vec<_> = item
        .errors
        .iter()
        .map(|error| wire_error(error, Some(item.id)))
        .collect();
    json!({
        "id": item.id.to_string(), "operation": item.operation, "lun": item.lun,
        "partition": item.partition, "source": item.source.as_deref().map(wire_path),
        "xml_node": item.xml_node, "start_byte": item.start_byte,
        "source_bytes": item.source_bytes, "partition_bytes": item.partition_bytes,
        "transfer_bytes": item.transfer_bytes, "transferred_bytes": item.transferred_bytes,
        "execution": execution_name(item.execution), "write": write_name(item.write),
        "verification": verification_name(item.verification), "ranges": ranges,
        "reason": item.reason, "errors": errors,
    })
}

fn execution_name(state: ExecutionState) -> &'static str {
    match state {
        ExecutionState::NotAttempted => "not_attempted",
        ExecutionState::Done => "done",
        ExecutionState::Failed => "failed",
        ExecutionState::Skipped => "skipped",
    }
}

fn write_name(state: WriteState) -> &'static str {
    match state {
        WriteState::NotApplicable => "not_applicable",
        WriteState::NotAttempted => "not_attempted",
        WriteState::Acknowledged => "acknowledged",
        WriteState::Failed => "failed",
        WriteState::Unknown => "unknown",
    }
}

fn verification_name(state: VerificationState) -> &'static str {
    match state {
        VerificationState::NotApplicable => "not_applicable",
        VerificationState::NotRequested => "not_requested",
        VerificationState::Matched => "matched",
        VerificationState::Mismatch => "mismatch",
        VerificationState::Incomplete => "incomplete",
    }
}

fn wire_paths(paths: &[std::path::PathBuf]) -> Vec<Value> {
    paths.iter().map(|path| wire_path(path)).collect()
}

fn exit_code(status: Status) -> i32 {
    match status {
        Status::Success => 0,
        Status::InputError => 2,
        Status::Unsupported => 3,
        Status::ExecutionFailed => 4,
        Status::VerificationIncomplete => 5,
        Status::UnknownWrite => 6,
        Status::InputChanged => 7,
        Status::Stopped => 130,
    }
}

fn wire_path(path: &Path) -> Value {
    let mut value = json!({"display": path.to_string_lossy()});
    if path.to_str().is_none() {
        #[cfg(unix)]
        {
            use base64::{Engine, engine::general_purpose::STANDARD};
            use std::os::unix::ffi::OsStrExt;
            value["unix_bytes_base64"] = json!(STANDARD.encode(path.as_os_str().as_bytes()));
        }
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            value["windows_utf16"] = json!(path.as_os_str().encode_wide().collect::<Vec<_>>());
        }
    }
    value
}

#[cfg(test)]
#[test]
fn typed_item_and_range_states_keep_the_schema_one_string_values() {
    use oplus_edl_core::result::Range;
    let mut report = OperationResult::new(Some("flash"), Some("required"));
    let rows = [
        (
            ExecutionState::Done,
            WriteState::Acknowledged,
            VerificationState::Matched,
            ["done", "acknowledged", "matched"],
        ),
        (
            ExecutionState::Failed,
            WriteState::Unknown,
            VerificationState::Incomplete,
            ["failed", "unknown", "incomplete"],
        ),
        (
            ExecutionState::NotAttempted,
            WriteState::NotAttempted,
            VerificationState::NotApplicable,
            ["not_attempted", "not_attempted", "not_applicable"],
        ),
        (
            ExecutionState::Skipped,
            WriteState::NotApplicable,
            VerificationState::NotRequested,
            ["skipped", "not_applicable", "not_requested"],
        ),
        (
            ExecutionState::Failed,
            WriteState::Failed,
            VerificationState::Mismatch,
            ["failed", "failed", "mismatch"],
        ),
    ];
    for (execution, write, verification, _) in rows {
        let mut item = Item::read(0, Some("boot".into()), 32768, 512);
        item.execution = execution;
        item.write = write;
        item.verification = verification;
        item.ranges.push(Range {
            start_byte: Some("32768".into()),
            length_bytes: Some("512".into()),
            verification,
            reason: None,
        });
        report.items.push(item);
    }
    report.summarize();
    let mut bytes = Vec::new();
    OutputWriter::new(
        OutputMode {
            json: true,
            quiet: true,
        },
        &mut bytes,
    )
    .finish(&report)
    .unwrap();
    let result: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(result["schema_version"], 1);
    for (index, (_, _, _, expected)) in rows.into_iter().enumerate() {
        let item = &result["items"][index];
        assert_eq!(item["id"], (index + 1).to_string());
        assert_eq!(item["execution"], expected[0]);
        assert_eq!(item["write"], expected[1]);
        assert_eq!(item["verification"], expected[2]);
        assert_eq!(item["ranges"][0]["start_byte"], "32768");
        assert_eq!(item["ranges"][0]["verification"], expected[2]);
    }
}

#[cfg(test)]
#[test]
fn terminal_writer_maps_each_core_outcome_to_its_cli_exit_code() {
    for (status, code) in [
        (Status::Success, 0),
        (Status::InputError, 2),
        (Status::Unsupported, 3),
        (Status::ExecutionFailed, 4),
        (Status::VerificationIncomplete, 5),
        (Status::UnknownWrite, 6),
        (Status::InputChanged, 7),
        (Status::Stopped, 130),
    ] {
        let mut result = OperationResult::new(Some("backup"), None);
        result.status = status;
        assert_eq!(
            OutputWriter::new(OutputMode::default(), Vec::new())
                .finish(&result)
                .unwrap(),
            code
        );
    }
}

#[cfg(test)]
#[test]
fn terminal_keeps_warnings_and_owned_partials_in_every_output_mode() {
    use oplus_edl_core::result::RetainedOutput;
    let mut result = OperationResult::new(Some("backup"), None);
    result.outputs.push(RetainedOutput {
        path: "primary.bin.partial".into(),
        kind: "gpt",
        state: OutputState::Partial,
    });
    result
        .warnings
        .push(Error::new(Status::InputError, "fixture warning"));
    for json in [false, true] {
        for quiet in [false, true] {
            let mut output = Vec::new();
            OutputWriter::new(OutputMode { json, quiet }, &mut output)
                .finish(&result)
                .unwrap();
            if json {
                let record: Value = serde_json::from_slice(&output).unwrap();
                assert_eq!(record["warnings"][0]["message"], "fixture warning");
                assert_eq!(
                    record["outputs"][0]["path"]["display"],
                    "primary.bin.partial"
                );
                assert_eq!(record["outputs"][0]["state"], "partial");
            } else {
                let output = String::from_utf8(output).unwrap();
                assert!(output.contains("Warning InputError: fixture warning"));
                assert!(output.contains("Output partial (gpt): \"primary.bin.partial\""));
            }
        }
    }
}
