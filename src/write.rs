//! Prepared writes and bounded readback, with no retries or implicit final actions.
use crate::{
    cli::Verify,
    output::{Error, Item, Range, Report, Status},
    program_xml::{Candidate, Operation},
    restore::Mapping,
    sparse::Data,
};
use anyhow::{Context, Result, bail};
use qdl::types::{MutationFailure, QdlChan, ReadRejected};
use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom, Write},
};

struct Payload {
    file: Option<File>,
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
    slot: u8,
    action: Action,
}
/// Prepared inputs stay open for both transmission and comparison. No persistent plan is created.
pub struct Writes {
    steps: Vec<Step>,
    restore: bool,
    wire_mode: crate::firehose::WireMode,
    sector: u64,
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
    i.write = "not_attempted".into();
    i
}
fn file_payload(path: &std::path::Path, offset: u64, available: u64) -> Result<Payload> {
    let file = crate::input::open_regular(path)?;
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
            i.partition_bytes = Some(m.target.bytes()?.to_string());
            i.partition = Some(m.target.name);
            i.source = Some(m.source.display().to_string());
            i.source_bytes = Some(m.source_bytes.to_string());
            i.reason = Some(format!(
                "uncovered partition tail: {} bytes",
                m.uncovered_bytes
            ));
            steps.push(Step {
                item: i,
                slot: 0,
                action: Action::Program(Program {
                    start: m.target.start_sector.to_string(),
                    bytes: m.transfer_bytes,
                    payload: file_payload(&m.source, 0, m.source_bytes)?,
                }),
            });
        }
        Ok(Self {
            wire_mode: crate::firehose::WireMode::Direct,
            steps,
            restore: true,
            sector,
        })
    }
    pub fn flash(
        candidates: Vec<Candidate>,
        sector: u64,
        boot: Option<u8>,
        reboot: bool,
    ) -> Result<Self> {
        let mut steps = vec![];
        for c in candidates {
            if c.sector_size != sector {
                bail!("sector size conflict");
            }
            let base = numeric(&c.start_sector).and_then(|n| n.checked_mul(sector));
            let mut i = item("program", Some(c.lun), base, None);
            if base.is_none() {
                i.reason = Some(format!("unknown start address: {}", c.start_sector));
            }
            i.source = Some(c.xml.display().to_string());
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
                    i.source = Some(source.display().to_string());
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
                            slot: c.slot,
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
                        slot: c.slot,
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
                    i.execution = "skipped".into();
                    i.write = "not_applicable".into();
                    i.reason = Some(reason);
                    steps.push(Step {
                        item: i,
                        slot: c.slot,
                        action: Action::Skip,
                    });
                }
            }
        }
        if let Some(lun) = boot {
            steps.push(Step {
                item: item("set_bootable", Some(lun), None, None),
                slot: 0,
                action: Action::Boot(lun),
            });
        }
        if reboot {
            steps.push(Step {
                item: item("reboot", None, None, None),
                slot: 0,
                action: Action::Reboot,
            });
        }
        Ok(Self {
            wire_mode: crate::firehose::WireMode::Direct,
            steps,
            restore: false,
            sector,
        })
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
    pub fn summary(&self, out: &mut impl Write, verify: Verify) -> io::Result<()> {
        writeln!(out, "Verification: {}", verify.as_str())?;
        for s in &self.steps {
            writeln!(
                out,
                "{} LUN {:?} slot {} source {:?} node {:?} start {:?} transfer bytes {:?} source bytes {:?} partition bytes {:?}: {}",
                s.item.operation,
                s.item.lun,
                s.slot,
                s.item.source,
                s.item.xml_node,
                s.item.start_byte,
                s.item.transfer_bytes,
                s.item.source_bytes,
                s.item.partition_bytes,
                s.item.reason.as_deref().unwrap_or("")
            )?;
            if matches!(s.action, Action::Patch { .. })
                || matches!(s.action, Action::Program(_)) && s.item.start_byte.is_none()
            {
                writeln!(
                    out,
                    "  expected content/address unknown; verification incomplete unless --verify none"
                )?;
            }
        }
        Ok(())
    }
    pub fn reject(mut self, report: &mut Report, category: Status, reason: &str) {
        for s in &mut self.steps {
            if s.item.execution != "skipped" {
                s.item.reason = Some(reason.into());
            }
        }
        report.items.extend(self.steps.into_iter().map(|s| s.item));
        report.error(category, reason);
    }
    /// Shared CLI confirmation path, with injectable streams for offline tests.
    pub fn confirm_and_execute(
        self,
        channel: &mut impl QdlChan,
        verify: Verify,
        yes: bool,
        input: &mut impl io::BufRead,
        output: &mut impl Write,
        report: &mut Report,
    ) {
        if let Err(e) = self.preflight(verify) {
            self.reject(report, Status::Unsupported, &e.to_string());
            return;
        }
        if let Err(e) = self.summary(output, verify) {
            self.reject(report, Status::ExecutionFailed, &e.to_string());
            return;
        }
        if !yes {
            if let Err(e) = writeln!(
                output,
                "Write these ranges to the selected device? Type yes to continue:"
            )
            .and_then(|_| output.flush())
            {
                self.reject(report, Status::ExecutionFailed, &e.to_string());
                return;
            }
            let mut answer = String::new();
            if input.read_line(&mut answer).is_err() || answer.trim() != "yes" {
                self.reject(report, Status::InputError, "write confirmation declined");
                return;
            }
        }
        self.execute(channel, verify, report);
    }
    pub fn execute(mut self, channel: &mut impl QdlChan, verify: Verify, report: &mut Report) {
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
        let mut stopped = false;
        for index in 0..finals {
            if stopped {
                break;
            }
            stopped = !mutate(channel, &mut self.steps[index], self.sector, self.wire_mode);
            if !stopped && self.restore {
                stopped = !verify_step(
                    channel,
                    &mut self.steps,
                    index,
                    index + 1,
                    self.sector,
                    verify,
                    self.wire_mode,
                );
            }
        }
        if !stopped && !self.restore {
            for index in 0..finals {
                if !verify_step(
                    channel,
                    &mut self.steps,
                    index,
                    finals,
                    self.sector,
                    verify,
                    self.wire_mode,
                ) {
                    stopped = true;
                    break;
                }
            }
        }
        if !stopped {
            for index in finals..self.steps.len() {
                if !mutate(channel, &mut self.steps[index], self.sector, self.wire_mode) {
                    stopped = true;
                    break;
                }
            }
        }
        if stopped {
            for index in 0..self.steps.len() {
                if self.steps[index].item.execution == "not_attempted" {
                    self.steps[index].item.reason =
                        Some("stopped after prior operation failure".into());
                }
                let s = &self.steps[index];
                if s.item.write == "acknowledged"
                    && matches!(s.action, Action::Program(_) | Action::Patch { .. })
                    && s.item.verification == "not_applicable"
                    && s.item.reason.as_deref() != Some("superseded")
                {
                    let (base, spans) = effective_spans(&self.steps, index, finals);
                    let s = &mut self.steps[index];
                    if base.is_some() && spans.is_empty() {
                        s.item.reason = Some("superseded".into());
                        continue;
                    }
                    let state = if matches!(verify, Verify::None) {
                        "not_requested"
                    } else {
                        "incomplete"
                    };
                    s.item.verification = state.into();
                    s.item.reason = Some("verification not reached after stop".into());
                    s.item.ranges = if base.is_none() {
                        vec![Range {
                            start_byte: None,
                            length_bytes: None,
                            verification: state.into(),
                            reason: s.item.reason.clone(),
                        }]
                    } else {
                        spans
                            .into_iter()
                            .map(|span| Range {
                                start_byte: Some(span.start.to_string()),
                                length_bytes: Some((span.end - span.start).to_string()),
                                verification: state.into(),
                                reason: s.item.reason.clone(),
                            })
                            .collect()
                    };
                    if !matches!(verify, Verify::None) {
                        s.item.errors.push(Error {
                            category: Status::VerificationIncomplete,
                            message: "write acknowledged; verification not reached".into(),
                        });
                    }
                }
            }
        }
        report.items.extend(self.steps.into_iter().map(|s| s.item));
        report.summarize();
    }
}
fn mutate(
    channel: &mut impl QdlChan,
    s: &mut Step,
    sector: u64,
    mode: crate::firehose::WireMode,
) -> bool {
    let result = match &mut s.action {
        Action::Skip => return true,
        Action::Program(p) => {
            let reader = p
                .payload
                .reader(0, p.bytes)
                .map_err(MutationFailure::not_sent);
            match reader {
                Ok(reader) => {
                    let raw = std::cell::Cell::new(false);
                    let mut counted = CountChannel {
                        inner: channel,
                        raw: &raw,
                        accepted: 0,
                        count_unknown: false,
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
                        s.slot,
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
            s.slot,
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
            s.item.execution = "done".into();
            s.item.write = "acknowledged".into();
            s.item.transferred_bytes = if matches!(s.action, Action::Program(_)) {
                s.item.transfer_bytes.clone()
            } else {
                None
            };
            true
        }
        Err(e) => {
            s.item.mutation_failure(&e);
            false
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
            || later.slot != steps[index].slot
            || matches!(
                later.item.write.as_str(),
                "not_attempted" | "not_applicable"
            )
        {
            continue;
        }
        let unknown = matches!(later.action, Action::Patch { .. })
            || later.item.start_byte.is_none()
            || later.item.write != "acknowledged";
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
fn verify_step(
    channel: &mut impl QdlChan,
    steps: &mut [Step],
    index: usize,
    limit: usize,
    sector: u64,
    mode: Verify,
    wire_mode: crate::firehose::WireMode,
) -> bool {
    if steps[index].item.write != "acknowledged"
        || !matches!(
            steps[index].action,
            Action::Program(_) | Action::Patch { .. }
        )
    {
        return true;
    }
    let (base, spans) = effective_spans(steps, index, limit);
    let s = &mut steps[index];
    if base.is_some() && spans.is_empty() {
        s.item.reason = Some("superseded".into());
        return true;
    }
    if matches!(mode, Verify::None) {
        s.item.verification = "not_requested".into();
        if base.is_none() {
            s.item.ranges.push(Range {
                start_byte: None,
                length_bytes: None,
                verification: "not_requested".into(),
                reason: s.item.reason.clone(),
            });
        } else {
            s.item.ranges.extend(spans.into_iter().map(|span| Range {
                start_byte: Some(span.start.to_string()),
                length_bytes: Some((span.end - span.start).to_string()),
                verification: "not_requested".into(),
                reason: None,
            }));
        }
        return true;
    }
    let mut keep_going = true;
    if base.is_none() {
        s.item.ranges.push(Range {
            start_byte: None,
            length_bytes: None,
            verification: "incomplete".into(),
            reason: Some("unknown address".into()),
        });
    }
    for span in spans {
        let mut range = Range {
            start_byte: Some(span.start.to_string()),
            length_bytes: Some((span.end - span.start).to_string()),
            verification: "incomplete".into(),
            reason: None,
        };
        if span.unknown {
            range.reason = Some("unknown expected content after patch or expression write".into());
        } else if !keep_going {
            range.reason = Some("verification stopped".into());
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
                    let result = crate::firehose::read_storage_with_mode(
                        channel,
                        &mut compare,
                        ((end - first) / sector) as usize,
                        s.slot,
                        s.item.lun.unwrap(),
                        first / sector,
                        wire_mode,
                    );
                    let mismatch = compare.mismatch;
                    match result {
                        Ok(()) if !mismatch => range.verification = "matched".into(),
                        Ok(()) => {
                            range.verification = "mismatch".into();
                            keep_going = false;
                        }
                        Err(e) => {
                            let refused = e.is::<ReadRejected>();
                            range.reason = Some(if refused {
                                "read_refused".into()
                            } else {
                                format!("read failed: {e:#}")
                            });
                            keep_going = refused && matches!(mode, Verify::BestEffort);
                        }
                    }
                    if mismatch {
                        range.verification = "mismatch".into();
                        keep_going = false;
                    }
                }
                Err(e) => {
                    range.reason = Some(format!("expected input: {e}"));
                    keep_going = false;
                }
            }
        }
        s.item.ranges.push(range);
    }
    let state = if s.item.ranges.iter().any(|r| r.verification == "mismatch") {
        "mismatch"
    } else if s.item.ranges.iter().any(|r| r.verification == "incomplete") {
        "incomplete"
    } else {
        "matched"
    };
    s.item.verification = state.into();
    if state != "matched" {
        s.item.errors.push(Error {
            category: if state == "mismatch" {
                Status::ExecutionFailed
            } else {
                Status::VerificationIncomplete
            },
            message: format!("write acknowledged; verification {state}"),
        });
    }
    keep_going && !(state == "incomplete" && matches!(mode, Verify::Required))
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
    let mut targets = vec![];
    for &lun in luns {
        let table = crate::storage::read_gpt_with(channel, |ch, out, sectors, start| {
            let sector = ch.fh_config().storage_sector_size as u64;
            let mut i = Item::read(lun, None, start * sector, sectors as u64 * sector);
            let result = crate::firehose::read_storage(ch, out, sectors, 0, lun, start);
            i.transferred_bytes = Some(out.len().to_string());
            match &result {
                Ok(()) => i.execution = "done".into(),
                Err(e) => i.fail(Status::ExecutionFailed, e.to_string()),
            }
            report.items.push(i);
            result
        })
        .inspect_err(|e| {
            if let Some(i) = report.items.last_mut()
                && i.errors.is_empty()
            {
                i.fail(Status::ExecutionFailed, format!("GPT validation: {e:#}"));
            }
        })?;
        targets.extend(crate::storage::partitions(&table, lun)?);
    }
    report.coverage.limited = true;
    report.coverage.reason = Some(format!("selected GPT partitions in LUNs {luns:?}"));
    let mappings = match (from, image) {
        (Some(dir), None) => crate::restore::directory(dir, selected_lun, names, &targets)?,
        (None, Some(image)) if names.len() == 1 => vec![crate::restore::single(
            image,
            &names[0],
            selected_lun,
            &targets,
        )?],
        _ => bail!("invalid restore source selection"),
    };
    Writes::restore(mappings, channel.fh_config().storage_sector_size as u64)
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
