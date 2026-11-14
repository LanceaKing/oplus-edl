# Independent Re-review: Rust Oplus EDL

**Decision:** The revised design passes the complete document review within the supplied evidence. All five previous Major issues are closed as design-contract gaps. No new or residual Blocker, Major, or Nit was identified. No contradiction or expansion beyond the approved first-release scope was found.

**Target:** [docs/designs/oplus-edl.md](/Users/Apple/Documents/oplus-edl/reviews/2026-11-13-rereview/design_snapshot.md), 375 lines.
**Date:** 2026-11-13.
**Mode:** Uninterrupted, all five EGM stages, nine fresh-context reviewer runs.
**Snapshot SHA-256:** `92f93bdde0f8c9874973116fb1285c701d4f7bdf3326782e3154d18403ebfc6a`.
**Reviewed ranges:** FeatureRange 1–151; DetailedRange 152–375.

This result establishes document readiness, not successful implementation, compilation, or hardware operation. Upstream source behavior and interface suitability were not independently checked during this re-review.

## Five-issue closure and traceability

The original issue definitions come from the [first consolidated report](/Users/Apple/Documents/oplus-edl/reviews/2026-11-13-initial/consolidated_review_report.md). Its line references belong to the earlier snapshot. The evidence below uses current lines.

| Prior issue | Disposition | Current design evidence | Independent checks |
| --- | --- | --- | --- |
| **A1: Verification modes** | **Closed** | [Mode rules](/Users/Apple/Documents/oplus-edl/reviews/2026-11-13-rereview/design_snapshot.md:114), lines 114–130; result rules 281–296; tests 352–359. The modes distinguish preflight rejection, read refusal, loss of session synchronization, mismatch, disabled verification, continuation, and final actions. | Both Zoom Out runs, design critic, implementation readiness, and codebase-details review support closure. |
| **A2: Stop policy** | **Closed** | [Stopping rules](/Users/Apple/Documents/oplus-edl/reviews/2026-11-13-rereview/design_snapshot.md:108), lines 108–112; failure type 169; mappings 283; assertions 186 and 351–357. Failed or unconfirmed modifications stop queued writes, patches, boot-unit changes, and reboot. Completed results remain recorded. | Architectural and implementation lenses agree. Tests can assert absence of later modifying or reset commands. |
| **A3: Directory mapping** | **Closed** | [Mapping contract](/Users/Apple/Documents/oplus-edl/reviews/2026-11-13-rereview/design_snapshot.md:241), lines 241–259; workflow tests 215. Naming, collisions, LUNs, layouts, GPT captures, partial files, explicit images, and preflight rejection have specified behavior. | Design, architecture, and readiness reviews accept rejection of unsupported names as the scoped solution. |
| **A4: Public results** | **Closed** | [Result contract](/Users/Apple/Documents/oplus-edl/reviews/2026-11-13-rereview/design_snapshot.md:261), lines 261–322. Fields, execution, confirmation, verification, aggregate priority, skipped work, errors, and a JSON example are defined. | Readiness and style reviews confirm that mixed outcomes can be implemented and tested without inventing a new public result contract. |
| **A5: Offline fault injection** | **Closed as a documented test boundary** | [Test boundary](/Users/Apple/Documents/oplus-edl/reviews/2026-11-13-rereview/design_snapshot.md:173), lines 173–188; helper and test paths 214 and 229–232. The plan specifies scripted transport, independent file-I/O faults, command observations, session construction, selection tests, and cleanup checks. | Readiness and codebase-details reviews close the former documentation gap. They do not independently confirm the cited upstream traits or prove that the planned tests compile. |

No partial or unresolved A1–A5 design item remains in this review. The A5 evidence qualification is not an additional design finding and does not reverse the documented boundary decision.

## New contradictions and scope

No new findings were raised. The reviews specifically challenged these possible expansions:

- Final-content verification could have become a patch simulator. The document instead defines overwrite order and leaves unknown contents unverified, with mode-specific behavior.
- Confirmation reporting could have become a general execution framework. The failure type and result object remain limited to one invocation and the required stopping policy.
- Directory naming could have become a partition-identity system. The document chooses explicit mapping and rejection without requiring provenance or layout proof.
- Fault testing could have introduced a production transport framework. The design proposes use of existing interfaces and test-only scripted implementations.

The design does not restore manifests, journals, saved approval plans, automatic resume, identity proofs, resource databases, patch simulation, plugins, fastboot integration, or mandatory RMX3800 support. The added detail supports functions already retained in the first release. The product still owns workflows and reporting; the fork owns protocol correctness and required interfaces.

## Prior secondary comments

| Prior action | Disposition | Current evidence |
| --- | --- | --- |
| N1: Default LUN selection | Resolved | 82 and 328 state that omission selects all discovered units. |
| N2: Alternatives rationale | Resolved | 144–146 separate the new stack, external wrapper, and vendor alternatives and give concrete reasons. |
| N3: Dependency name | Resolved | 12, 73, 78 consistently identify qdlrs. |
| N4: Interoperability claim | Resolved | 147 states a format choice without claiming tested directory compatibility. |
| N5: Repeated approval wording | No further action required | 35–50 retain useful precedence over the historical brief. Further shortening was optional. |
| N6: Judgmental completeness wording | Resolved | 45 states the factual limit of size checks. |
| N7: Sparse-write wording | Resolved | 210 and 359 distinguish RAW/FILL writes from DONT_CARE skips. |
| N8: Expected verification contents | Resolved | 211 points to the patch and overlap rules at 116–128 and 281. |
| N9: Short I/O and sector padding | Resolved | 350 rejects incomplete transfers; 359 preserves permitted tail padding. |
| N10: Intermediate phase checkpoints | Resolved | 339–346 require builds and available tests while incomplete write entry points return unsupported. |

## Stage outcomes

| Stage | Outcome | New or residual findings |
| --- | --- | --- |
| 5: Initial Zoom Out | Architectural pass | 0 |
| 6: Comprehension | Proposal and existing-system claims could be paraphrased; no clarity gap found | 0 |
| 7: Critic | Design, structural, and style lenses pass | 0 |
| 8: Intermediate Zoom Out | Earlier closure conclusions survived the scope and simplicity challenge | 0 |
| 9: Implementation Readiness | Document readiness passes; local codebase checks remain evidence-limited | 0 |

No raw finding IDs were assigned in this re-review because no reviewer reported a finding. There are no new action items to deduplicate. The A1–A5 and N1–N10 tables above provide traceability for the earlier report's actions.

## Execution and isolation

| Native reviewer run | Persona | Standard | Status |
| --- | --- | --- | --- |
| `/root/r2_stage5_zoom` | Zoom Out | Zoom Out | Completed |
| `/root/r2_stage6_comprehension` | Comprehension | None | Completed |
| `/root/r2_stage7_design` | Design Critic | Feature Specification | Completed |
| `/root/r2_stage7_structural` | Structural | Feature Specification | Completed |
| `/root/r2_stage7_style` | Style | None | Completed |
| `/root/r2_stage8_zoom` | Zoom Out | Zoom Out | Completed |
| `/root/r2_stage9_readiness` | Implementation Readiness | Implementation Readiness | Completed |
| `/root/r2_stage9_codebase` | Codebase Details | Implementation Readiness | Completed within local evidence |
| `/root/r2_stage9_style` | Style | None | Completed |

All nine runs were dispatched using `fork_turns: "none"`; actual completion reports were received. Each reviewer reported complete reads of its persona, required standard where supplied, and permitted documents. Truncated tool output was recovered with bounded reads. No required local read remained missing.

The only automatic reading context was the target's local links to the concept brief and the original consolidated report. Stage 6 read the target alone. Stage 8 additionally read this run's preceding reports, as required by the workflow. Other reports, unrelated drafts, recursive links, conversation history, and memory were excluded.

Fresh parent-turn context does not establish exclusion of host instruction files; that was not separately tested. Read inventories are reviewer attestations, not a separate audit of every child tool trace.

## Evidence limits and unchanged files

The local inventory contains documents and review artifacts, with no production source, Cargo files, or test infrastructure. The design expressly identifies these files as future work, so their absence is not a defect.

The user supplied no additional context. External URLs were excluded from the review reading set. The document records earlier source inspections, but this re-review did not repeat those inspections. Upstream defects, exact signatures and generic bounds, USB/serial integration, dependency compatibility, licenses, host drivers, and hardware support remain outside the independently verified evidence.

No design or pre-existing report was changed. No implementation files were created, builds or code tests run, devices operated, or forks created. Only this re-review's report artifacts were written. Snapshot hashes and the excluded-link inventory are in [review_scope.json](/Users/Apple/Documents/oplus-edl/reviews/2026-11-13-rereview/review_scope.json).

## Reports

- [Initial Zoom Out](/Users/Apple/Documents/oplus-edl/reviews/2026-11-13-rereview/zoom_out_report.md)
- [Comprehension, including the complete reviewer paraphrase](/Users/Apple/Documents/oplus-edl/reviews/2026-11-13-rereview/comprehension_report.md)
- [Design, structural, and style review](/Users/Apple/Documents/oplus-edl/reviews/2026-11-13-rereview/critic_report.md)
- [Intermediate Zoom Out](/Users/Apple/Documents/oplus-edl/reviews/2026-11-13-rereview/intermediate_zoom_out_report.md)
- [Implementation readiness, codebase details, and style](/Users/Apple/Documents/oplus-edl/reviews/2026-11-13-rereview/implementation_readiness_report.md)
