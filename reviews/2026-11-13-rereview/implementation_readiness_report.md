# Stage 9: Implementation Readiness Re-review

**Decision:** Pass within the permitted document evidence. No Blocker, Major, or Nit findings. A1–A5 now have implementable design answers. No R2-I9, R2-B9, or R2-T9 finding IDs are assigned.
**Target:** `docs/designs/oplus-edl.md`, active lines 152–375. All reviewers read the whole target.

## Reviewer status

| Reviewer | Native run | Result |
| --- | --- | --- |
| Implementation readiness | `/root/r2_stage9_readiness` | Completed; passes document readiness |
| Codebase details | `/root/r2_stage9_codebase` | Completed within local evidence; no additional findings |
| Style | `/root/r2_stage9_style` | Completed; no findings |

All runs used fresh parent-turn contexts. Each reviewer reported complete reads of its persona, target (375 lines), concept brief (207 lines), and original consolidated report (169 lines). Readiness and codebase reviewers also read `implementation-readiness-standard.md`. No style standard was supplied. No required read remained missing; bounded reads recovered initial truncation where needed.

## A1–A5 closure

| Previous action | Disposition | Current evidence and implementation assessment |
| --- | --- | --- |
| A1: Verification modes | Closed | 120–128, 217, 281–296, 348–361 define preflight, known and unknown contents, clean NAK, timeout, mismatch, disabled verification, continuation, final actions, and test outcomes. |
| A2: Stop policy | Closed | 110–112, 169, 186, 283, 296, 350–361 carry failure and uncertainty into typed reporting and command-record assertions. Later modifying commands and resets stop; completed results remain available. |
| A3: Directory mapping | Closed | 241–259 and 215 define names, collisions, GPT paths, partial files, layouts, LUN filtering, exact matching, and round-trip/rejection tests. Mapping failure requires no writes. |
| A4: Public results | Closed | 261–322 define fields, item states, confirmation, verification ranges, error placement, aggregate priority, skips, and a concrete example. Mixed outcomes can be tested without inventing a new response contract. |
| A5: Offline fault injection | Closed as a documented design boundary | 181–188, 214, 229–232 identify the test interfaces, scripted input and failures, shared read position, command capture through cleanup, protocol calls, independent source/sink faults, selection seam, and exact files. Upstream compatibility remains unverified in this review. |

## Readiness reasoning

The exact file boundary at lines 190–239 defines responsibilities and narrow adaptations. It excludes deferred manifest, journal, plan, executor, profile, and schema subsystems. Planned files are not defects because they do not yet exist.

The phases at lines 339–346 now require a build and the tests available at each checkpoint. Incomplete public write entry points return unsupported until write, stop, verification, and reporting behavior is connected. This closes the former checkpoint concern.

The acceptance cases at lines 348–361 give observable outcomes for partial I/O, missing ACK, NAK, refusal, timeout, uncertainty, mismatch, sparse data, failed mapping, mixed backup results, and final actions. Host checks are at 343 and 365. Device validation remains separate.

The readiness reviewer found no remaining substantive public behavior or API choice that blocks implementation. Internal helper names, representations, fixtures, and the explicitly deferred integer exit constants remain ordinary coding decisions.

## Confirmed local facts and evidence limits

The codebase reviewer inspected the complete local inventory, including hidden and ignored file names. The workspace contains documents and reports, but no production source, Cargo manifest, lockfile, or test infrastructure. This agrees with line 192. Other reports and drafts were seen only as filenames and were not read.

A5 now supplies an actual proposed test boundary rather than merely naming test cases. Scripted records remain accessible after destruction; they are test observations, not a new persistent job log. Separate helpers match the two repository boundaries. No supplied source establishes a reusable local alternative.

The reviewers did not independently check upstream trait visibility or generic bounds, construction of test sessions, the reported defects, integration of selection with USB/serial, existence of fork source paths, dependency resolution, licenses, builds, or hardware. These are limits of evidence, not claims that the interfaces are wrong or additional findings.

## Prior detailed style issues

| Previous issue | Disposition | Current evidence |
| --- | --- | --- |
| T9-01 / N7: sparse writes versus skips | Resolved | 210 and 359 distinguish RAW/FILL writes from DONT_CARE skips; 102 and 116 supply context. |
| T9-02 / N8: comparison against expected contents | Resolved | 211 refers to known contents and the patch/overlap rules at 116–128 and 281. |
| T9-03 / N9: incomplete I/O versus padding | Resolved | 350 rejects incomplete transfer success; 359 permits bounded sector padding, consistent with 92. |

The style reviewer found that execution, confirmation, and verification definitions at 275–296 are consistent when read with 110–130. Static evidence, future work, builds, and device validation are distinguished. No caveat needs DROP, CALIBRATE, or RELOCATE.

## Scope and operations

The added result contract, mapping rules, failure type, selection helper, and test transport support retained first-release behavior. They do not add saved plans, identity proofs, manifests, journals, resume, patch simulation, databases, fastboot, or mandatory RMX3800 support.

No reviewer read external sources, recursive links, other review reports, unrelated drafts, history, or memory. No files were changed, code built, tests run, devices operated, or nested agents created. This is a document readiness pass, not a claim that the implementation or hardware tests have passed.
