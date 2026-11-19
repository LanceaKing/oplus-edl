//! Explicit SFP package preparation profile reconstructed from the RMX3800 success log.
//! This profile protects LUN5 and OCDT; ordinary --program remains strict and unchanged.
use crate::{
    input::Sources,
    model::{PackageArtifacts, PackageInput, RawSize},
    program_xml::{self, Candidate},
    result::{OutputState, Report},
    super_image,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::BTreeMap,
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
const PROFILE: &str = "sfp-preserve-lun5-ocdt";
pub(crate) const SECTOR_SIZE: u64 = 4096;

#[derive(Clone, Debug, Serialize)]
pub struct Origin {
    pub root: PathBuf,
    pub profile: &'static str,
    pub nv_id: String,
    pub project: u32,
    pub definition: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct SelectionEntry {
    pub lun: u8,
    pub label: String,
    pub filename: String,
    pub selected: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Preparation {
    pub origin: Origin,
    pub manifest: PathBuf,
    pub super_image: PathBuf,
    pub programs: Vec<PathBuf>,
    pub patches: Vec<PathBuf>,
    pub exclusions: Vec<SelectionEntry>,
}

pub struct Prepared {
    pub candidates: Vec<Candidate>,
    pub manifest: PathBuf,
    pub project: u32,
    pub summary: Preparation,
    pub(crate) sources: Sources,
}

type PatchOperations = Vec<Vec<BTreeMap<String, String>>>;

#[derive(Debug, Deserialize, Serialize)]
struct Evidence {
    profile: String,
    nv_id: String,
    project: u32,
    definition: PathBuf,
    programs: Vec<PathBuf>,
    patches: Vec<PathBuf>,
    raw_size: String,
    decisions: Vec<SelectionEntry>,
    #[serde(default)]
    patch_operations: Option<PatchOperations>,
}

struct Selection {
    origin: Origin,
    programs: Vec<(PathBuf, Element)>,
    patches: Vec<(PathBuf, Element)>,
    decisions: Vec<SelectionEntry>,
    super_path: PathBuf,
    super_bytes: u64,
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

impl Selection {
    fn reconstruct(
        input: &PackageInput,
        output: &Path,
        sources: &mut Sources,
        checkpoint: &impl Fn(&'static str) -> Result<()>,
    ) -> Result<Self> {
        let PackageInput {
            root,
            nv_id,
            config,
        } = input;
        let root = sources.directory(root)?;
        checkpoint("package build metadata")?;
        let build_path = super_image::package_path(&root, "META/build.json", sources)?;
        checkpoint("package build metadata read")?;
        let build: BuildMetadata = serde_json::from_reader(sources.open(&build_path)?)
            .context("invalid package build metadata")?;
        let project = project_id(&build.ofp.product)?;
        if let Some(variant) = build.ofp.build_cfg.and_then(|c| c.product_variant) {
            ensure!(
                project_id(&variant)? == project,
                "conflicting package project IDs"
            );
        }
        checkpoint("package definition path")?;
        let definition =
            super_image::package_path(&root, &format!("META/super_def.{nv_id}.json"), sources)?;
        let config: FlashConfig = match config {
            Some(path) => {
                checkpoint("package flash configuration")?;
                serde_json::from_reader(sources.open(path)?)?
            }
            None => FlashConfig::default(),
        };
        let super_path = output.join("super.img");
        let mut selection = Self {
            origin: Origin {
                root,
                profile: PROFILE,
                nv_id: nv_id.clone(),
                project,
                definition,
            },
            programs: Vec::new(),
            patches: Vec::new(),
            decisions: Vec::new(),
            super_path,
            super_bytes: 0,
        };
        // Keep LUN5 and OCDT protected as a package policy.
        for lun in 0..=5 {
            selection.select_lun(lun, output, &config, sources, checkpoint)?;
        }
        ensure!(
            selection.super_bytes != 0,
            "package profile requires one selected super"
        );
        Ok(selection)
    }

    fn select_lun(
        &mut self,
        lun: u8,
        output: &Path,
        config: &FlashConfig,
        sources: &mut Sources,
        checkpoint: &impl Fn(&'static str) -> Result<()>,
    ) -> Result<()> {
        checkpoint("package program XML")?;
        let filename = format!("rawprogram{lun}.xml");
        let source =
            super_image::package_path(&self.origin.root, &format!("IMAGES/{filename}"), sources)?;
        checkpoint("package program XML read")?;
        let mut xml = program_xml::read_root(&source, sources)?;
        ensure!(
            xml.name == "data" && xml.namespace.is_none(),
            "invalid rawprogram root"
        );
        let mut selected = Vec::new();
        let mut excluded = Vec::new();
        for node in program_xml::operations(&xml)? {
            checkpoint("package program operation")?;
            let mut entry = node.clone();
            ensure!(entry.name == "program", "unexpected package XML operation");
            ensure!(
                attr(&entry, "physical_partition_number")?.parse::<u8>()? == lun,
                "package XML LUN mismatch"
            );
            let label = entry.attributes.get("label").cloned().unwrap_or_default();
            let filename = attr(&entry, "filename")?.to_owned();
            let configured_exclusion = config
                .excluded_partitions_for_write
                .iter()
                .any(|partition| partition.eq_ignore_ascii_case(&label));
            if lun != 5 && configured_exclusion {
                excluded.push(ExcludedRange {
                    label: label.clone(),
                    start: range_start(attr(&entry, "start_sector")?, SECTOR_SIZE)?,
                    bytes: attr(&entry, "num_partition_sectors")?
                        .parse::<u64>()?
                        .checked_mul(SECTOR_SIZE)
                        .context("excluded partition size overflow")?,
                });
            }
            let mut reason = if lun == 5 {
                Some("protected LUN5")
            } else if label.eq_ignore_ascii_case("ocdt") {
                Some("preserve device OCDT")
            } else if configured_exclusion {
                Some("excludedPartitionsForWrite")
            } else if filename.is_empty() {
                Some("empty filename")
            } else {
                None
            };
            if reason.is_none() {
                match self.select_source(&mut entry, &label, &filename, sources, checkpoint)? {
                    Some(path) => {
                        checkpoint("package source selected")?;
                        entry.attributes.insert(
                            "filename".into(),
                            path.to_str().context("non-UTF8 package path")?.into(),
                        );
                        selected.push(entry);
                    }
                    None => reason = Some("optional secdata image absent"),
                }
            }
            self.decisions.push(SelectionEntry {
                lun,
                label,
                filename,
                selected: reason.is_none(),
                reason: reason.map(str::to_owned),
            });
        }
        if lun == 5 {
            return Ok(());
        }
        // Keep package order within each class.
        selected.sort_by_key(
            |entry| match entry.attributes.get("label").map(String::as_str) {
                Some("PrimaryGPT") => 0,
                Some("BackupGPT") => 2,
                _ => 1,
            },
        );
        xml.children = selected.into_iter().map(XMLNode::Element).collect();
        self.programs.push((output.join(filename), xml));

        checkpoint("package patch XML")?;
        let patch_path = super_image::package_path(
            &self.origin.root,
            &format!("IMAGES/patch{lun}.xml"),
            sources,
        )?;
        checkpoint("package patch XML read")?;
        let patch_xml = program_xml::read_root(&patch_path, sources)?;
        for node in program_xml::operations(&patch_xml)? {
            ensure!(
                attr(node, "physical_partition_number")?.parse::<u8>()? == lun,
                "patch LUN mismatch"
            );
            check_excluded_patch(node, &excluded, SECTOR_SIZE)?;
        }
        self.patches.push((patch_path, patch_xml));
        Ok(())
    }

    fn select_source(
        &mut self,
        entry: &mut Element,
        label: &str,
        filename: &str,
        sources: &mut Sources,
        checkpoint: &impl Fn(&'static str) -> Result<()>,
    ) -> Result<Option<PathBuf>> {
        if label == "super" {
            ensure!(self.super_bytes == 0, "ambiguous package super entries");
            ensure!(filename == "super.img", "unsupported segmented super entry");
            ensure!(
                matches!(attr(entry, "sparse")?, "true" | "1")
                    && attr(entry, "file_sector_offset")?.parse::<u64>()? == 0,
                "unsupported package super source"
            );
            self.super_bytes = attr(entry, "num_partition_sectors")?
                .parse::<u64>()?
                .checked_mul(SECTOR_SIZE)
                .context("package super size overflow")?;
            ensure!(self.super_bytes > 0, "package super has zero target span");
            return Ok(Some(self.super_path.clone()));
        }

        checkpoint("package source binding")?;
        let relative = format!("IMAGES/{filename}");
        if label == "secdata" {
            let path = super_image::relative_path(&relative)?;
            let path = self.origin.root.join(path);
            if matches!(fs::symlink_metadata(&path), Err(error) if error.kind() == std::io::ErrorKind::NotFound)
            {
                sources.expect_absent(path)?;
                return Ok(None);
            }
        }
        let path = super_image::package_path(&self.origin.root, &relative, sources)?;
        if label == "userdata" && attr(entry, "num_partition_sectors")? == "0" {
            ensure!(
                attr(entry, "sparse")? == "true" && attr(entry, "file_sector_offset")? == "0",
                "unsupported zero-span userdata"
            );
            checkpoint("package userdata open")?;
            let image =
                crate::sparse::parse_controlled(&mut sources.open(&path)?, SECTOR_SIZE, &|| {
                    checkpoint("package sparse selection")
                })?;
            ensure!(image.expanded_bytes > 0, "empty userdata image");
            entry.attributes.insert(
                "num_partition_sectors".into(),
                (image.expanded_bytes / SECTOR_SIZE).to_string(),
            );
        }
        Ok(Some(path))
    }

    fn evidence(&self) -> Result<Evidence> {
        let patch_operations = self
            .patches
            .iter()
            .map(|(_, root)| {
                Ok(program_xml::operations(root)?
                    .iter()
                    .map(|op| {
                        op.attributes
                            .iter()
                            .map(|(key, value)| (key.clone(), value.clone()))
                            .collect()
                    })
                    .collect())
            })
            .collect::<Result<_>>()?;
        Ok(Evidence {
            profile: PROFILE.into(),
            nv_id: self.origin.nv_id.clone(),
            project: self.origin.project,
            definition: self.origin.definition.clone(),
            programs: paths(&self.programs),
            patches: paths(&self.patches),
            raw_size: "file".into(),
            decisions: self.decisions.clone(),
            patch_operations: Some(patch_operations),
        })
    }

    fn candidates(
        &self,
        sources: &mut Sources,
        checkpoint: &impl Fn(&'static str) -> Result<()>,
    ) -> Result<Vec<Candidate>> {
        let mut candidates = Vec::new();
        for (files, patch) in [(&self.programs, false), (&self.patches, true)] {
            for (path, expected) in files {
                checkpoint("prepared package XML")?;
                let root = if patch {
                    // The original patch document was read while checking exclusions.
                    expected.clone()
                } else {
                    require_output_file(path)?;
                    let root = program_xml::read_root(path, sources)?;
                    program_xml::require_same_operations(expected, &root)
                        .with_context(|| format!("prepared XML {}", path.display()))?;
                    root
                };
                let entries = program_xml::parse_root(
                    &root,
                    path,
                    patch,
                    SECTOR_SIZE,
                    RawSize::File,
                    sources,
                    checkpoint,
                )
                .with_context(|| format!("package XML {}", path.display()))?;
                candidates.extend(entries);
            }
        }
        let super_candidate = candidates
            .iter()
            .find(|c| c.label.as_deref() == Some("super"))
            .context("package profile requires executable super")?;
        ensure!(
            matches!(&super_candidate.operation,
                program_xml::Operation::Program { span_bytes, transfer_bytes, sparse: Some(_), .. }
                if *span_bytes == self.super_bytes && *transfer_bytes > 0),
            "package super must cover its declared target span"
        );
        Ok(candidates)
    }
}

fn paths(files: &[(PathBuf, Element)]) -> Vec<PathBuf> {
    files.iter().map(|(path, _)| path.clone()).collect()
}

fn require_output_file(path: &Path) -> Result<()> {
    let canonical = path
        .canonicalize()
        .with_context(|| format!("missing prepared file {}", path.display()))?;
    ensure!(
        canonical.as_os_str() == path.as_os_str(),
        "relocated prepared file binding: {}",
        path.display()
    );
    Ok(())
}

fn require_evidence(manifest: &Path, expected: &Evidence, sources: &mut Sources) -> Result<()> {
    require_output_file(manifest)?;
    let observed: Evidence = serde_json::from_reader(sources.open(manifest)?)
        .context("invalid or incomplete selection.json evidence")?;
    let same_paths = |observed: &[PathBuf], expected: &[PathBuf]| {
        observed
            .iter()
            .map(|path| path.as_os_str())
            .eq(expected.iter().map(|path| path.as_os_str()))
    };
    ensure!(
        observed.profile == expected.profile
            && observed.nv_id == expected.nv_id
            && observed.project == expected.project
            && observed.definition.as_os_str() == expected.definition.as_os_str()
            && same_paths(&observed.programs, &expected.programs)
            && same_paths(&observed.patches, &expected.patches)
            && observed.raw_size == expected.raw_size
            && observed.decisions == expected.decisions
            && observed
                .patch_operations
                .as_ref()
                .is_none_or(|ops| Some(ops) == expected.patch_operations.as_ref()),
        "selection.json does not match the original package, selected operations or output paths"
    );
    Ok(())
}

fn write_evidence(
    manifest: &Path,
    evidence: Evidence,
    candidates: &[Candidate],
    report: &mut Report,
) -> Result<()> {
    let order: Vec<_> = candidates
        .iter()
        .filter_map(|candidate| match &candidate.operation {
            program_xml::Operation::Program {
                source,
                span_bytes,
                transfer_bytes,
                ..
            } => Some(json!({
                "lun": candidate.lun, "label": candidate.label,
                "filename": source.file_name().and_then(|name| name.to_str()),
                "span_bytes": span_bytes, "transfer_bytes": transfer_bytes,
            })),
            _ => None,
        })
        .collect();
    let mut evidence = serde_json::to_value(evidence)?;
    evidence["program_order"] = json!(order);
    evidence["device_validated"] = json!(false);
    evidence["note"] = json!(
        "Offline preparation only; does not validate device project, cloud loader, digest, or signature."
    );
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(manifest)?;
    report.output(manifest.to_owned(), "preparation", OutputState::Partial);
    serde_json::to_writer_pretty(&mut file, &evidence)?;
    file.sync_all()?;
    report.output(manifest.to_owned(), "preparation", OutputState::Complete);
    Ok(())
}

pub fn prepare(
    root: &Path,
    nv_id: &str,
    output: &Path,
    config: Option<&Path>,
    sector: u64,
) -> Result<Prepared> {
    prepare_input(
        &PackageInput {
            root: root.into(),
            nv_id: nv_id.into(),
            config: config.map(Path::to_owned),
        },
        &PackageArtifacts::Generate(output.into()),
        sector,
        &|_| Ok(()),
        &mut Report::new(None, None),
    )
}

pub fn reopen(
    root: &Path,
    nv_id: &str,
    output: &Path,
    config: Option<&Path>,
    sector: u64,
) -> Result<Prepared> {
    prepare_input(
        &PackageInput {
            root: root.into(),
            nv_id: nv_id.into(),
            config: config.map(Path::to_owned),
        },
        &PackageArtifacts::Reopen(output.into()),
        sector,
        &|_| Ok(()),
        &mut Report::new(None, None),
    )
}

pub(crate) fn prepare_input(
    input: &PackageInput,
    artifacts: &PackageArtifacts,
    sector: u64,
    checkpoint: &impl Fn(&'static str) -> Result<()>,
    report: &mut Report,
) -> Result<Prepared> {
    let check = || checkpoint("package preparation");
    ensure!(
        sector == SECTOR_SIZE,
        "SFP package profile requires 4096-byte sectors"
    );
    ensure!(
        !input.nv_id.is_empty() && input.nv_id.bytes().all(|b| b.is_ascii_digit()),
        "invalid NV ID"
    );
    check()?;
    let mut sources = Sources::default();
    let (output, generate) = match artifacts {
        PackageArtifacts::Generate(output) => {
            let name = output
                .file_name()
                .context("missing prepare output directory name")?;
            let parent = output
                .parent()
                .filter(|path| !path.as_os_str().is_empty())
                .unwrap_or(Path::new("."));
            let path = sources.directory(parent)?.join(name);
            ensure!(
                !path.try_exists()?,
                "prepare output must be a new directory"
            );
            (path, true)
        }
        PackageArtifacts::Reopen(output) => (
            sources
                .directory(output)
                .context("open prepared output directory")?,
            false,
        ),
    };
    let selection = Selection::reconstruct(input, &output, &mut sources, checkpoint)?;
    let manifest = output.join("selection.json");
    let evidence = selection.evidence()?;
    if !generate {
        require_evidence(&manifest, &evidence, &mut sources)?;
        require_output_file(&selection.super_path)?;
    }
    check()?;
    let layout = super_image::plan(
        &selection.origin.root,
        &selection.origin.definition,
        &mut sources,
        &check,
    )?;
    ensure!(
        layout.size() == selection.super_bytes,
        "selected super span differs from the selected definition"
    );
    if generate {
        sources.check()?;
        check()?;
        fs::create_dir(&output).context("prepare output must be a new directory")?;
        for (path, root) in &selection.programs {
            check()?;
            let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
            report.output(path.clone(), "program_xml", OutputState::Partial);
            root.write(&mut file)?;
            file.sync_all()?;
            report.output(path.clone(), "program_xml", OutputState::Complete);
        }
        layout.build(&selection.super_path, &mut sources, checkpoint, report)?;
    } else {
        layout.validate(&selection.super_path, &mut sources, &check)?;
    }
    let candidates = selection.candidates(&mut sources, checkpoint)?;
    sources.check()?;
    check()?;
    if generate {
        write_evidence(&manifest, evidence, &candidates, report)?;
        sources.open(&manifest)?;
    }
    let summary = Preparation {
        programs: paths(&selection.programs),
        patches: paths(&selection.patches),
        origin: selection.origin,
        manifest: manifest.clone(),
        super_image: selection.super_path,
        exclusions: selection
            .decisions
            .into_iter()
            .filter(|entry| !entry.selected)
            .collect(),
    };
    Ok(Prepared {
        candidates,
        manifest,
        project: summary.origin.project,
        summary,
        sources,
    })
}
