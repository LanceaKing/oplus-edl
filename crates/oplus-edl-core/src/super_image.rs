//! Build a sparse super image from an SFP definition and its reference LP metadata.
//! Preserve the package's extents, groups, attributes, and metadata version.
use crate::{
    input::Sources,
    result::{OutputState, Report},
    sparse::{self, Data},
};
use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Component, Path, PathBuf},
};

#[derive(Deserialize)]
struct FileRef {
    path: String,
}
#[derive(Deserialize)]
struct Device {
    name: String,
    size: String,
    block_size: String,
    alignment: String,
}
#[derive(Deserialize)]
struct Group {
    name: String,
    maximum_size: Option<String>,
}
#[derive(Deserialize)]
struct Partition {
    name: String,
    group_name: String,
    path: Option<String>,
    size: Option<String>,
}
#[derive(Deserialize)]
struct Definition {
    super_meta: FileRef,
    block_devices: Vec<Device>,
    groups: Vec<Group>,
    partitions: Vec<Partition>,
}

pub(crate) fn relative_path(name: &str) -> Result<&Path> {
    let relative = Path::new(name);
    ensure!(
        !relative.as_os_str().is_empty()
            && relative
                .components()
                .all(|c| matches!(c, Component::Normal(_))),
        "invalid package path: {name}"
    );
    Ok(relative)
}

pub(crate) fn package_path(root: &Path, name: &str, sources: &mut Sources) -> Result<PathBuf> {
    let selected = root.join(relative_path(name)?);
    let path = selected
        .canonicalize()
        .with_context(|| format!("package input {name}"))?;
    ensure!(path.starts_with(root), "package input escapes root: {name}");
    sources.open(&selected)?;
    Ok(path)
}
fn u32at(b: &[u8], o: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(
        b.get(o..o + 4).context("truncated LP field")?.try_into()?,
    ))
}
fn u64at(b: &[u8], o: usize) -> Result<u64> {
    Ok(u64::from_le_bytes(
        b.get(o..o + 8).context("truncated LP field")?.try_into()?,
    ))
}
fn name(b: &[u8]) -> Result<String> {
    let end = b.iter().position(|x| *x == 0).unwrap_or(b.len());
    Ok(std::str::from_utf8(&b[..end])?.to_string())
}
fn checksum(b: &[u8], offset: usize) -> Result<()> {
    let mut copy = b.to_vec();
    let expected = copy
        .get(offset..offset + 32)
        .context("truncated LP checksum")?
        .to_vec();
    copy[offset..offset + 32].fill(0);
    ensure!(
        Sha256::digest(&copy)[..] == expected,
        "LP checksum mismatch"
    );
    Ok(())
}
fn table<'a>(
    header: &[u8],
    tables: &'a [u8],
    index: usize,
    stride: usize,
) -> Result<Vec<&'a [u8]>> {
    let at = 80 + index * 12;
    let offset = u32at(header, at)? as usize;
    let count = u32at(header, at + 4)? as usize;
    ensure!(
        u32at(header, at + 8)? as usize == stride,
        "unsupported LP table entry size"
    );
    let end = count
        .checked_mul(stride)
        .and_then(|n| offset.checked_add(n))
        .context("LP table overflow")?;
    Ok(tables
        .get(offset..end)
        .context("LP table outside metadata")?
        .chunks_exact(stride)
        .collect())
}
struct Source {
    path: PathBuf,
    target: u64,
    bytes: u64,
    image: Option<sparse::Image>,
}

pub(crate) struct Layout {
    size: u64,
    region: Vec<u8>,
    sources: Vec<Source>,
}

/// Only the package's single-device, linear reference layout is accepted.
/// A definition must match every reference partition; missing inputs are errors.
pub fn build(root: &Path, definition: &Path, output: &Path) -> Result<()> {
    ensure!(!output.try_exists()?, "super output already exists");
    let root = root.canonicalize()?;
    let mut sources = Sources::default();
    let layout = plan(&root, definition, &mut sources, &|| Ok(()))?;
    layout.build(
        output,
        &mut sources,
        &|_| Ok(()),
        &mut Report::new(None, None),
    )
}

pub(crate) fn plan(
    root: &Path,
    definition: &Path,
    inputs: &mut Sources,
    check: &impl Fn() -> Result<()>,
) -> Result<Layout> {
    check()?;
    let def: Definition = serde_json::from_reader(inputs.open(definition)?)?;
    ensure!(
        def.block_devices.len() == 1,
        "multi-device super is unsupported"
    );
    let device = &def.block_devices[0];
    let size: u64 = device.size.parse()?;
    ensure!(
        device.block_size == "4096" && size.is_multiple_of(4096) && size / 4096 <= u32::MAX as u64,
        "unsupported super block geometry"
    );
    let meta_path = package_path(root, &def.super_meta.path, inputs)?;
    let mut meta_file = inputs.open(&meta_path)?;
    ensure!(
        meta_file.metadata()?.len() <= 16 * 1024 * 1024,
        "LP reference too large"
    );
    let mut meta = Vec::new();
    meta_file.read_to_end(&mut meta)?;
    ensure!(
        u32at(&meta, 0)? == 0x616c4467 && u32at(&meta, 4)? == 52,
        "invalid LP geometry"
    );
    checksum(meta.get(..52).context("truncated geometry")?, 8)?;
    let max = u32at(&meta, 40)? as usize;
    let slots = u32at(&meta, 44)? as usize;
    ensure!(
        max > 0
            && max <= 1024 * 1024
            && max.is_multiple_of(4096)
            && (1..=3).contains(&slots)
            && u32at(&meta, 48)? == 4096,
        "unsupported LP geometry"
    );
    let raw = meta.get(4096..).context("missing reference metadata")?;
    let version = u32at(raw, 4)?;
    let hs = u32at(raw, 8)? as usize;
    ensure!(
        u32at(raw, 0)? == 0x414c5030
            && matches!(version, 10 | 0x1000a | 0x2000a)
            && hs == if version == 0x2000a { 256 } else { 128 },
        "unsupported LP header"
    );
    let header = raw.get(..hs).context("truncated LP header")?;
    checksum(header, 12)?;
    let ts = u32at(header, 44)? as usize;
    ensure!(hs + ts <= max, "LP metadata exceeds slot size");
    let tables = raw.get(hs..hs + ts).context("truncated LP tables")?;
    ensure!(
        Sha256::digest(tables)[..] == header[48..80],
        "LP table checksum mismatch"
    );
    let parts = table(header, tables, 0, 52)?;
    let extents = table(header, tables, 1, 24)?;
    let groups = table(header, tables, 2, 48)?;
    let devices = table(header, tables, 3, 64)?;
    ensure!(devices.len() == 1, "multi-device reference unsupported");
    let dev = devices[0];
    ensure!(
        u64at(dev, 16)? == size
            && name(&dev[24..60])? == device.name
            && u32at(dev, 8)? as u64 == device.alignment.parse::<u64>()?
            && u32at(dev, 12)? == 0
            && u32at(dev, 60)? == 0,
        "reference block device differs from definition"
    );
    let first = u64at(dev, 0)?
        .checked_mul(512)
        .context("LP first sector overflow")?;
    let region_size = 12288 + 2 * slots * max;
    ensure!(
        first >= region_size as u64 && first <= size,
        "LP metadata overlaps data"
    );
    ensure!(def.groups.len() == groups.len(), "LP group count mismatch");
    for g in &def.groups {
        let entry = groups
            .iter()
            .find(|e| name(&e[..36]).ok().as_deref() == Some(&g.name))
            .context("missing reference group")?;
        ensure!(
            u64at(entry, 40)? == g.maximum_size.as_deref().unwrap_or("0").parse::<u64>()?
                && u32at(entry, 36)? == 0,
            "LP group differs from definition"
        );
    }
    let definitions: BTreeMap<_, _> = def
        .partitions
        .iter()
        .map(|p| (p.name.as_str(), p))
        .collect();
    ensure!(
        definitions.len() == parts.len() && definitions.len() == def.partitions.len(),
        "LP partition count or duplicate mismatch"
    );
    let mut seen = BTreeSet::new();
    let mut used_extents = BTreeSet::new();
    let mut group_sizes = vec![0u64; groups.len()];
    let mut sources = Vec::new();
    for p in parts {
        let label = name(&p[..36])?;
        ensure!(seen.insert(label.clone()), "duplicate LP partition");
        let d = definitions
            .get(label.as_str())
            .context("reference partition absent from definition")?;
        let gi = u32at(p, 48)? as usize;
        let group = groups.get(gi).context("invalid LP group index")?;
        ensure!(
            name(&group[..36])? == d.group_name && u32at(p, 36)? == 1,
            "unsupported reference partition attributes/group"
        );
        let ei = u32at(p, 40)? as usize;
        let count = u32at(p, 44)?;
        let bytes: u64 = d.size.as_deref().unwrap_or("0").parse()?;
        if count == 0 {
            ensure!(
                bytes == 0 && d.path.is_none(),
                "empty LP partition has input"
            );
            continue;
        }
        ensure!(
            count == 1 && used_extents.insert(ei),
            "only distinct single linear extents are supported"
        );
        let e = extents.get(ei).context("invalid LP extent index")?;
        let length = u64at(e, 0)?.checked_mul(512).context("LP size overflow")?;
        let target = u64at(e, 12)?
            .checked_mul(512)
            .context("LP offset overflow")?;
        ensure!(
            u32at(e, 8)? == 0
                && u32at(e, 20)? == 0
                && length == bytes
                && length > 0
                && target >= first
                && target.is_multiple_of(4096)
                && length.is_multiple_of(4096)
                && target.checked_add(length).is_some_and(|end| end <= size),
            "invalid reference extent for {label}"
        );
        group_sizes[gi] = group_sizes[gi]
            .checked_add(bytes)
            .context("group size overflow")?;
        let path = package_path(
            root,
            d.path.as_deref().context("missing partition image")?,
            inputs,
        )?;
        let mut f = inputs.open(&path)?;
        let mut magic = [0; 4];
        f.read_exact(&mut magic)?;
        let image = if u32::from_le_bytes(magic) == sparse::MAGIC {
            let image = sparse::parse_controlled(&mut f, 4096, check)?;
            ensure!(
                image.expanded_bytes == bytes,
                "sparse image size differs from LP: {label}"
            );
            Some(image)
        } else {
            ensure!(
                f.metadata()?.len() == bytes,
                "raw image size differs from LP: {label}"
            );
            None
        };
        sources.push(Source {
            path,
            target,
            bytes,
            image,
        });
    }
    ensure!(
        used_extents.len() == extents.len(),
        "unreferenced LP extents"
    );
    for (i, g) in groups.iter().enumerate() {
        let limit = u64at(g, 40)?;
        ensure!(
            limit == 0 || group_sizes[i] <= limit,
            "LP group over capacity"
        );
    }
    sources.sort_by_key(|s| s.target);
    let mut end = region_size as u64;
    for s in &sources {
        ensure!(s.target >= end, "overlapping LP extents");
        end = s.target + s.bytes;
    }
    let mut region = vec![0u8; region_size];
    region[4096..8192].copy_from_slice(&meta[..4096]);
    region[8192..12288].copy_from_slice(&meta[..4096]);
    for slot in 0..slots * 2 {
        let start = 12288 + slot * max;
        region[start..start + hs + ts].copy_from_slice(&raw[..hs + ts]);
    }
    Ok(Layout {
        size,
        region,
        sources,
    })
}

fn writable_ranges(ranges: impl IntoIterator<Item = (u64, u64)>) -> Vec<(u64, u64)> {
    let mut merged: Vec<(u64, u64)> = Vec::new();
    for (start, bytes) in ranges {
        if let Some((_, end)) = merged.last_mut().filter(|(_, end)| *end == start) {
            *end += bytes;
        } else {
            merged.push((start, start + bytes));
        }
    }
    merged
}

impl Layout {
    pub(crate) fn size(&self) -> u64 {
        self.size
    }

    pub(crate) fn validate(
        &self,
        output: &Path,
        inputs: &mut Sources,
        check: &impl Fn() -> Result<()>,
    ) -> Result<()> {
        check()?;
        let mut file = inputs.open(output)?;
        let image = sparse::parse_controlled(&mut file, 4096, check)?;
        ensure!(
            image.expanded_bytes == self.size,
            "prepared super span differs from the selected definition"
        );
        let Some(sparse::Chunk {
            offset: 0,
            bytes,
            data: Data::Raw { file_offset },
        }) = image.chunks.first()
        else {
            anyhow::bail!("prepared super lacks the expected LP metadata region");
        };
        ensure!(
            *bytes == self.region.len() as u64,
            "prepared super LP metadata span differs"
        );
        let mut region = vec![0; self.region.len()];
        file.seek(SeekFrom::Start(*file_offset))?;
        file.read_exact(&mut region)?;
        ensure!(
            region == self.region,
            "prepared super LP metadata differs from the selected definition"
        );
        let expected = writable_ranges(std::iter::once((0, self.region.len() as u64)).chain(
            self.sources.iter().flat_map(|source| {
                match &source.image {
                    Some(image) => image
                        .chunks
                        .iter()
                        .filter(|chunk| chunk.data != Data::DontCare)
                        .map(|chunk| (source.target + chunk.offset, chunk.bytes))
                        .collect::<Vec<_>>(),
                    None => vec![(source.target, source.bytes)],
                }
            }),
        ));
        let actual = writable_ranges(
            image
                .chunks
                .iter()
                .filter(|chunk| chunk.data != Data::DontCare)
                .map(|chunk| (chunk.offset, chunk.bytes)),
        );
        ensure!(
            expected == actual,
            "prepared super sparse ranges differ from the selected layout"
        );
        Ok(())
    }

    pub(crate) fn build(
        self,
        output: &Path,
        inputs: &mut Sources,
        checkpoint: &impl Fn(&'static str) -> Result<()>,
        report: &mut Report,
    ) -> Result<()> {
        let Self {
            size,
            region,
            sources,
        } = self;
        let check = || checkpoint("super image");
        check()?;
        let partial = output.with_extension("img.partial");
        let mut out = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&partial)?;
        report.output(partial.clone(), "super_image", OutputState::Partial);
        out.write_all(&[0; 28])?;
        let mut chunks = 0u32;
        write_chunk(&mut out, 0xcac1, region.len() as u64, region.len() as u64)?;
        out.write_all(&region)?;
        checkpoint("sparse partial written")?;
        chunks += 1;
        let mut pos = region.len() as u64;
        for s in sources {
            check()?;
            if s.target > pos {
                write_chunk(&mut out, 0xcac3, s.target - pos, 0)?;
                chunks += 1;
            }
            let mut f = inputs.open(&s.path)?;
            let source_chunks = s.image.map(|i| i.chunks).unwrap_or_else(|| {
                vec![sparse::Chunk {
                    offset: 0,
                    bytes: s.bytes,
                    data: Data::Raw { file_offset: 0 },
                }]
            });
            for c in source_chunks {
                check()?;
                match c.data {
                    Data::DontCare => {
                        write_chunk(&mut out, 0xcac3, c.bytes, 0)?;
                        chunks = chunks.checked_add(1).context("too many chunks")?;
                    }
                    Data::Fill(v) => {
                        write_chunk(&mut out, 0xcac2, c.bytes, 4)?;
                        out.write_all(&v)?;
                        chunks = chunks.checked_add(1).context("too many chunks")?;
                    }
                    Data::Raw { file_offset } => {
                        f.seek(SeekFrom::Start(file_offset))?;
                        let mut remaining = c.bytes;
                        while remaining > 0 {
                            let bytes = remaining.min(0xfffff000);
                            write_chunk(&mut out, 0xcac1, bytes, bytes)?;
                            let mut unwritten = bytes;
                            while unwritten != 0 {
                                check()?;
                                let block = unwritten.min(16 * 1024 * 1024);
                                ensure!(
                                    std::io::copy(&mut (&mut f).take(block), &mut out)? == block,
                                    "source truncated during super build"
                                );
                                unwritten -= block;
                            }
                            chunks = chunks.checked_add(1).context("too many chunks")?;
                            remaining -= bytes;
                        }
                    }
                }
            }
            pos = s.target + s.bytes;
        }
        if pos < size {
            write_chunk(&mut out, 0xcac3, size - pos, 0)?;
            chunks = chunks.checked_add(1).context("too many chunks")?;
        }
        out.seek(SeekFrom::Start(0))?;
        out.write_all(&sparse::MAGIC.to_le_bytes())?;
        for v in [1u16, 0, 28, 12] {
            out.write_all(&v.to_le_bytes())?;
        }
        for v in [4096u32, (size / 4096) as u32, chunks, 0] {
            out.write_all(&v.to_le_bytes())?;
        }
        out.sync_all()?;
        drop(out);
        inputs.check()?;
        check()?;
        crate::publish::no_replace(&partial, output)?;
        report.outputs.retain(|entry| entry.path != partial);
        report.output(output.to_owned(), "super_image", OutputState::Complete);
        Ok(())
    }
}
fn write_chunk(out: &mut File, kind: u16, bytes: u64, payload: u64) -> Result<()> {
    ensure!(bytes.is_multiple_of(4096), "unaligned sparse output");
    out.write_all(&kind.to_le_bytes())?;
    out.write_all(&0u16.to_le_bytes())?;
    out.write_all(&u32::try_from(bytes / 4096)?.to_le_bytes())?;
    out.write_all(&u32::try_from(payload + 12)?.to_le_bytes())?;
    Ok(())
}

#[cfg(test)]
#[test]
fn stopped_build_keeps_the_owned_sparse_partial_in_the_report() {
    let root = tempfile::tempdir().unwrap();
    let output = root.path().join("super.img");
    let partial = root.path().join("super.img.partial");
    let layout = Layout {
        size: 8192,
        region: vec![0; 4096],
        sources: vec![],
    };
    let mut report = Report::new(Some("package_prepare"), None);
    let check = |_| {
        if partial.exists() {
            Err(crate::session::Stopped.into())
        } else {
            Ok(())
        }
    };
    let error = layout
        .build(&output, &mut Sources::default(), &check, &mut report)
        .unwrap_err();
    assert!(error.is::<crate::session::Stopped>());
    assert!(!output.exists() && report.data.is_none());
    assert!(std::fs::metadata(&partial).unwrap().len() > 0);
    assert_eq!(report.outputs.len(), 1);
    assert_eq!(report.outputs[0].path, partial);
    assert_eq!(report.outputs[0].state, OutputState::Partial);
}
