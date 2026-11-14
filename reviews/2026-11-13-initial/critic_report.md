# Stage 7: Critic Review

**Target:** `docs/designs/oplus-edl.md`, active lines 1–133; all reviewers read lines 1–237 to check for answers elsewhere.
**Context:** `concepts_brief.md`, lines 1–207.
**Result after stage deduplication:** 0 Blockers, 3 Majors, 4 Nits. The alternatives item retains the structural reviewer's Major rating here; its raw ratings differ.

## Reviewer status

| Reviewer | Agent | Status | Raw findings |
| --- | --- | --- | --- |
| Design critic | `/root/stage7_design` | Completed | 2 Major, 1 Nit |
| Structural | `/root/stage7_structural` | Completed | 1 Major |
| Style | `/root/stage7_style` | Completed | 4 Nit |

Each reviewer used a fresh parent-turn context and reported complete reads of its persona, the entire target, and the context. Design and structural reviewers also read `feature-spec-standard.md`; no separate style standard was supplied. No required reads failed. External references were not opened, so upstream behavior remains unverified. No design edits, builds, or device operations occurred.

## Prioritized findings

### D7-01 — Major — Define when a failed write stops the batch

**Lens:** Design. **Target:** lines 99–113.

The design orders program and patch operations and permits explicit boot-unit selection and reboot. It distinguishes a missing write acknowledgement from a confirmed failure and prohibits automatic rewriting. It does not say whether either result stops the remaining operations.

If an early program operation sends data but loses its final acknowledgement while more programs, patches, and `--reboot` remain queued, continuing can modify more storage despite an uncertain session state or reboot an incomplete image set. Reporting the first operation as unknown does not prevent those effects. The later dependency fixes preserve errors and remove automatic resets but do not define the product batch policy. The concept brief requires stopping when a write result is unknown.

**Minimal change:** State that a failed or unknown write stops subsequent writes, patches, boot-unit changes, and automatic execution of the requested reboot. Preserve completed results and identify unattempted operations. Separately state whether readback refusal under `best-effort` allows execution to continue.

### D7-02 — Major — Preserve partition identity through filename changes

**Lens:** Design. **Target:** lines 85–95.

Backup uses `lun<N>/<partition>.bin` and changes names to handle path separators or duplicates. Restore matches images to partition names. The design does not define how restore recovers partition identity after a filename changes.

Two names can produce the same safe filename, or two partitions within one LUN can have the same name. A suffix can prevent backup overwrite without telling restore which partition was intended. Multi-LUN disambiguation does not resolve duplicates within one LUN.

**Minimal change:** Define one shared convention for backup and restore, with reversible name escaping and a partition discriminator where needed. Define how it is matched against current GPT data and reject ambiguity. A plain directory convention is sufficient; no manifest is required.

### S7-01 / D7-03 — Major / Nit — Record concrete alternatives trade-offs

**Lenses:** Structural and design. **Target:** lines 127–128.

The table combines building a protocol stack and wrapping an external EDL program. Reducing repeated protocol work explains rejecting a new stack but not rejecting a wrapper. The vendored-qdl row records the selected fork without explaining the technical or scope trade-off. The feature specification standard requires specific rejection reasons. The concept brief already names runtime packaging and error reporting as wrapper costs.

**Minimal change:** Separate the reasons for rejecting a new stack and an external wrapper. Add a concrete reason for preferring the separate Rust fork over vendored qdl. Do not invent upstream limitations or reopen the approved choice.

**Rating difference:** Structural rated this Major for standard compliance; design rated it Nit because a short rationale can resolve it without changing behavior.

### T7-01 — Nit — Name the dependency consistently

**Lens:** Style. **Target:** lines 74–76.

The diagram names a qdl adapter although qdlrs and qdl are separate references in the table.

**Minimal change:** Use `薄 qdlrs 适配层`, or define `qdlrs 的 qdl crate` once if the crate name is intended.

### T7-02 — Nit — Narrow the interoperability claim

**Lens:** Style. **Target:** line 129. **Action:** CALIBRATE.

The claim that images can be used directly with existing tools does not identify a tested exchange path. Ordinary raw-image format does not establish directory naming or every tool's input compatibility.

**Minimal wording:** `采用普通原始分区镜像，不强制专用格式。`

### T7-03 — Nit — Remove repeated approval commentary

**Lens:** Style. **Target:** lines 36 and 51. **Action:** DROP.

Line 36 states precedence and lines 38–49 record the scope changes. Line 51 repeats these decisions and commentary about unresolved disputes without adding an implementation or operator decision.

**Reviewer proposal:** Remove `不再重复要求确认已经批准的取舍` from line 36 and paragraph 51. Preserve the precedence rule, scope table, and historical status of the original brief. No edits were applied.

### T7-04 — Nit — State the file-length limit without judgment

**Lens:** Style. **Target:** line 44.

`不假装知道` suggests misleading intent where the intended requirement is a factual limit.

**Minimal wording:** `按文件长度与目标分区大小检查；不据此判断镜像是否来自完整备份。`

## Structural checks with no findings

The active range has one H1, six H2 sections, and five H3 workflow sections under the technical approach. Tables, lists, links, and the fenced diagram are structurally valid. The supplied standard does not prescribe exact heading names or order. The later limitations section supplies relevant context and was not treated as missing merely because it lies beyond line 133.

## Scope guard

The findings concern retained first-version behavior. They do not require manifests, saved plans, automatic resume, or a firmware proof system. All reviewers accepted explicit current scope changes over the historical concept brief.
