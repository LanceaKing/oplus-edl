# Stage 9: Implementation Readiness Review

**Target:** `docs/designs/oplus-edl.md`, active lines 134–237. All reviewers read the complete target and `concepts_brief.md`.
**Result:** Does not yet pass the implementation readiness standard. 0 Blockers, 4 Majors, 4 Nits.

## Reviewer status

| Reviewer | Agent | Status | Findings |
| --- | --- | --- | --- |
| Implementation readiness | `/root/stage9_readiness` | Completed | 3 Major, 1 Nit |
| Codebase details | `/root/stage9_codebase` | Completed within available local evidence | 1 Major; no confirmed code/document discrepancy |
| Style | `/root/stage9_style` | Completed | 3 Nit |

Each reviewer ran in a fresh parent-turn context, read its complete persona, the complete target (237 lines), and the complete concept brief (207 lines). Readiness and codebase reviewers also read `implementation-readiness-standard.md`. No separate style standard was supplied. No required local read failed. They did not read earlier reports, unrelated drafts, history, or external sources.

## Major findings

### I9-01 — Verification modes lack distinct behavior

**Target:** lines 174, 180, 209–210, 221, 225; related specification at 107, 111–115.

The plan defines `best-effort|required|none` but describes only the default behavior. It does not say what `required` changes when a read is rejected or a patch makes expected bytes unknowable. It also leaves boot-unit changes and reboot after incomplete verification or mismatch unclear.

For example, every write can receive an ACK while a patched range cannot be verified. The implementer must decide whether `required` rejects the operation before writing, stops after writing, or completes like `best-effort`. These choices change device behavior and the command promise.

**Minimal remedy:** Add a three-mode table covering attempted verification, unknown expected bytes, read failures, final status, continuation, and explicit final actions. Add assertions to `tests/verification.rs`. No patch simulator or new subsystem is needed.

### I9-02 — Backup filenames lack a complete restore mapping

**Target:** lines 169–170, 178, 207–208; related specification at 85, 91, 95.

The normal `lun<N>/<partition>.bin` layout is specified, but name escaping, collision handling, and primary/backup GPT filenames are not. Directory restore matches ordinary images by partition name. A renamed file needs a defined reverse mapping, while GPT captures need an exclusion rule.

**Minimal remedy:** Define accepted names, collision behavior, GPT filenames or subdirectory, and restore candidates. State how nested LUN directories interact with `--lun`. Rejecting names that cannot be mapped safely is a valid small policy. Add round-trip tests for ordinary names, collisions, same-named partitions across LUNs, and GPT captures. No manifest is needed.

### I9-03 — The machine-readable result contract is undefined

**Target:** lines 164, 175, 180–181, 211, 221; related specification at 113–115.

JSON is promised to carry the same facts as text, with several exit categories, but fields, permitted states, and aggregation are not defined. A backup may mix successful and failed items. A flash may mix acknowledged writes, unverified ranges, and skipped local patches. The implementer must invent the response format and overall outcome rules.

**Minimal remedy:** Add one JSON example and a compact field/state table covering target, operation or item, write outcome, verification outcome, skipped items, and errors. Define aggregation and mapping to the existing exit categories. Integer exit values can remain implementation constants. No separate JSON schema suite is needed.

### B9-01 — Offline fault injection has no defined interface

**Target:** lines 177, 192–194, 218.

The plan lists synthetic responses, short reads/writes, timeouts, and tests for unintended resets or wrong-device opens. It does not identify how tests supply transport input, capture commands, and trigger timeouts or end-of-stream events. No local production or test infrastructure resolves that choice.

This is a design gap, not proof that upstream lacks a usable interface.

**Minimal remedy:** Check the pinned qdlrs source, then name the existing interface to reuse or the narrow change needed for offline tests. Specify input injection, output observation, and error/timeout delivery. If a shared helper file is necessary, add its exact path and Create/Modify action. No general protocol framework is required.

## Nits

### I9-04 — Define completion checks for intermediate phases

**Target:** lines 218–227.

The sequence and final checks are explicit, but only the fork phase has an immediate build requirement. Add a short completion condition for phases 2–4 using the already planned test files and build checks. Unfinished command paths can return unsupported at intermediate checkpoints. No additional test infrastructure is required.

### T9-01 — Distinguish sparse chunks that write from those that skip

**Target:** line 173.

`解析并发送 RAW／FILL／DONT_CARE` groups skipped ranges with transmitted data, unlike lines 103 and 225.

**Minimal wording:** Parse RAW, FILL, and DONT_CARE chunks; write RAW and FILL data, and skip DONT_CARE ranges.

### T9-02 — Preserve the expected-content rule in the verification file description

**Target:** line 174.

`与实际源字节比较` can be read as comparing patched ranges with the original image, contrary to line 111.

**Minimal wording:** Read back with bounded memory, compare ranges whose expected contents are known under the verification rules above, and report unverified or mismatching ranges.

### T9-03 — Preserve partial-I/O and sector-padding distinctions in acceptance text

**Target:** line 225.

`短读短写不成功` can imply every partial I/O call must fail, although lines 142–143 permit handling it. `较短镜像只写实际范围` omits the allowed zero padding from line 93.

**Minimal wording:** Incomplete or failed transfers must not be reported as complete. A short image writes only its data and permitted zero padding up to the sector boundary, within the target partition.

## Validated local facts and differing interpretation

The inspected local tree contains no Cargo manifest, Rust production code, CI configuration, or test infrastructure. This agrees with line 155. Product paths are explicitly future additions; the fork and its final revision are also future work. Their absence is not a broken-reference finding. The concept brief resolves at the linked local path.

The file allowlist is explicit. The design separates future build checks and device validation from completed work. Deferred manifests, identity proofs, journals, saved plans, automatic resume, and RMX3800 support were not treated as requirements.

The readiness reviewer did not raise a separate stop-policy finding because the supplied brief already requires stopping on unexpected storage responses and forbids automatic write retries. This differs from D7-01's focus on carrying the policy into the current target and explicitly suppressing remaining actions. The cross-stage report preserves that distinction rather than counting a new issue.

## Upstream verification limits

No upstream checkout was supplied or found locally. Consequently, the review cannot independently confirm the defects at lines 138–149, fork paths and signatures at 186–199, completeness of affected callers, available transport/test abstractions, exact reference XML/sparse/storage/backend behavior, licensing, or build compatibility. These are unverified claims, not confirmed discrepancies.

No design edits, builds, tests, device actions, or nested reviewers were used. The lack of upstream evidence limits the codebase audit; it does not turn the planned files into defects or establish that the implementation would fail.
