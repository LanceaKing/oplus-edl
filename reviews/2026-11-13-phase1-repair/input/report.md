# Phase-1 repair review: caller input compatibility

Status: complete. No actionable P0–P3 finding was found in the reviewed input repair. Prior finding R1 is closed for the approved offline first-stage scope. This is a review of supplied, uncommitted changes; no commit or push was made.

Reviewed source: `/Users/Apple/Documents/oplus-edl/reviews/2026-11-13-phase1-repair/source`. Baseline recorded by the review package: `75fe2f7e42546a1d75969040378d4616419a9dcf`. Review inputs were `repair.diff`, relevant sections of `source.patch`, `design.md`, `previous-review.md`, `review_scope.json`, and the source files cited below. I did not read other current reviewers' reports.

## Findings

None. I did not identify a remaining R1 failure or a new input-handling regression supported by source inspection or an offline probe.

## Prior R1 closure

- **Erase now supplies the full finite byte stream.** `cli/src/main.rs:346–353` obtains the partition sector count with a checked conversion, creates `zero_input`, and passes that same count to the core. `cli/src/input.rs:15–23,87–88` checks multiplication and bounds `io::repeat(0)` with `take`. The supplied erase test at `cli/tests/program_input.rs:84–98` successfully sends 20 zero bytes in `[8,8,4]` payloads and verifies EOF afterward. This closes the previous one-byte-source failure without reinstating core padding.
- **Both raw-image callers use the repaired preparation path.** `cli/src/main.rs:379–395` and `415–441` use `ProgramInput::from_file(..., 0, None)` and its returned sector count. The partition write path compares the rounded sector count with `part.size()` before sending the program command (`421–431`). `cli/src/input.rs:36–55` snapshots the needed source length and permits less than one sector of zero padding. Aligned, sub-sector, and multi-payload partial-tail images are covered by the supplied test at `cli/tests/program_input.rs:64–79`.
- **The actual XML parser uses the adapter.** `cli/src/programfile.rs:132–135` parses optional `file_sector_offset` strictly as `u64`; `164–167` checks sector-to-byte multiplication and constructs the bounded selected range before `firehose_program_storage` at `169–178`. The supplied tests execute `parse_program_xml`, rather than an imitation of its preparation logic: they check offsets, bounded ranges, permitted tail padding, and invalid ranges with no writes (`cli/tests/program_input.rs:102–176`). An invalid range here is rejected before that entry's program command; this does not claim all-entry product preflight.
- **Truncation is still an error.** `cli/src/input.rs:69–77` returns `UnexpectedEof` if the known required file range runs out before its saved limit. It starts padding only after that range has been fully read. The strict core `read_exact` remains at `qdl/src/lib.rs:614–616`. The supplied truncation test at `cli/tests/program_input.rs:180–199` sends the first eight valid bytes, then returns `Unknown/UnexpectedEof` without sending an incomplete second buffer or a reset. Additional probes confirmed that truncation inside a selected eight-byte range fails, whereas removing only unused bytes after that selected range succeeds.
- **Final ACK and error propagation remain intact.** `qdl/src/lib.rs:644–649` flushes and requires a final ACK; NAK returns `Rejected`, while response errors become `Unknown`. `cli/src/programfile.rs:178` preserves `MutationFailure` in `anyhow::Error`, and `parse_program_xml` propagates it using `?` at `219–226`. The supplied final-response test (`cli/tests/program_input.rs:203–220`) rejects final NAK and timeout after sending the complete padded bytes. A reviewer probe verified the actual error types through the XML entry point and verified that a second program entry is not sent after either failure.

## Regression adequacy and scope

The eight supplied `program_input` tests address the specific prior caller gap. They compile the production adapter and XML parser through `#[path]` modules. The fake transport is the same `QdlDevice<ScriptedIo>` seam used by the library tests. Its `Read` and `BufRead` share position, unscripted reads panic, and writes are available after device drop (`qdl/tests/support/mod.rs:62–94,98–110,124–149`). The payload assertions are meaningful here because these scripts accept each complete write; short-write semantics are separately exercised by the supplied core suite.

The supplied Firehose suite adds coverage around the strict input core: short source reads, incomplete buffers, later input failure, USB short writes, final ACK timeout, initial/final NAK, and preserved errors. All 45 tests passed. No restoration of silent premature-EOF padding was found.

The repair does not introduce product workflows, plan/manifest state, a new protocol abstraction, or device operations. The input module is a small caller adapter; parsing the offset strictly and checking partition size are directly tied to the source-range contract. Existing upstream CLI success-reset and automatic bootable behavior were not reclassified as new repair findings, consistent with the prior review's scope.

The supplied tests do not invoke the executable `main()` or exercise real GPT reads. I statically verified the main call sites and their count/range arguments, then tested their shared production preparation functions offline. This is sufficient for the R1 repair assessment, but is not an end-to-end CLI or device validation claim. Existing product-level all-entry preflight, workflow policy, sparse-image support, and cross-platform runtime behavior remain outside this repair review. R2/R3 were covered by the supplied Firehose suite run, but their independent semantic review is assigned separately.

## Reviewer-only probes

The probe file is `input/probe-source/cli/tests/review_input_probes.rs`; it exists only in this reviewer's copy. Four probe tests passed:

1. A 12,285-case range matrix across sector sizes 1, 2, 3, 4, and 7; source lengths 0–20; in-range and past-EOF offsets; inferred and explicit sector counts; empty reads; three-byte reads; and repeated EOF. Every accepted case produced exactly the selected source bytes plus permitted tail zeros; invalid cases failed construction.
2. Appending data after preparation did not change the known source range or consume appended bytes in place of saved tail padding.
3. Truncation distinguished loss inside the required range from removal of unused source tail bytes, preserving `Unknown/UnexpectedEof` and withholding incomplete payloads.
4. Real XML parsing with the optional offset omitted and `allow_missing_files=true` preserved `Rejected` for final NAK and `Unknown` for final timeout, emitted the expected padded payload, and stopped before a second XML entry.

These probes assert expected correct behavior. Their passing result is evidence of the checked cases, not a simulated failure counted as a production pass.

## Commands and actual results

All execution occurred in `input/probe-source`, copied from the immutable source snapshot. Build artifacts and temporary fixture files were redirected under `input`:

```sh
TMPDIR=/Users/Apple/Documents/oplus-edl/reviews/2026-11-13-phase1-repair/input/tmp
CARGO_TARGET_DIR=/Users/Apple/Documents/oplus-edl/reviews/2026-11-13-phase1-repair/input/target
```

These variables were passed with `env` on each command below:

```sh
cargo test --offline --locked -p qdl-rs --test program_input
cargo test --offline --locked -p qdl-rs --test review_input_probes -- --nocapture
cargo test --offline --locked -p qdl --test firehose_io
```

Results:

| Command target | Result |
| --- | --- |
| `program_input` supplied tests | Exit 0; 8 passed, 0 failed |
| `review_input_probes` reviewer-only tests | Exit 0; 4 passed, 0 failed; 12,285 range cases checked |
| `firehose_io` supplied tests | Exit 0; 45 passed, 0 failed |

The reviewer-only probe compilation emitted one expected `dead_code` warning for `zero_input`, which that extra probe module does not call. The supplied `program_input` suite uses and tests the function. No claim is made that the scratch probe suite passes warning-as-error linting.

Read-only inspection used `rg`, `cat`, `nl -ba`, and `sed` on the listed package/source files. One initial `rg` command used the review root instead of the source directory and exited 2 for a missing relative test path; the lookup was rerun successfully from the source directory. A Python `shutil.copytree` call created this reviewer's copy. No application CLI was run, no hardware was enumerated or opened, no network modification was made, and neither the immutable snapshot nor `/Users/Apple/Documents/qdlrs` was edited. This reviewer did not run full-workspace formatting, Clippy, build-only, or cross-platform checks.
