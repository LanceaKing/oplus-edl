//! Offline firmware candidates; no device access and no implicit final actions.
use crate::sparse;
use anyhow::{Context, Result, bail};
use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};
use xmltree::{Element, XMLNode};
#[derive(Debug)]
pub enum Operation {
    Program {
        source: PathBuf,
        source_bytes: u64,
        source_offset: u64,
        transfer_bytes: u64,
        span_bytes: u64,
        sparse: Option<sparse::Image>,
    },
    Patch {
        byte_offset: u64,
        size: u64,
        value: String,
    },
    Skip {
        reason: String,
    },
}
#[derive(Debug)]
pub struct Candidate {
    pub xml: PathBuf,
    pub xml_node: usize,
    pub lun: u8,
    pub slot: u8,
    pub sector_size: u64,
    pub start_sector: String,
    pub label: Option<String>,
    pub operation: Operation,
}
fn attr<'a>(e: &'a Element, key: &str) -> Result<&'a str> {
    e.attributes
        .get(key)
        .map(String::as_str)
        .with_context(|| format!("missing {key}"))
}
fn number(e: &Element, key: &str) -> Result<u64> {
    attr(e, key)?
        .parse()
        .with_context(|| format!("invalid {key}"))
}
fn optional(e: &Element, key: &str, default: u64) -> Result<u64> {
    if e.attributes.contains_key(key) {
        number(e, key)
    } else {
        Ok(default)
    }
}
/// Preserve the limited protocol expression syntax without evaluating device values.
pub fn expression(text: &str, crc: bool) -> Result<()> {
    let s = text.strip_suffix('.').unwrap_or(text);
    if s.parse::<u64>().is_ok() {
        return Ok(());
    }
    if let Some(rest) = s.strip_prefix("NUM_DISK_SECTORS")
        && (rest.is_empty()
            || rest
                .strip_prefix(['-', '+'])
                .is_some_and(|v| v.parse::<u64>().is_ok()))
    {
        return Ok(());
    }
    if crc
        && let Some(inner) = s.strip_prefix("CRC32(").and_then(|s| s.strip_suffix(')'))
        && let Some((start, len)) = inner.split_once(',')
    {
        expression(start.trim(), false)?;
        len.trim().parse::<u64>().context("invalid CRC32 length")?;
        return Ok(());
    }
    bail!("unsupported protocol expression: {text}")
}
fn parse_file(path: &Path, patch: bool, sector: u64, result: &mut Vec<Candidate>) -> Result<()> {
    let nodes = Element::parse_all(File::open(path)?)?;
    let roots: Vec<_> = nodes.iter().filter_map(XMLNode::as_element).collect();
    if roots.len() != 1 {
        bail!("expected one XML root");
    }
    let root = roots[0];
    if !matches!(root.name.as_str(), "data" | "patches") {
        bail!("unsupported XML root");
    }
    let mut index = 0;
    for node in &root.children {
        let e = match node {
            XMLNode::Element(e) => e,
            XMLNode::Comment(_) => continue,
            XMLNode::Text(s) if s.trim().is_empty() => continue,
            _ => bail!("unexpected XML content"),
        };
        index += 1;
        if e.name != if patch { "patch" } else { "program" }
            || !e.children.is_empty()
            || e.namespace.is_some()
        {
            bail!("unsupported XML operation: {}", e.name);
        }
        let size = number(e, "SECTOR_SIZE_IN_BYTES")?;
        if size != sector || !matches!(size, 512 | 4096) {
            bail!("sector size conflict");
        }
        let lun = u8::try_from(number(e, "physical_partition_number")?)?;
        let slot = u8::try_from(optional(e, "slot", 0)?)?;
        let start = attr(e, "start_sector")?;
        expression(start, false)?;
        let filename = attr(e, "filename")?;
        let operation = if patch {
            let byte_offset = number(e, "byte_offset")?;
            let bytes = number(e, "size_in_bytes")?;
            if bytes == 0
                || bytes > 8
                || byte_offset
                    .checked_add(bytes)
                    .is_none_or(|end| end > sector)
            {
                bail!("invalid patch byte range");
            }
            let value = attr(e, "value")?;
            expression(value, true)?;
            if filename != "DISK" {
                Operation::Skip {
                    reason: format!("local file patch skipped: {filename}"),
                }
            } else {
                check_end(start, sector, byte_offset + bytes)?;
                Operation::Patch {
                    byte_offset,
                    size: bytes,
                    value: value.into(),
                }
            }
        } else {
            let sectors = number(e, "num_partition_sectors")?;
            let span = sectors
                .checked_mul(sector)
                .context("program size overflow")?;
            let offset = optional(e, "file_sector_offset", 0)?
                .checked_mul(sector)
                .context("source offset overflow")?;
            let sparse_flag = match e.attributes.get("sparse").map(String::as_str) {
                None | Some("false") | Some("0") => false,
                Some("true") | Some("1") => true,
                _ => bail!("invalid sparse flag"),
            };
            check_end(start, sector, span)?;
            if filename.is_empty() {
                Operation::Skip {
                    reason: "empty filename".into(),
                }
            } else {
                let source = path.parent().unwrap_or(Path::new(".")).join(filename);
                let mut file =
                    File::open(&source).with_context(|| format!("open {}", source.display()))?;
                let metadata = file.metadata()?;
                if !metadata.is_file() {
                    bail!("program source is not a regular file");
                }
                let source_bytes = metadata.len();
                if sectors == 0 {
                    Operation::Skip {
                        reason: "zero-length program".into(),
                    }
                } else if sparse_flag {
                    if offset != 0 {
                        bail!("sparse source offset is unsupported");
                    }
                    let image = sparse::parse(&mut file, sector)?;
                    if image.expanded_bytes > span {
                        bail!("sparse image exceeds declared target range");
                    }
                    if image.transfer_bytes == 0 {
                        Operation::Skip {
                            reason: "sparse image contains no writable ranges".into(),
                        }
                    } else {
                        Operation::Program {
                            source,
                            source_bytes,
                            source_offset: 0,
                            transfer_bytes: image.transfer_bytes,
                            span_bytes: image.expanded_bytes,
                            sparse: Some(image),
                        }
                    }
                } else {
                    let mut magic = [0; 4];
                    if file.read(&mut magic)? == 4 && u32::from_le_bytes(magic) == sparse::MAGIC {
                        bail!("sparse image requires sparse=true");
                    }
                    let available = source_bytes
                        .checked_sub(offset)
                        .context("source offset beyond file")?;
                    if available == 0 || span.saturating_sub(available) >= sector {
                        bail!("source range exceeds file beyond final-sector padding");
                    }
                    Operation::Program {
                        source,
                        source_bytes,
                        source_offset: offset,
                        transfer_bytes: span,
                        span_bytes: span,
                        sparse: None,
                    }
                }
            }
        };
        result.push(Candidate {
            xml: path.into(),
            xml_node: index,
            lun,
            slot,
            sector_size: sector,
            start_sector: start.into(),
            label: e.attributes.get("label").cloned(),
            operation,
        });
    }
    Ok(())
}
fn check_end(start: &str, sector: u64, bytes: u64) -> Result<()> {
    if let Ok(start) = start.strip_suffix('.').unwrap_or(start).parse::<u64>() {
        start
            .checked_mul(sector)
            .and_then(|s| s.checked_add(bytes))
            .context("target range overflow")?;
    }
    Ok(())
}
/// The caller supplies known package requirements; unknown local dependencies are not proved safe.
pub fn parse(
    programs: &[PathBuf],
    patches: &[PathBuf],
    sector: u64,
    requires_local_transform: bool,
) -> Result<Vec<Candidate>> {
    if requires_local_transform {
        bail!("package requires unsupported local image transformations");
    }
    let mut result = Vec::new();
    for path in programs {
        parse_file(path, false, sector, &mut result)?;
    }
    for path in patches {
        parse_file(path, true, sector, &mut result)?;
    }
    if !result
        .iter()
        .any(|c| !matches!(c.operation, Operation::Skip { .. }))
    {
        bail!("no executable firmware operations");
    }
    Ok(result)
}
