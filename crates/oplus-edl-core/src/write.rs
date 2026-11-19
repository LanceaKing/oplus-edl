//! Prepared writes and bounded readback, with no retries or implicit final actions.
use crate::result::{ExecutionState, VerificationState, WriteState};
use crate::{
    input::{SourceFile, Sources},
    model::{PartitionTargets, Verify},
    program_xml::{Candidate, Operation},
    restore::Mapping,
    result::{Error, InputValidity, Item, Range, Report, Status},
    service::Job,
    session::{check_dispatch, check_start, response_wait_expired},
    sparse::Data,
};
use anyhow::{Context, Result, bail};
use qdl::types::{MutationConfirmation, MutationFailure, QdlChan, ReadRejected};
use std::io::{self, Read, Seek, SeekFrom, Write};

struct Payload {
    file: Option<SourceFile>,
    offset: u64,
    available: u64,
    fill: [u8; 4],
}
impl Payload {
    fn reader(&mut self, offset: u64, length: u64) -> io::Result<PayloadReader<'_>> {
        if let Some(file) = &mut self.file {
            file.seek(SeekFrom::Start(self.offset + offset.min(self.available)))?;
        }
        Ok(PayloadReader {
            payload: self,
            pos: offset,
            remaining: length,
        })
    }
}
struct PayloadReader<'a> {
    payload: &'a mut Payload,
    pos: u64,
    remaining: u64,
}
impl Read for PayloadReader<'_> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let n = out.len().min(self.remaining as usize);
        if n == 0 {
            return Ok(0);
        }
        if let Some(file) = &mut self.payload.file {
            let bytes = n.min(self.payload.available.saturating_sub(self.pos) as usize);
            file.read_exact(&mut out[..bytes])?;
            out[bytes..n].fill(0);
        } else {
            for (i, b) in out[..n].iter_mut().enumerate() {
                *b = self.payload.fill[((self.pos + i as u64) % 4) as usize];
            }
        }
        self.pos += n as u64;
        self.remaining -= n as u64;
        Ok(n)
    }
}
struct Program {
    start: String,
    bytes: u64,
    payload: Payload,
}
enum Action {
    Program(Program),
    Patch {
        start: String,
        offset: u64,
        size: u64,
        value: String,
    },
    Skip,
    Boot(u8),
    Reboot,
}
struct Step {
    item: Item,
    action: Action,
}
/// Prepared inputs stay open for both transmission and comparison. No persistent plan is created.
pub struct Writes {
    steps: Vec<Step>,
    restore: bool,
    wire_mode: crate::firehose::WireMode,
    sector: u64,
    sources: Sources,
}
fn numeric(s: &str) -> Option<u64> {
    s.strip_suffix('.').unwrap_or(s).parse().ok()
}
fn item(operation: &str, lun: Option<u8>, start: Option<u64>, bytes: Option<u64>) -> Item {
    let mut i = Item::read(lun.unwrap_or(0), None, 0, 0);
    i.operation = operation.into();
    i.lun = lun;
    i.start_byte = start.map(|n| n.to_string());
    i.partition_bytes = None;
    i.transfer_bytes = bytes.map(|n| n.to_string());
    i.transferred_bytes = bytes.map(|_| "0".into());
    i.write = WriteState::NotAttempted;
    i
}
fn file_payload(
    path: &std::path::Path,
    offset: u64,
    available: u64,
    sources: &mut Sources,
) -> Result<Payload> {
    let file = sources.open(path)?;
    if file.metadata()?.len() < offset.checked_add(available).context("source overflow")? {
        bail!("source truncated during preflight");
    }
    Ok(Payload {
        file: Some(file),
        offset,
        available,
        fill: [0; 4],
    })
}
impl Writes {
    pub fn set_wire_mode(&mut self, mode: crate::firehose::WireMode) {
        self.wire_mode = mode;
    }
    pub fn restore(mappings: Vec<Mapping>, sector: u64) -> Result<Self> {
        Self::restore_bound(mappings, sector, Sources::default())
    }

    fn restore_bound(mappings: Vec<Mapping>, sector: u64, mut sources: Sources) -> Result<Self> {
        let mut steps = vec![];
        for m in mappings {
            if m.target.sector_size as u64 != sector {
                bail!("sector size conflict");
            }
            let mut i = item(
                "program",
                Some(m.target.lun),
                Some(m.target.start_byte()?),
                Some(m.transfer_bytes),
            );
            i.slot = Some(0);
            i.partition_bytes = Some(m.target.bytes()?.to_string());
            i.partition = Some(m.target.name);
            i.source = Some(m.source.clone());
            i.source_bytes = Some(m.source_bytes.to_string());
            i.reason = Some(format!(
                "uncovered partition tail: {} bytes",
                m.uncovered_bytes
            ));
            steps.push(Step {
                item: i,
                action: Action::Program(Program {
                    start: m.target.start_sector.to_string(),
                    bytes: m.transfer_bytes,
                    payload: file_payload(&m.source, 0, m.source_bytes, &mut sources)?,
                }),
            });
        }
        Ok(Self {
            wire_mode: crate::firehose::WireMode::Direct,
            steps,
            restore: true,
            sector,
            sources,
        })
    }
    pub fn flash(
        candidates: Vec<Candidate>,
        sector: u64,
        boot: Option<u8>,
        reboot: bool,
    ) -> Result<Self> {
        Self::flash_bound(candidates, sector, boot, reboot, Sources::default())
    }

    pub(crate) fn flash_bound(
        candidates: Vec<Candidate>,
        sector: u64,
        boot: Option<u8>,
        reboot: bool,
        mut sources: Sources,
    ) -> Result<Self> {
        let mut steps = vec![];
        for c in candidates {
            if c.sector_size != sector {
                bail!("sector size conflict");
            }
            let base = numeric(&c.start_sector).and_then(|n| n.checked_mul(sector));
            let mut i = item("program", Some(c.lun), base, None);
            i.slot = Some(c.slot);
            if base.is_none() {
                i.reason = Some(format!("unknown start address: {}", c.start_sector));
            }
            i.source = Some(c.xml);
            i.xml_node = Some(c.xml_node);
            i.partition = c.label;
            match c.operation {
                Operation::Program {
                    source,
                    source_bytes,
                    source_offset,
                    transfer_bytes,
                    sparse,
                    ..
                } => {
                    i.source = Some(source.clone());
                    i.source_bytes = Some(source_bytes.to_string());
                    let chunks = sparse.map(|s| s.chunks).unwrap_or_else(|| {
                        vec![crate::sparse::Chunk {
                            offset: 0,
                            bytes: transfer_bytes,
                            data: Data::Raw {
                                file_offset: source_offset,
                            },
                        }]
                    });
                    for chunk in chunks {
                        if chunk.data == Data::DontCare {
                            continue;
                        }
                        let start = shifted_start(&c.start_sector, chunk.offset / sector)?;
                        let payload = match chunk.data {
                            Data::Raw { file_offset } => file_payload(
                                &source,
                                file_offset,
                                chunk.bytes.min(source_bytes.saturating_sub(file_offset)),
                                &mut sources,
                            )?,
                            Data::Fill(fill) => Payload {
                                file: None,
                                offset: 0,
                                available: 0,
                                fill,
                            },
                            Data::DontCare => unreachable!(),
                        };
                        let mut ci = i.clone();
                        ci.start_byte = base.map(|n| (n + chunk.offset).to_string());
                        ci.transfer_bytes = Some(chunk.bytes.to_string());
                        ci.transferred_bytes = Some("0".into());
                        steps.push(Step {
                            item: ci,
                            action: Action::Program(Program {
                                start,
                                bytes: chunk.bytes,
                                payload,
                            }),
                        });
                    }
                }
                Operation::Patch {
                    byte_offset,
                    size,
                    value,
                } => {
                    i.operation = "patch".into();
                    i.start_byte = base.map(|n| (n + byte_offset).to_string());
                    i.transfer_bytes = Some(size.to_string());
                    steps.push(Step {
                        item: i,
                        action: Action::Patch {
                            start: c.start_sector,
                            offset: byte_offset,
                            size,
                            value,
                        },
                    });
                }
                Operation::Skip { reason } => {
                    if reason.starts_with("local file patch") {
                        i.operation = "patch".into();
                    }
                    i.execution = ExecutionState::Skipped;
                    i.write = WriteState::NotApplicable;
                    i.reason = Some(reason);
                    steps.push(Step {
                        item: i,
                        action: Action::Skip,
                    });
                }
            }
        }
        if let Some(lun) = boot {
            steps.push(Step {
                item: item("set_bootable", Some(lun), None, None),
                action: Action::Boot(lun),
            });
        }
        if reboot {
            steps.push(Step {
                item: item("reboot", None, None, None),
                action: Action::Reboot,
            });
        }
        Ok(Self {
            wire_mode: crate::firehose::WireMode::Direct,
            steps,
            restore: false,
            sector,
            sources,
        })
    }
    pub(crate) fn check_sources(&self) -> Result<()> {
        self.sources.check()
    }
    pub fn luns(&self) -> Vec<u8> {
        let mut luns: Vec<_> = self
            .steps
            .iter()
            .filter(|s| !matches!(s.action, Action::Skip))
            .filter_map(|s| s.item.lun)
            .collect();
        luns.sort_unstable();
        luns.dedup();
        luns
    }
    pub fn preflight(&self, verify: Verify) -> Result<()> {
        if !self
            .steps
            .iter()
            .any(|s| matches!(s.action, Action::Program(_) | Action::Patch { .. }))
        {
            bail!("no executable storage operations");
        }
        for s in &self.steps {
            if let Action::Program(p) = &s.action {
                if self.sector == 0 || p.bytes == 0 || !p.bytes.is_multiple_of(self.sector) {
                    bail!("invalid transfer geometry");
                }
                usize::try_from(p.bytes).context("transfer exceeds host address size")?;
            }
            if matches!(verify, Verify::Required)
                && (matches!(s.action, Action::Patch { .. })
                    || matches!(s.action, Action::Program(_)) && s.item.start_byte.is_none())
            {
                bail!("required verification cannot determine expected content or address");
            }
        }
        Ok(())
    }
    pub fn reject(mut self, report: &mut Report, category: Status, reason: &str) {
        for s in &mut self.steps {
            if s.item.execution != ExecutionState::Skipped {
                s.item.reason = Some(reason.into());
            }
        }
        report.items.extend(self.steps.into_iter().map(|s| s.item));
        report.error(category, reason);
    }
    pub(crate) fn prepared_items(&self) -> Vec<Item> {
        self.steps.iter().map(|step| step.item.clone()).collect()
    }

    pub fn execute(self, channel: &mut impl QdlChan, verify: Verify, report: &mut Report) {
        self.execute_controlled(channel, verify, None, report);
    }

    pub(crate) fn execute_controlled(
        mut self,
        channel: &mut impl QdlChan,
        verify: Verify,
        job: Option<&Job>,
        report: &mut Report,
    ) {
        if let Err(e) = self.preflight(verify) {
            self.reject(report, Status::Unsupported, &e.to_string());
            return;
        }
        if channel.fh_config().storage_sector_size as u64 != self.sector {
            self.reject(report, Status::InputError, "sector size conflict");
            return;
        }
        let finals = self
            .steps
            .iter()
            .position(|s| matches!(s.action, Action::Boot(_) | Action::Reboot))
            .unwrap_or(self.steps.len());
        let execution = (|| -> Result<(), ExecutionHalt> {
            for index in 0..finals {
                check_execution_stop(job)?;
                check_execution_sources(&self.sources, report)?;
                let completed = mutate(
                    channel,
                    &mut self.steps[index],
                    self.sector,
                    self.wire_mode,
                    job,
                    report,
                );
                check_execution_sources(&self.sources, report)?;
                completed?;
                check_execution_stop(job)?;
                if self.restore {
                    verify_step(
                        channel,
                        &mut self.steps,
                        index,
                        index + 1,
                        self.sector,
                        verify,
                        self.wire_mode,
                        job,
                        &self.sources,
                        report,
                    )?;
                }
            }
            if !self.restore {
                for index in 0..finals {
                    verify_step(
                        channel,
                        &mut self.steps,
                        index,
                        finals,
                        self.sector,
                        verify,
                        self.wire_mode,
                        job,
                        &self.sources,
                        report,
                    )?;
                }
            }
            for index in finals..self.steps.len() {
                check_execution_stop(job)?;
                check_execution_sources(&self.sources, report)?;
                mutate(
                    channel,
                    &mut self.steps[index],
                    self.sector,
                    self.wire_mode,
                    job,
                    report,
                )?;
            }
            check_execution_sources(&self.sources, report)
        })();
        if let Err(halt) = execution {
            let reason = match halt {
                ExecutionHalt::Stopped => "stop requested",
                ExecutionHalt::Failed => "stopped after prior operation failure",
            };
            for index in 0..self.steps.len() {
                if self.steps[index].item.execution == ExecutionState::NotAttempted {
                    self.steps[index].item.reason = Some(reason.into());
                }
                let s = &self.steps[index];
                if s.item.write == WriteState::Acknowledged
                    && matches!(s.action, Action::Program(_) | Action::Patch { .. })
                    && s.item.verification == VerificationState::NotApplicable
                    && s.item.reason.as_deref() != Some("superseded")
                {
                    let (base, spans) = effective_spans(&self.steps, index, finals);
                    let s = &mut self.steps[index];
                    if base.is_some() && spans.is_empty() {
                        s.item.reason = Some("superseded".into());
                        continue;
                    }
                    let state = if matches!(verify, Verify::None) {
                        VerificationState::NotRequested
                    } else {
                        VerificationState::Incomplete
                    };
                    s.item.verification = state;
                    s.item.reason = Some("verification not reached after stop".into());
                    s.item.ranges = if base.is_none() {
                        vec![Range {
                            start_byte: None,
                            length_bytes: None,
                            verification: state,
                            reason: s.item.reason.clone(),
                        }]
                    } else {
                        spans
                            .into_iter()
                            .map(|span| Range {
                                start_byte: Some(span.start.to_string()),
                                length_bytes: Some((span.end - span.start).to_string()),
                                verification: state,
                                reason: s.item.reason.clone(),
                            })
                            .collect()
                    };
                    if !matches!(verify, Verify::None) {
                        s.item.errors.push(Error::new(
                            Status::VerificationIncomplete,
                            "write acknowledged; verification not reached",
                        ));
                    }
                }
            }
        }
        report.items.extend(self.steps.into_iter().map(|s| s.item));
        report.summarize();
    }
}

#[derive(Clone, Copy)]
enum ExecutionHalt {
    Stopped,
    Failed,
}

fn check_execution_stop(job: Option<&Job>) -> Result<(), ExecutionHalt> {
    check_start(job).map_err(|_| ExecutionHalt::Stopped)
}

fn check_execution_sources(sources: &Sources, report: &mut Report) -> Result<(), ExecutionHalt> {
    if sources_current(sources, report) {
        Ok(())
    } else {
        Err(ExecutionHalt::Failed)
    }
}

fn sources_current(sources: &Sources, report: &mut Report) -> bool {
    match sources.check() {
        Ok(()) => {
            report.input_validity = InputValidity::Valid;
            true
        }
        Err(error) => {
            report.error_source(Status::InputChanged, &error);
            false
        }
    }
}

fn mutate(
    channel: &mut impl QdlChan,
    s: &mut Step,
    sector: u64,
    mode: crate::firehose::WireMode,
    job: Option<&Job>,
    report: &mut Report,
) -> Result<(), ExecutionHalt> {
    if let Some(job) = job {
        match s.action {
            Action::Patch { .. } => job.progress("write", None),
            Action::Boot(_) | Action::Reboot => job.progress("finalization", None),
            _ => {}
        }
    }
    check_dispatch(job, &s.item.operation).map_err(|_| ExecutionHalt::Stopped)?;
    let result = match &mut s.action {
        Action::Skip => return Ok(()),
        Action::Program(p) => {
            if let Some(job) = job {
                job.progress("write", Some((0, p.bytes)));
            }
            let reader = p
                .payload
                .reader(0, p.bytes)
                .map_err(MutationFailure::not_sent);
            match reader {
                Ok(reader) => {
                    check_execution_stop(job)?;
                    let raw = std::cell::Cell::new(false);
                    let mut counted = CountChannel {
                        inner: channel,
                        raw: &raw,
                        accepted: 0,
                        count_unknown: false,
                        progress: job.map(|job| (job, p.bytes)),
                    };
                    let mut reader = StartRaw {
                        inner: reader,
                        raw: &raw,
                    };
                    let result = crate::firehose::program_with_mode(
                        &mut counted,
                        &mut reader,
                        s.item.partition.as_deref().unwrap_or("image"),
                        (p.bytes / sector) as usize,
                        s.item.slot.expect("program slot"),
                        s.item.lun.unwrap(),
                        &p.start,
                        mode,
                    );
                    s.item.transferred_bytes =
                        (!counted.count_unknown).then(|| counted.accepted.to_string());
                    result
                }
                Err(e) => Err(e),
            }
        }
        Action::Patch {
            start,
            offset,
            size,
            value,
        } => crate::firehose::patch(
            channel,
            *offset,
            s.item.slot.expect("patch slot"),
            s.item.lun.unwrap(),
            *size,
            start,
            value,
        ),
        Action::Boot(lun) => crate::firehose::set_bootable(channel, *lun),
        Action::Reboot => crate::firehose::reset(channel),
    };
    match result {
        Ok(()) => {
            if matches!(s.action, Action::Reboot) {
                report.unusable = true;
                report.session.reason = Some("rebooted");
            }
            s.item.execution = ExecutionState::Done;
            s.item.write = WriteState::Acknowledged;
            s.item.transferred_bytes = if matches!(s.action, Action::Program(_)) {
                s.item.transfer_bytes.clone()
            } else {
                None
            };
            Ok(())
        }
        Err(e) => {
            report.unusable |= e.confirmation != MutationConfirmation::NotSent;
            s.item.mutation_failure(&e);
            Err(ExecutionHalt::Failed)
        }
    }
}
#[derive(Clone)]
struct Span {
    start: u64,
    end: u64,
    unknown: bool,
}
fn overlay(spans: Vec<Span>, start: u64, end: u64, unknown: bool) -> Vec<Span> {
    let mut result = vec![];
    for s in spans {
        let a = start.max(s.start);
        let b = end.min(s.end);
        if a >= b {
            result.push(s);
            continue;
        }
        if s.start < a {
            result.push(Span {
                start: s.start,
                end: a,
                unknown: s.unknown,
            });
        }
        if unknown {
            result.push(Span {
                start: a,
                end: b,
                unknown: true,
            });
        }
        if b < s.end {
            result.push(Span {
                start: b,
                end: s.end,
                unknown: s.unknown,
            });
        }
    }
    result
}
fn effective_spans(steps: &[Step], index: usize, limit: usize) -> (Option<u64>, Vec<Span>) {
    let base = steps[index]
        .item
        .start_byte
        .as_ref()
        .and_then(|s| s.parse::<u64>().ok());
    let length = steps[index]
        .item
        .transfer_bytes
        .as_ref()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);
    let patch = matches!(steps[index].action, Action::Patch { .. });
    let mut spans = base
        .map(|start| {
            vec![Span {
                start,
                end: start + length,
                unknown: patch,
            }]
        })
        .unwrap_or_default();
    for later in &steps[index + 1..limit] {
        if later.item.lun != steps[index].item.lun
            || later.item.slot != steps[index].item.slot
            || matches!(
                later.item.write,
                WriteState::NotAttempted | WriteState::NotApplicable
            )
        {
            continue;
        }
        let unknown = matches!(later.action, Action::Patch { .. })
            || later.item.start_byte.is_none()
            || later.item.write != WriteState::Acknowledged;
        if !matches!(later.action, Action::Program(_) | Action::Patch { .. }) {
            continue;
        }
        if let Some(start) = later
            .item
            .start_byte
            .as_ref()
            .and_then(|s| s.parse::<u64>().ok())
        {
            let bytes = later
                .item
                .transfer_bytes
                .as_ref()
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);
            spans = overlay(spans, start, start + bytes, unknown);
        } else {
            for span in &mut spans {
                span.unknown = true;
            }
        }
    }
    (base, spans)
}
#[allow(clippy::too_many_arguments)]
fn verify_step(
    channel: &mut impl QdlChan,
    steps: &mut [Step],
    index: usize,
    limit: usize,
    sector: u64,
    mode: Verify,
    wire_mode: crate::firehose::WireMode,
    job: Option<&Job>,
    sources: &Sources,
    report: &mut Report,
) -> Result<(), ExecutionHalt> {
    if steps[index].item.write != WriteState::Acknowledged
        || !matches!(
            steps[index].action,
            Action::Program(_) | Action::Patch { .. }
        )
    {
        return Ok(());
    }
    let (base, spans) = effective_spans(steps, index, limit);
    let s = &mut steps[index];
    if base.is_some() && spans.is_empty() {
        s.item.reason = Some("superseded".into());
        return Ok(());
    }
    if matches!(mode, Verify::None) {
        s.item.verification = VerificationState::NotRequested;
        if base.is_none() {
            s.item.ranges.push(Range {
                start_byte: None,
                length_bytes: None,
                verification: VerificationState::NotRequested,
                reason: s.item.reason.clone(),
            });
        } else {
            s.item.ranges.extend(spans.into_iter().map(|span| Range {
                start_byte: Some(span.start.to_string()),
                length_bytes: Some((span.end - span.start).to_string()),
                verification: VerificationState::NotRequested,
                reason: None,
            }));
        }
        return Ok(());
    }
    if let Some(job) = job {
        job.progress("verification", None);
    }
    let mut halt = None;
    if base.is_none() {
        s.item.ranges.push(Range {
            start_byte: None,
            length_bytes: None,
            verification: VerificationState::Incomplete,
            reason: Some("unknown address".into()),
        });
    }
    for span in spans {
        let mut range = Range {
            start_byte: Some(span.start.to_string()),
            length_bytes: Some((span.end - span.start).to_string()),
            verification: VerificationState::Incomplete,
            reason: None,
        };
        if span.unknown {
            range.reason = Some("unknown expected content after patch or expression write".into());
        } else if halt.is_some() || check_start(job).is_err() {
            range.reason = Some("verification stopped".into());
            halt.get_or_insert(ExecutionHalt::Stopped);
        } else if !sources_current(sources, report) {
            range.reason = Some("verification source changed".into());
            halt = Some(ExecutionHalt::Failed);
        } else if let Action::Program(p) = &mut s.action {
            let first = span.start / sector * sector;
            let end = span.end.div_ceil(sector) * sector;
            match p
                .payload
                .reader(span.start - base.unwrap(), span.end - span.start)
            {
                Ok(expected) => {
                    let mut compare = Compare {
                        expected,
                        skip: span.start - first,
                        remaining: span.end - span.start,
                        mismatch: false,
                    };
                    if check_start(job).is_err() {
                        range.reason = Some("verification stopped".into());
                        halt = Some(ExecutionHalt::Stopped);
                        s.item.ranges.push(range);
                        continue;
                    }
                    let result = crate::firehose::read_storage_with_mode(
                        channel,
                        &mut compare,
                        ((end - first) / sector) as usize,
                        s.item.slot.expect("program slot"),
                        s.item.lun.unwrap(),
                        first / sector,
                        wire_mode,
                    );
                    let mismatch = compare.mismatch;
                    let stable = sources_current(sources, report);
                    match result {
                        Ok(()) if !stable => {
                            range.reason = Some("verification source changed".into());
                            halt = Some(ExecutionHalt::Failed);
                        }
                        Ok(()) if !mismatch => range.verification = VerificationState::Matched,
                        Ok(()) => {
                            range.verification = VerificationState::Mismatch;
                            halt = Some(ExecutionHalt::Failed);
                        }
                        Err(e) => {
                            let refused = e.is::<ReadRejected>();
                            report.unusable |= !refused;
                            let expired = response_wait_expired(&e);
                            let reason = Error::from_source(Status::VerificationIncomplete, &e);
                            if expired || reason.io_kind.is_some() {
                                s.item.errors.push(reason);
                            }
                            range.reason = Some(if expired && mismatch {
                                "byte_difference_observed".into()
                            } else if refused {
                                "read_refused".into()
                            } else {
                                format!("read failed: {e:#}")
                            });
                            if !(stable && refused && matches!(mode, Verify::BestEffort)) {
                                halt = Some(ExecutionHalt::Failed);
                            }
                        }
                    }
                }
                Err(e) => {
                    range.reason = Some(format!("expected input: {e}"));
                    halt = Some(ExecutionHalt::Failed);
                }
            }
        }
        s.item.ranges.push(range);
    }
    let state = if s
        .item
        .ranges
        .iter()
        .any(|r| r.verification == VerificationState::Mismatch)
    {
        VerificationState::Mismatch
    } else if s
        .item
        .ranges
        .iter()
        .any(|r| r.verification == VerificationState::Incomplete)
    {
        VerificationState::Incomplete
    } else {
        VerificationState::Matched
    };
    s.item.verification = state;
    if state != VerificationState::Matched {
        let (category, description) = if state == VerificationState::Mismatch {
            (Status::ExecutionFailed, "mismatch")
        } else {
            (Status::VerificationIncomplete, "incomplete")
        };
        if !s.item.errors.iter().any(|error| error.category == category) {
            let mut error = Error::new(
                category,
                format!("write acknowledged; verification {description}"),
            );
            if state == VerificationState::Mismatch {
                error.code = Some("verification_mismatch");
            }
            s.item.errors.push(error);
        }
    }
    if state == VerificationState::Incomplete && matches!(mode, Verify::Required) {
        halt.get_or_insert(ExecutionHalt::Failed);
    }
    halt.map_or(Ok(()), Err)
}
struct Compare<R> {
    expected: R,
    skip: u64,
    remaining: u64,
    mismatch: bool,
}
impl<R: Read> Write for Compare<R> {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        let skip = data.len().min(self.skip as usize);
        self.skip -= skip as u64;
        let bytes = (data.len() - skip).min(self.remaining as usize);
        let mut offset = 0;
        let mut buf = [0; 8192];
        while offset < bytes {
            let n = buf.len().min(bytes - offset);
            self.expected.read_exact(&mut buf[..n])?;
            self.mismatch |= buf[..n] != data[skip + offset..skip + offset + n];
            offset += n;
        }
        self.remaining -= bytes as u64;
        Ok(data.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Read every selected GPT before resolving any source-to-target mapping.
pub fn prepare_restore(
    channel: &mut impl QdlChan,
    luns: &[u8],
    from: Option<&std::path::Path>,
    image: Option<&std::path::Path>,
    names: &[String],
    selected_lun: Option<u8>,
    report: &mut Report,
) -> Result<Writes> {
    let partitions = PartitionTargets::from_names(selected_lun, names);
    let mapping_luns = selected_lun.as_ref().map(std::slice::from_ref);
    prepare_restore_controlled(
        channel,
        luns,
        mapping_luns,
        (from, image),
        &partitions,
        None,
        report,
    )
}

pub(crate) fn prepare_restore_controlled(
    channel: &mut impl QdlChan,
    scan_luns: &[u8],
    mapping_luns: Option<&[u8]>,
    source: (Option<&std::path::Path>, Option<&std::path::Path>),
    partitions: &PartitionTargets,
    job: Option<&Job>,
    report: &mut Report,
) -> Result<Writes> {
    let mut targets = vec![];
    for &lun in scan_luns {
        let table = crate::storage::read_gpt_recorded(
            channel,
            lun,
            crate::firehose::WireMode::Direct,
            job,
            report,
        )?;
        if let Some(job) = job {
            job.check_gpt(lun, &table)?;
        }
        targets.extend(
            crate::storage::partitions(&table, lun)?
                .into_iter()
                .filter(|p| partitions.includes(p.lun, &p.name)),
        );
    }
    check_start(job)?;
    report.coverage.limited = true;
    report.coverage.reason = Some(format!("selected GPT partitions in LUNs {scan_luns:?}"));
    let mut sources = Sources::default();
    let mappings = match source {
        (Some(dir), None) => {
            crate::restore::directory_bound(dir, mapping_luns, partitions, &targets, &mut sources)?
        }
        (None, Some(image)) if let Some(target) = partitions.single_target() => {
            vec![crate::restore::single_bound(
                image,
                &target.name,
                target
                    .lun
                    .or_else(|| (scan_luns.len() == 1).then(|| scan_luns[0])),
                &targets,
                &mut sources,
            )?]
        }
        _ => bail!("invalid restore source selection"),
    };
    sources.check()?;
    Writes::restore_bound(
        mappings,
        channel.fh_config().storage_sector_size as u64,
        sources,
    )
}

// Count only raw bytes accepted by the transport, never XML or input prefetch.
struct StartRaw<'a, R> {
    inner: R,
    raw: &'a std::cell::Cell<bool>,
}
impl<R: Read> Read for StartRaw<'_, R> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        self.raw.set(true);
        self.inner.read(out)
    }
}
struct CountChannel<'a, T> {
    inner: &'a mut T,
    raw: &'a std::cell::Cell<bool>,
    accepted: u64,
    count_unknown: bool,
    progress: Option<(&'a Job, u64)>,
}
impl<T: QdlChan> Read for CountChannel<'_, T> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        self.inner.read(out)
    }
}
impl<T: QdlChan> io::BufRead for CountChannel<'_, T> {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        self.inner.fill_buf()
    }
    fn consume(&mut self, n: usize) {
        self.inner.consume(n);
    }
}
impl<T: QdlChan> Write for CountChannel<'_, T> {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        let n = match self.inner.write(data) {
            Ok(n) => n,
            Err(error) => {
                // The USB adapter can report an error after accepting bytes,
                // but its error does not retain that completion length.
                if self.raw.get()
                    && !data.is_empty()
                    && self.inner.fh_config().backend == qdl::types::QdlBackend::Usb
                {
                    self.count_unknown = true;
                }
                return Err(error);
            }
        };
        if self.raw.get() {
            self.accepted += n as u64;
            if let Some((job, total)) = self.progress {
                job.progress("write", Some((self.accepted, total)));
            }
        }
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}
impl<T: QdlChan> QdlChan for CountChannel<'_, T> {
    fn fh_config(&self) -> &qdl::types::FirehoseConfiguration {
        self.inner.fh_config()
    }
    fn mut_fh_config(&mut self) -> &mut qdl::types::FirehoseConfiguration {
        self.inner.mut_fh_config()
    }
    fn fill_response_buf(&mut self) -> io::Result<&[u8]> {
        self.inner.fill_response_buf()
    }
    fn check_response_wait(&self) -> Result<()> {
        self.inner.check_response_wait()
    }
}

// Shift only the constant part of the already accepted protocol expression.
fn shifted_start(start: &str, sectors: u64) -> Result<String> {
    if sectors == 0 {
        return Ok(start.into());
    }
    if let Some(n) = numeric(start) {
        return Ok(n
            .checked_add(sectors)
            .context("start overflow")?
            .to_string());
    }
    let expression = start.strip_suffix('.').unwrap_or(start);
    let suffix = expression
        .strip_prefix("NUM_DISK_SECTORS")
        .context("unsupported start expression")?;
    let constant = if suffix.is_empty() {
        0i128
    } else {
        suffix.parse::<i128>()?
    };
    let shifted = constant + i128::from(sectors);
    u64::try_from(shifted.abs()).context("start expression offset overflow")?;
    Ok(if shifted == 0 {
        "NUM_DISK_SECTORS.".into()
    } else {
        format!("NUM_DISK_SECTORS{shifted:+}.")
    })
}
