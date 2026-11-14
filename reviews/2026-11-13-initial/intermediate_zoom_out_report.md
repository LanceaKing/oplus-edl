# Stage 8: Intermediate Zoom Out Review

**Target:** `docs/designs/oplus-edl.md`, lines 1–237.
**Reviewer:** `egm-zoom-out` (`/root/stage8_zoom_out`), completed in a fresh parent-turn context.
**Standard:** `zoom-out-standard.md`.
**Result:** No new findings. Three Major behavior gaps from earlier reviews remain; none requires a new subsystem or dependency strategy.

## Confirmations of earlier findings

### Z8-01 — Major — Verification mode contract

**Confirms:** C6-01. **Target:** lines 111–115, 174, 180, 210.

`best-effort` and `required` lack distinct behavior. Define how each handles unavailable readback, unknown expected contents, continuation, and final results. Use the same verification path. Do not add a patch simulator or capability database to make `required` work. Removing that mode until its policy is defined is also a small remedy.

### Z8-02 — Major — Stop later modifying operations after failure

**Confirms:** D7-01. **Target:** lines 99–113, 145, 170–171, 209–210.

Preserving protocol errors and removing implicit resets does not define the handling of remaining operations. A failed or unknown modifying operation should stop later writes, patches, boot-unit changes, and the requested reboot. Preserve completed results and mark unattempted operations. Define verification refusal separately through Z8-01. Existing restore and flash workflows can own this policy; no general executor, journal, or recovery framework is required.

### Z8-03 — Nit — Default storage-unit selection

**Confirms:** C6-02. **Target:** lines 83, 205, 207.

State what omitting `--lun` selects. If it selects all discovered units, connect that rule to reported backup coverage. This is one selection rule in the storage layer.

### Z8-04 — Nit — Dependency naming

**Confirms:** T7-01. **Target:** lines 74–76, 159, 167.

Use qdlrs consistently for the selected dependency, or define its qdl crate once. No additional adapter is needed.

### Z8-05 — Nit — Interoperability claim

**Confirms:** T7-02. **Target:** line 129.

State the ordinary raw-image format choice without claiming all external tools' naming and directory compatibility. No new interoperability matrix should become a release requirement.

### Z8-06 — Nit — Factual file-length limit

**Confirms:** T7-04. **Target:** line 44.

State that length and partition-size checks do not establish whether an image came from a complete backup. No architectural change follows.

## Contests and narrowed remedies

### Z8-07 — Major — Mapping needs a rule, not a general identity system

**Confirms and narrows:** D7-02. **Target:** lines 85–95, 168–170, 208.

Preventing overwrite does not establish how a renamed file maps to a partition. Multi-LUN disambiguation does not resolve duplicate names within one LUN.

Define the ordinary directory convention and reject ambiguous mappings before writing. For names that cannot safely round-trip, use a small reversible convention or report unsupported entries and require an explicit, unambiguous target. A discriminator is needed only if duplicate-name partitions are supported in the first release. Do not restore manifest or source-layout proof requirements.

### Z8-08 — Nit — Missing rationale does not overturn the architecture

**Contests:** S7-01's architectural Major rating. **Agrees with:** D7-03's Nit rating. **Target:** lines 127–128; context at 11, 79, 201.

The alternatives table needs reasons for rejecting a wrapper and preferring the separate fork. The dependency choice and ownership boundary are already coherent. Separate the new-stack and wrapper alternatives, record the wrapper costs already identified in the brief, and add the actual fork rationale. Do not invent upstream limitations or reopen the approved choice. Structural standard compliance and architectural suitability are separate judgments.

### Z8-09 — Nit — Approval wording is an optional edit

**Narrows:** T7-03. **Target:** lines 36–51.

The precedence rule serves a purpose because the historical brief is broader. Paragraph 51 can be shortened or removed because lines 36–49 already establish precedence. This is optional editing, not an architectural acceptance condition. Preserve an explicit rule that current scope supersedes conflicting historical requirements.

## Effect on Stage 5

The synchronous design, bounded buffers, thin adapter, and protocol fixes in the fork remain suitable. Later findings qualify implementation readiness without overturning those choices. Storage and restore can own mapping, restore and flash can own stopping policy, verification can own comparison outcomes, and output can own reporting.

## Read inventory and limits

The reviewer reported complete reads of the persona, standard, target (237 lines), concept brief (207 lines), and all three preceding stage reports. Follow-up reads recovered truncated initial output. No access failures occurred. A local inventory found no production code or reusable implementation. The unrelated draft and review-scope file were not opened.

No external sources, history, or memory were consulted. Upstream behavior, licenses, API suitability, and claimed defects remain unverified. No design files were changed, builds run, devices accessed, or nested agents used.
