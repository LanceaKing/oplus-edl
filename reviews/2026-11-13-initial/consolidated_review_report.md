# Independent Review: Rust Oplus EDL

**Decision:** The architecture fits the approved first release, but the document does not yet pass implementation readiness. Resolve five Major contract or testability gaps before the affected implementation work. Ten Nits cover wording, rationale, and phase checkpoints. No Blocker was found within the reviewed evidence.

**Target:** [docs/designs/oplus-edl.md](/Users/Apple/Documents/oplus-edl/reviews/2026-11-13-initial/design_snapshot.md), 237 lines.
**Date:** 2026-11-13.
**Mode:** Uninterrupted, all five EGM review stages.
**Snapshot SHA-256:** `17702d7a38ee19e47968240a80df7d52327ae07934ff9a97b57cffa78abba822`.
**Change status:** The design and concept brief were not edited. This review created reports only.

## Scope and evidence

FeatureRange was lines 1–133. DetailedRange was lines 134–237, beginning at `详细实现计划`. The only added reading context was the target's local link to `concepts_brief.md`. The target's explicit scope changes take precedence over that historical brief. Stage 6 read the target alone; Stage 8 also read the preceding reports, as the workflow requires.

Nine reviewer runs completed using native subagents with `fork_turns: "none"`. Each returned a read-completion inventory; no required local read was reported missing. Parent-turn isolation was requested through the tool. Exclusion of host instruction files was not independently tested. Read inventories are reviewer attestations, not a separate audit of every child tool trace.

The user selected no additional context. External GitHub links were left outside the reading set, and links in the concept brief were not followed recursively. No upstream checkout or local production implementation was available. Thus this is a design review, with limited local codebase verification. It does not independently establish upstream defects, API suitability, licenses, build compatibility, or device support. The excluded URLs and snapshot metadata are recorded in [review_scope.json](/Users/Apple/Documents/oplus-edl/reviews/2026-11-13-initial/review_scope.json).

## Stage outcomes

| Stage | Result | Finding treatment |
| --- | --- | --- |
| 5: Initial Zoom Out | Architecture passes within scope | No findings |
| 6: Comprehension | Proposal can be paraphrased; two contract ambiguities remain | 1 Major, 1 Nit |
| 7: Critic | Retained behavior needs clearer failure and mapping rules | 8 raw findings; 7 after stage deduplication |
| 8: Intermediate Zoom Out | Architecture remains suitable; no new subsystem is justified | 9 confirmations or narrowed interpretations; no new issue |
| 9: Implementation Readiness | Does not yet pass | 4 Major, 4 Nit; upstream source comparison remains limited |

The final count is **5 Major and 10 Nit action items**, deduplicated from 27 raw finding records. Repeated Stage 8 confirmations are not additional defects. S7-01's Major rating is reduced to Nit in this synthesis for the reason recorded below.

## Major actions

### A1 — Define the three verification modes

**Target:** [lines 111–115](/Users/Apple/Documents/oplus-edl/reviews/2026-11-13-initial/design_snapshot.md:111), 174, 180, 209–210, 221, 225.
**Sources:** C6-01, Z8-01, I9-01.

The document exposes `best-effort|required|none` without defining distinct execution and result rules. When every write is acknowledged but readback is refused or a patch leaves the expected bytes unknown, an implementer must decide what `required` promises. The handling of later operations, boot-unit changes, and reboot is also unclear.

**Smallest useful change:** Add one mode table covering whether verification runs, unknown expected contents, read failures, mismatches, final status, and whether remaining operations or explicit final actions may proceed. Use the existing verification path and output module. Do not add a patch simulator or capability database.

**Done when:** `tests/verification.rs` can derive expected outcomes for all three modes, including read refusal, unknown patched contents, mismatch, and explicit final actions, without making new policy choices.

### A2 — Carry the stop policy into the current design and cover remaining actions

**Target:** [lines 99–113](/Users/Apple/Documents/oplus-edl/reviews/2026-11-13-initial/design_snapshot.md:99), 145, 170–171, 209–210.
**Sources:** D7-01, Z8-02; qualified by the Stage 9 readiness review.

The target preserves errors, forbids automatic rewriting, and removes implicit resets. It does not explicitly say what happens to queued programs, patches, boot-unit changes, and a requested reboot after a failed or unknown modifying operation. Losing the final ACK on an early write is the concrete case: continuing could modify more storage or reboot an incomplete image set.

The concept brief already requires stopping on unexpected storage responses. This finding therefore concerns carrying the rule into the operative design and defining its coverage; it is not a claim that no stopping policy exists anywhere in the supplied context. The Stage 9 reviewer relied on the brief and did not report this as a new finding.

**Smallest useful change:** State that failed or unknown modifying operations stop later modifying operations and the queued reboot. Preserve completed item results and mark remaining items unattempted. Define readback refusal separately under A1, rather than treating it as a missing write ACK.

**Done when:** An offline sequence with a lost final ACK proves that no later program, patch, boot-unit command, or reset is emitted. The result preserves the uncertain write and identifies unattempted items.

### A3 — Define a backup directory that restore can map unambiguously

**Target:** [lines 85–95](/Users/Apple/Documents/oplus-edl/reviews/2026-11-13-initial/design_snapshot.md:85), 169–170, 178, 207–208.
**Sources:** D7-02, Z8-07, I9-02.

The ordinary layout is defined, but handling path separators and duplicate names can change filenames without defining the reverse mapping. Saving primary and backup GPT data also needs a naming and exclusion rule so directory restore does not treat those captures as partition images. Multi-LUN handling alone does not resolve duplicate names within a LUN.

**Smallest useful change:** Specify ordinary filenames, escaping or rejection rules, collision behavior, GPT capture locations, restore candidates, and the interaction between nested LUN directories and `--lun`. Reject ambiguous targets before writing. Support for duplicate-name partitions may use a small discriminator, but a general partition-identity system is not required. Reporting names that cannot be mapped as unsupported is also a valid scoped choice.

**Done when:** Round-trip cases cover ordinary names, collisions, unsafe names, the same name across LUNs, and GPT captures. Each accepted image maps to one current target; rejected cases perform no write. No manifest or source-layout proof is introduced.

### A4 — Specify the public result fields and aggregation rules

**Target:** [lines 113–115](/Users/Apple/Documents/oplus-edl/reviews/2026-11-13-initial/design_snapshot.md:113), 164, 175, 180–181, 211, 221.
**Source:** I9-03.

The design promises JSON parity with text and distinct exit categories, but it leaves fields, permitted item states, and the overall outcome rule undefined. Mixed successful and failed backup items, or acknowledged writes with unverified ranges and skipped patches, need a deterministic command result.

**Smallest useful change:** Add one JSON example and a compact field/state table for target, item or operation, write result, verification result, skipped or unattempted work, and errors. Define how item outcomes map to the existing exit categories. Exact integer codes may remain implementation constants as already planned.

**Done when:** Workflow and CLI tests can assert the result for mixed-success backup, unavailable verification, mismatch, missing ACK, and skipped local patches without inventing the response contract. No separate schema suite is required.

### A5 — Identify the narrow boundary for offline fault injection

**Target:** [line 177](/Users/Apple/Documents/oplus-edl/reviews/2026-11-13-initial/design_snapshot.md:177), 192–194, 218.
**Source:** B9-01.

The plan names offline tests but not how they supply transport input, capture emitted commands, and cause timeouts or end-of-stream conditions. No local code resolves that choice. This is not evidence that qdlrs lacks a suitable interface: its source was not in the review context.

**Smallest useful change:** After inspecting the fixed qdlrs revision, document the existing interface to reuse or the minimal change needed. Name how tests inject responses, observe writes and resets, and deliver errors. Add an exact helper-file path to the allowlist only if one is needed.

**Done when:** The plan identifies a concrete test seam for short I/O, EOF, timeouts, ACK/NAK handling, reset suppression, and device selection. Do not design a general protocol framework merely to meet this requirement.

## Nits

| ID | Target lines | Action | Sources |
| --- | --- | --- | --- |
| N1 | 83, 205, 207 | State which LUNs are selected when `--lun` is omitted and relate that choice to reported backup coverage. | C6-02, Z8-03 |
| N2 | 127–128 | Separate the new-stack and external-wrapper alternatives. Add the actual wrapper and fork-versus-vendor trade-offs without reopening the approved choice or inventing upstream limitations. | D7-03, S7-01, Z8-08 |
| N3 | 74–76 | Name the selected dependency qdlrs consistently, or define its qdl crate once. | T7-01, Z8-04 |
| N4 | 129 | Narrow the interoperability sentence to the ordinary raw-image format choice; directory or naming compatibility has not been demonstrated. | T7-02, Z8-05 |
| N5 | 36–51 | Optionally shorten repeated approval commentary. Preserve the explicit precedence of the current design over the historical brief. | T7-03, Z8-09 |
| N6 | 44 | Replace `不假装知道` with a factual statement that size checks do not establish backup completeness. | T7-04, Z8-06 |
| N7 | 173 | Say RAW and FILL produce writes while DONT_CARE ranges are skipped. | T9-01 |
| N8 | 174 | Refer to known expected contents under the verification rules, rather than unconditional comparison with original source bytes. | T9-02 |
| N9 | 225 | Say incomplete transfers must not be reported complete; retain the allowed sector-boundary zero padding for shorter images. | T9-03 |
| N10 | 218–227 | Add a build/test completion condition to each intermediate product phase using the planned test files. | I9-04 |

N2 is a standard-compliance gap, but no evidence shows that the selected architecture is unsuitable. This synthesis adopts the design and Stage 8 reviewers' Nit rating instead of the structural reviewer's Major rating. N5 is optional editing, not an acceptance condition.

## Scope preserved

The review does not require a backup manifest, persistent plan, hash approval, journal, automatic resume, identity or layout proof, local-patch simulator, resource database, fastboot integration, or verified RMX3800 support. The fixes belong in the existing product and fork responsibilities. The absence of planned source files is not a defect: the workspace has no production implementation, as the document states.

The strongest objection to a broader remediation is that it would rebuild systems the author explicitly removed. The accepted remedies therefore specify only observable behavior, plain-directory mapping, and the narrow offline test boundary needed by retained functions.

## Traceability matrix

Every raw finding maps to an action below. Stage 8 records are confirmations or narrowed remedies rather than new defects.

| Raw ID | Original severity | Consolidated action | Treatment |
| --- | --- | --- | --- |
| C6-01 | Major | A1 | Retained |
| C6-02 | Nit | N1 | Retained |
| D7-01 | Major | A2 | Retained with the historical-brief qualification |
| D7-02 | Major | A3 | Remedy narrowed to mapping or explicit rejection |
| D7-03 | Nit | N2 | Merged with S7-01 |
| S7-01 | Major | N2 | Reduced to Nit; rationale gap does not invalidate the architecture |
| T7-01 | Nit | N3 | Retained |
| T7-02 | Nit | N4 | Retained; CALIBRATE |
| T7-03 | Nit | N5 | Optional wording edit; preserve precedence |
| T7-04 | Nit | N6 | Retained |
| Z8-01 | Major | A1 | Confirms C6-01 |
| Z8-02 | Major | A2 | Confirms D7-01 |
| Z8-03 | Nit | N1 | Confirms C6-02 |
| Z8-04 | Nit | N3 | Confirms T7-01 |
| Z8-05 | Nit | N4 | Confirms T7-02 |
| Z8-06 | Nit | N6 | Confirms T7-04 |
| Z8-07 | Major | A3 | Confirms D7-02 while narrowing the remedy |
| Z8-08 | Nit | N2 | Resolves the alternative-rationale severity difference |
| Z8-09 | Nit | N5 | Makes the deletion proposal optional |
| I9-01 | Major | A1 | Adds final-action cases and test expectations |
| I9-02 | Major | A3 | Adds GPT capture and restore-candidate rules |
| I9-03 | Major | A4 | Retained |
| I9-04 | Nit | N10 | Retained |
| B9-01 | Major | A5 | Retained as a design gap, not an upstream defect |
| T9-01 | Nit | N7 | Retained |
| T9-02 | Nit | N8 | Retained |
| T9-03 | Nit | N9 | Retained |

## Reviewer completion record

| Run | Persona | Standard | Status |
| --- | --- | --- | --- |
| `/root/stage5_zoom_out` | Zoom Out | Zoom Out | Completed |
| `/root/stage6_comprehension` | Comprehension | None | Completed; target only |
| `/root/stage7_design` | Design Critic | Feature Specification | Completed |
| `/root/stage7_structural` | Structural | Feature Specification | Completed |
| `/root/stage7_style` | Style | None | Completed |
| `/root/stage8_zoom_out` | Zoom Out | Zoom Out | Completed; preceding reports also supplied |
| `/root/stage9_readiness` | Implementation Readiness | Implementation Readiness | Completed |
| `/root/stage9_codebase` | Codebase Details | Implementation Readiness | Completed within local evidence; upstream audit not performed |
| `/root/stage9_style` | Style | None | Completed |

No reviewer failed or timed out. No design edits, builds, device operations, or fork creation occurred. The read-only upstream checks needed to settle A5 and verify the claimed fixes remain future work, not hidden evidence behind this report.

## Stage reports

- [Stage 5: Initial Zoom Out](/Users/Apple/Documents/oplus-edl/reviews/2026-11-13-initial/zoom_out_report.md)
- [Stage 6: Comprehension, including the complete paraphrase](/Users/Apple/Documents/oplus-edl/reviews/2026-11-13-initial/comprehension_report.md)
- [Stage 7: Design, structural, and style review](/Users/Apple/Documents/oplus-edl/reviews/2026-11-13-initial/critic_report.md)
- [Stage 8: Intermediate Zoom Out and contested findings](/Users/Apple/Documents/oplus-edl/reviews/2026-11-13-initial/intermediate_zoom_out_report.md)
- [Stage 9: Implementation readiness, codebase details, and style](/Users/Apple/Documents/oplus-edl/reviews/2026-11-13-initial/implementation_readiness_report.md)
