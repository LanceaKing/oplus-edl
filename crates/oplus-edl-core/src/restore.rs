//! Offline preflight only: this module has no transport or mutation entry point.
use crate::{
    input::Sources,
    model::PartitionTargets,
    storage::{self, Partition},
};
use anyhow::{Context, Result, bail};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug)]
pub struct Mapping {
    pub source: PathBuf,
    pub target: Partition,
    pub source_bytes: u64,
    pub transfer_bytes: u64,
    pub uncovered_bytes: u64,
}
fn mapping(source: PathBuf, target: &Partition, sources: &mut Sources) -> Result<Mapping> {
    let file = sources.open(&source)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() == 0 || target.sector_size == 0 {
        bail!("empty image or invalid geometry");
    }
    let sector = target.sector_size as u64;
    let transfer = metadata
        .len()
        .checked_add(sector - 1)
        .context("image size overflow")?
        / sector
        * sector;
    let uncovered = target
        .bytes()?
        .checked_sub(transfer)
        .context("image exceeds partition")?;
    target
        .start_byte()?
        .checked_add(transfer)
        .context("target end overflow")?;
    Ok(Mapping {
        source,
        target: target.clone(),
        source_bytes: metadata.len(),
        transfer_bytes: transfer,
        uncovered_bytes: uncovered,
    })
}
pub fn single(image: &Path, name: &str, lun: Option<u8>, targets: &[Partition]) -> Result<Mapping> {
    let mut sources = Sources::default();
    let result = single_bound(image, name, lun, targets, &mut sources)?;
    sources.check()?;
    Ok(result)
}

pub(crate) fn single_bound(
    image: &Path,
    name: &str,
    lun: Option<u8>,
    targets: &[Partition],
    sources: &mut Sources,
) -> Result<Mapping> {
    let matches: Vec<_> = targets
        .iter()
        .filter(|p| p.name == name && lun.is_none_or(|l| p.lun == l))
        .collect();
    if matches.len() != 1 {
        bail!("partition is missing or ambiguous");
    }
    mapping(image.into(), matches[0], sources)
}
fn images(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type()?.is_file() && entry.path().extension().is_some_and(|s| s == "bin") {
            files.push(entry.path());
        }
    }
    files.sort();
    Ok(files)
}
pub fn directory(
    root: &Path,
    lun: Option<u8>,
    names: &[String],
    targets: &[Partition],
) -> Result<Vec<Mapping>> {
    let selected = PartitionTargets::from_names(lun, names);
    let mut sources = Sources::default();
    let luns = lun.as_ref().map(std::slice::from_ref);
    let result = directory_bound(root, luns, &selected, targets, &mut sources)?;
    sources.check()?;
    Ok(result)
}

pub(crate) fn directory_bound(
    root: &Path,
    luns: Option<&[u8]>,
    selected: &PartitionTargets,
    targets: &[Partition],
    inputs: &mut Sources,
) -> Result<Vec<Mapping>> {
    let root = inputs.directory(root)?;
    let root = root.as_path();
    let flat = images(root)?;
    let mut dirs = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        if entry.file_type()?.is_dir()
            && let Some(n) = entry.file_name().to_str().and_then(storage::lun_directory)
        {
            dirs.push((n, entry.path()));
        }
    }
    dirs.sort();
    if !flat.is_empty() && !dirs.is_empty() {
        bail!("mixed flat and LUN directory layout");
    }
    let mut sources = Vec::new();
    if dirs.is_empty() {
        let Some([n]) = luns else {
            bail!("flat directory requires a single target LUN");
        };
        sources.extend(flat.into_iter().map(|p| (*n, p)));
    } else {
        for (n, dir) in dirs {
            if luns.is_none_or(|luns| luns.contains(&n)) {
                sources.extend(images(&dir)?.into_iter().map(|p| (n, p)));
            }
        }
    }
    let mut result = Vec::new();
    for (n, path) in sources {
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .context("non-UTF8 partition name")?;
        if !selected.includes(n, name) {
            continue;
        }
        if !storage::safe_name(name) {
            bail!("unsafe directory partition name");
        }
        let entry = single_bound(&path, name, Some(n), targets, inputs)?;
        result.push(entry);
    }
    // Exact-name uniqueness was checked against all targets by single().
    // Directory case collisions concern only mappings selected for this run.
    for entry in &result {
        if storage::name_conflicts(result.iter().map(|m| &m.target), &entry.target) {
            bail!("target directory name collision");
        }
    }
    if result.is_empty() {
        bail!("no selected partition images");
    }
    for target in selected.explicit() {
        if !result.iter().any(|m| {
            m.target.name == target.name && target.lun.is_none_or(|lun| m.target.lun == lun)
        }) {
            bail!("selected partition image missing: {}", target.name);
        }
    }
    Ok(result)
}
