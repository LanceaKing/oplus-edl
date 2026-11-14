use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Success,
    VerificationIncomplete,
    Unsupported,
    InputError,
    ExecutionFailed,
    UnknownWrite,
}
impl Status {
    pub fn exit_code(self) -> u8 {
        match self {
            Self::Success => 0,
            Self::InputError => 2,
            Self::Unsupported => 3,
            Self::ExecutionFailed => 4,
            Self::VerificationIncomplete => 5,
            Self::UnknownWrite => 6,
        }
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct Error {
    pub category: Status,
    pub message: String,
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
#[derive(Clone, Debug, Serialize)]
pub struct Range {
    pub start_byte: Option<String>,
    pub length_bytes: Option<String>,
    pub verification: String,
    pub reason: Option<String>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Item {
    pub id: usize,
    pub operation: String,
    pub lun: Option<u8>,
    pub partition: Option<String>,
    pub source: Option<String>,
    pub xml_node: Option<usize>,
    pub start_byte: Option<String>,
    pub source_bytes: Option<String>,
    pub partition_bytes: Option<String>,
    pub transfer_bytes: Option<String>,
    pub transferred_bytes: Option<String>,
    pub execution: String,
    pub write: String,
    pub verification: String,
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
            partition,
            source: None,
            xml_node: None,
            start_byte: Some(start.to_string()),
            source_bytes: None,
            partition_bytes: Some(size.to_string()),
            transfer_bytes: Some(size.to_string()),
            transferred_bytes: Some("0".into()),
            execution: "not_attempted".into(),
            write: "not_applicable".into(),
            verification: "not_applicable".into(),
            ranges: vec![],
            reason: None,
            errors: vec![],
        }
    }
    pub fn mutation_failure(&mut self, failure: &qdl::types::MutationFailure) {
        use qdl::types::MutationConfirmation;
        let (write, category) = match failure.confirmation {
            MutationConfirmation::NotSent => ("not_attempted", Status::ExecutionFailed),
            MutationConfirmation::Rejected => ("failed", Status::ExecutionFailed),
            MutationConfirmation::Unknown => ("unknown", Status::UnknownWrite),
        };
        self.write = write.into();
        self.verification = "not_applicable".into();
        self.fail(category, failure.cause.to_string());
    }
    pub fn reject(&mut self, category: Status, message: impl Into<String>) {
        self.execution = "not_attempted".into();
        self.errors.push(Error {
            category,
            message: message.into(),
        });
    }
    pub fn fail(&mut self, category: Status, message: impl Into<String>) {
        self.execution = "failed".into();
        self.errors.push(Error {
            category,
            message: message.into(),
        });
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    pub command: Option<String>,
    pub verify_mode: Option<String>,
    pub target: Option<Target>,
    pub coverage: Coverage,
    pub candidates: Vec<String>,
    pub status: Status,
    pub items: Vec<Item>,
    pub errors: Vec<Error>,
}
impl Report {
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
        }
    }
    pub fn error(&mut self, category: Status, message: impl Into<String>) {
        self.errors.push(Error {
            category,
            message: message.into(),
        });
        self.summarize();
    }
    pub fn summarize(&mut self) {
        self.status = self
            .errors
            .iter()
            .chain(self.items.iter().flat_map(|i| &i.errors))
            .map(|e| e.category)
            .max()
            .unwrap_or(Status::Success);
        for (index, item) in self.items.iter_mut().enumerate() {
            item.id = index + 1;
        }
    }
    pub fn emit(&self, json: bool) {
        if json {
            println!(
                "{}",
                serde_json::to_string(self).expect("serializable result")
            );
        } else {
            self.write_text(&mut std::io::stdout().lock())
                .expect("write result");
        }
    }
    /// Text and JSON use the same report; retain identities even without a safe output path.
    pub fn write_text(&self, out: &mut impl std::io::Write) -> std::io::Result<()> {
        writeln!(
            out,
            "{}: {:?}",
            self.command.as_deref().unwrap_or("input"),
            self.status
        )?;
        if let Some(reason) = &self.coverage.reason {
            writeln!(out, "Scope: {reason}")?;
        }
        for c in &self.candidates {
            writeln!(out, "Candidate: {c}")?;
        }
        for i in &self.items {
            writeln!(
                out,
                "{}: {} LUN {} partition {:?} source {:?}: {}; write={}; verification={} ({} bytes)",
                i.id,
                i.operation,
                i.lun.map(|n| n.to_string()).as_deref().unwrap_or("unknown"),
                i.partition.as_deref().unwrap_or("-"),
                i.source.as_deref().unwrap_or("-"),
                i.execution,
                i.write,
                i.verification,
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
                    range.verification,
                    range.reason.as_deref().unwrap_or("")
                )?;
            }
            for e in &i.errors {
                writeln!(out, "  item {} {:?}: {}", i.id, e.category, e.message)?;
            }
        }
        for e in &self.errors {
            writeln!(out, "{:?}: {}", e.category, e.message)?;
        }
        Ok(())
    }
}
