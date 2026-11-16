//! Explicit SFP package preparation profile reconstructed from the RMX3800 success log.
//! This profile protects LUN5 and OCDT; ordinary --program remains strict and unchanged.
use crate::{
    cli::RawSize,
    program_xml::{self, Candidate},
    super_image,
};
use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use serde_json::json;
use std::{
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
};
use xmltree::{Element, XMLNode};

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FlashConfig {
    #[serde(default)]
    excluded_partitions_for_write: Vec<String>,
}
pub struct Prepared {
    pub candidates: Vec<Candidate>,
    pub manifest: PathBuf,
    pub project: u32,
}
#[derive(Deserialize)]
struct BuildMetadata {
    ofp: OfpMetadata,
}
#[derive(Deserialize)]
struct OfpMetadata {
    product: String,
    build_cfg: Option<OfpBuildConfig>,
}
#[derive(Deserialize)]
struct OfpBuildConfig {
    #[serde(rename = "PRODUCT_VARIANT")]
    product_variant: Option<String>,
}
fn project_id(value: &str) -> Result<u32> {
    ensure!(
        !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()),
        "invalid package project ID"
    );
    let project = value.parse::<u32>().context("invalid package project ID")?;
    ensure!(project != 0, "invalid package project ID");
    Ok(project)
}
fn attr<'a>(e: &'a Element, key: &str) -> Result<&'a str> {
    e.attributes
        .get(key)
        .map(String::as_str)
        .with_context(|| format!("missing {key}"))
}

// Keep NUM_DISK_SECTORS symbolic. Ranges with different bases cannot be proved
// disjoint offline, and must not bypass an explicit write exclusion.
fn range_start(start: &str, sector: u64) -> Result<(bool, i128)> {
    program_xml::expression(start, false)?;
    let start = start.strip_suffix('.').unwrap_or(start);
    if let Some(suffix) = start.strip_prefix("NUM_DISK_SECTORS") {
        let offset = if suffix.is_empty() {
            0
        } else {
            suffix.parse::<i128>()?
        };
        Ok((true, offset * i128::from(sector)))
    } else {
        Ok((
            false,
            i128::from(start.parse::<u64>()?) * i128::from(sector),
        ))
    }
}
struct ExcludedRange {
    label: String,
    start: (bool, i128),
    bytes: u64,
}
fn check_excluded_patch(node: &Element, excluded: &[ExcludedRange], sector: u64) -> Result<()> {
    if attr(node, "filename")? != "DISK" || excluded.is_empty() {
        return Ok(());
    }
    let (relative, start) = range_start(attr(node, "start_sector")?, sector)?;
    let start = start + i128::from(attr(node, "byte_offset")?.parse::<u64>()?);
    let end = start + i128::from(attr(node, "size_in_bytes")?.parse::<u64>()?);
    for range in excluded {
        ensure!(
            range.bytes > 0
                && relative == range.start.0
                && (end <= range.start.1 || start >= range.start.1 + i128::from(range.bytes)),
            "cannot prove DISK patch avoids excluded partition {}; package selection is unsupported",
            range.label
        );
    }
    Ok(())
}

pub fn prepare(
    root: &Path,
    nv_id: &str,
    output: &Path,
    config: Option<&Path>,
    sector: u64,
) -> Result<Prepared> {
    ensure!(
        sector == 4096,
        "SFP package profile requires 4096-byte sectors"
    );
    ensure!(
        !nv_id.is_empty() && nv_id.bytes().all(|b| b.is_ascii_digit()),
        "invalid NV ID"
    );
    let root = root.canonicalize()?;
    let build = super_image::package_path(&root, "META/build.json")?;
    let build: BuildMetadata = serde_json::from_reader(crate::input::open_regular(&build)?)
        .context("invalid package build metadata")?;
    let project = project_id(&build.ofp.product)?;
    if let Some(variant) = build.ofp.build_cfg.and_then(|c| c.product_variant) {
        ensure!(
            project_id(&variant)? == project,
            "conflicting package project IDs"
        );
    }
    let definition = super_image::package_path(&root, &format!("META/super_def.{nv_id}.json"))?;
    let config: FlashConfig = match config {
        Some(p) => serde_json::from_reader(crate::input::open_regular(p)?)?,
        None => FlashConfig::default(),
    };
    // Require a fresh output directory, so stale super images cannot be selected.
    fs::create_dir(output).context("prepare output must be a new directory")?;
    let output = output.canonicalize()?;
    let images = root.join("IMAGES");
    let super_path = output.join("super.img");
    let mut programs = Vec::new();
    let mut patches = Vec::new();
    let mut decisions = Vec::new();
    let mut super_count = 0;
    let mut super_bytes = 0;
    // Keep this fixed profile separate from general XML processing. In the observed
    // session rawprogram5 and patch5 were protected together, not just modemst.
    for lun in 0..=5 {
        let source = images.join(format!("rawprogram{lun}.xml"));
        let mut xml = Element::parse(crate::input::open_regular(&source)?)?;
        ensure!(xml.name == "data", "invalid rawprogram root");
        let mut selected = Vec::new();
        let mut excluded = Vec::new();
        for node in xml.children.drain(..) {
            let XMLNode::Element(mut e) = node else {
                continue;
            };
            ensure!(e.name == "program", "unexpected package XML operation");
            ensure!(
                attr(&e, "physical_partition_number")?.parse::<u8>()? == lun,
                "package XML LUN mismatch"
            );
            let label = e.attributes.get("label").cloned().unwrap_or_default();
            let filename = attr(&e, "filename")?.to_string();
            if lun != 5
                && config
                    .excluded_partitions_for_write
                    .iter()
                    .any(|p| p.eq_ignore_ascii_case(&label))
            {
                excluded.push(ExcludedRange {
                    label: label.clone(),
                    start: range_start(attr(&e, "start_sector")?, sector)?,
                    bytes: attr(&e, "num_partition_sectors")?
                        .parse::<u64>()?
                        .checked_mul(sector)
                        .context("excluded partition size overflow")?,
                });
            }
            let reason = if lun == 5 {
                Some("protected LUN5")
            } else if label.eq_ignore_ascii_case("ocdt") {
                Some("preserve device OCDT")
            } else if config
                .excluded_partitions_for_write
                .iter()
                .any(|p| p.eq_ignore_ascii_case(&label))
            {
                Some("excludedPartitionsForWrite")
            } else if filename.is_empty() {
                Some("empty filename")
            } else {
                None
            };
            if let Some(reason) = reason {
                decisions.push(json!({"lun":lun,"label":label,"filename":filename,"selected":false,"reason":reason}));
                continue;
            }
            let path = if label == "super" {
                super_count += 1;
                ensure!(super_count == 1, "ambiguous package super entries");
                ensure!(filename == "super.img", "unsupported segmented super entry");
                ensure!(
                    matches!(attr(&e, "sparse")?, "true" | "1")
                        && attr(&e, "file_sector_offset")?.parse::<u64>()? == 0,
                    "unsupported package super source"
                );
                super_bytes = attr(&e, "num_partition_sectors")?
                    .parse::<u64>()?
                    .checked_mul(sector)
                    .context("package super size overflow")?;
                ensure!(super_bytes > 0, "package super has zero target span");
                super_path.clone()
            } else {
                // Preview deselects absent optional images. Preserve that decision
                // visibly in selection.json; never invent an alias or padded file.
                match super_image::package_path(&root, &format!("IMAGES/{filename}")) {
                    Ok(p) => p,
                    Err(_e) if !images.join(&filename).exists() && label == "secdata" => {
                        decisions.push(json!({"lun":lun,"label":label,"filename":filename,"selected":false,"reason":"optional secdata image absent"}));
                        continue;
                    }
                    Err(e) => return Err(e),
                }
            };
            if label == "userdata" && attr(&e, "num_partition_sectors")? == "0" {
                ensure!(
                    attr(&e, "sparse")? == "true" && attr(&e, "file_sector_offset")? == "0",
                    "unsupported zero-span userdata"
                );
                let image = crate::sparse::parse(&mut crate::input::open_regular(&path)?, sector)?;
                ensure!(image.expanded_bytes > 0, "empty userdata image");
                e.attributes.insert(
                    "num_partition_sectors".into(),
                    (image.expanded_bytes / sector).to_string(),
                );
            }
            e.attributes.insert(
                "filename".into(),
                path.to_str().context("non-UTF8 package path")?.into(),
            );
            decisions.push(json!({"lun":lun,"label":label,"filename":filename,"selected":true}));
            selected.push(e);
        }
        if lun == 5 {
            continue;
        }
        // Stable within each class: the success log puts PrimaryGPT first and
        // BackupGPT last, retaining package order for the remaining entries.
        selected.sort_by_key(|e| match e.attributes.get("label").map(String::as_str) {
            Some("PrimaryGPT") => 0,
            Some("BackupGPT") => 2,
            _ => 1,
        });
        xml.children = selected.into_iter().map(XMLNode::Element).collect();
        let path = output.join(format!("rawprogram{lun}.xml"));
        xml.write(
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)?,
        )?;
        programs.push(path);
        let path = images.join(format!("patch{lun}.xml"));
        let patch_xml = Element::parse(crate::input::open_regular(&path)?)?;
        for node in patch_xml.children.iter().filter_map(XMLNode::as_element) {
            ensure!(
                attr(node, "physical_partition_number")?.parse::<u8>()? == lun,
                "patch LUN mismatch"
            );
            check_excluded_patch(node, &excluded, sector)?;
        }
        patches.push(path);
    }
    ensure!(
        super_count == 1,
        "package profile requires one selected super"
    );
    super_image::build(&root, &definition, &super_path)?;
    let candidates =
        program_xml::parse_with_raw_size(&programs, &patches, sector, false, RawSize::File)?;
    let super_candidate = candidates
        .iter()
        .find(|c| c.label.as_deref() == Some("super"))
        .context("package profile requires executable super")?;
    ensure!(
        matches!(&super_candidate.operation,
        program_xml::Operation::Program { span_bytes, transfer_bytes, sparse: Some(_), .. }
        if *span_bytes == super_bytes && *transfer_bytes > 0),
        "package super must cover its declared target span"
    );
    let order:Vec<_>=candidates.iter().filter_map(|c|match &c.operation{
        program_xml::Operation::Program{source,span_bytes,transfer_bytes,..}=>Some(json!({"lun":c.lun,"label":c.label,"filename":source.file_name().and_then(|n| n.to_str()),"span_bytes":span_bytes,"transfer_bytes":transfer_bytes})),_=>None
    }).collect();
    let manifest = output.join("selection.json");
    serde_json::to_writer_pretty(
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&manifest)?,
        &json!({
            "profile":"sfp-preserve-lun5-ocdt","nv_id":nv_id,"project":project,"definition":definition,
            "programs":programs,"patches":patches,"raw_size":"file","decisions":decisions,"program_order":order,
            "device_validated":false,"note":"Offline preparation only; does not validate device project, cloud loader, digest, or signature."
        }),
    )?;
    Ok(Prepared {
        candidates,
        manifest,
        project,
    })
}
