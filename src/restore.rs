//! Offline preflight only: this module has no transport or mutation entry point.
use crate::storage::{self, Partition};
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
fn mapping(source: PathBuf, target: &Partition) -> Result<Mapping> {
    let file = crate::input::open_regular(&source)?;
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
    let matches: Vec<_> = targets
        .iter()
        .filter(|p| p.name == name && lun.is_none_or(|l| p.lun == l))
        .collect();
    if matches.len() != 1 {
        bail!("partition is missing or ambiguous");
    }
    mapping(image.into(), matches[0])
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
        let n = lun.context("flat directory requires --lun")?;
        sources.extend(flat.into_iter().map(|p| (n, p)));
    } else {
        for (n, dir) in dirs {
            if lun.is_none_or(|l| l == n) {
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
        if !names.is_empty() && !names.iter().any(|s| s == name) {
            continue;
        }
        if !storage::safe_name(name) {
            bail!("unsafe directory partition name");
        }
        let entry = single(&path, name, Some(n), targets)?;
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
    for name in names {
        if !result.iter().any(|m| &m.target.name == name) {
            bail!("selected partition image missing: {name}");
        }
    }
    Ok(result)
}
