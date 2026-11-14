# Independent coverage and scope review

Status: **completed**. Reviewed against baseline `75fe2f7e42546a1d75969040378d4616419a9dcf`. Two actionable P2 findings. No fixes were made to the fork or immutable snapshot.

## Findings

### COV-01 — P2: Adapt existing CLI sources to the new exact-length program contract

**Primary current-source location:** `qdl/src/lib.rs:599–604` (the new `read_exact` contract). Affected callers are `cli/src/main.rs:342–353` (erase), `cli/src/main.rs:374–389` and `409–433` (rounded-up file writes), and `cli/src/programfile.rs:167–182` (rawprogram input).

**Trigger and consequence:** `erase` still passes the one-byte slice `&[0u8][..]` for a whole partition. Every normal nonempty partition therefore receives its program request and initial ACK, then immediately fails with `MutationConfirmation::Unknown` / `UnexpectedEof` without supplying the promised payload. The built-in `write` and `overwrite-storage` callers still round file length up to sectors but pass the unpadded file. An otherwise valid image with a partial final sector now fails after starting the modification; large images can already have sent earlier chunks. The XML caller has the same failure when a program entry rounds a non-sector-aligned image up to the final sector.

**Executed evidence:** `probe-source/qdl/tests/coverage_review_probes.rs` uses the unchanged `cli/src/programfile.rs` directly via a path module. A nine-byte file, four-byte sectors, and a three-sector XML entry sends only `abcdefgh` (8 of 12 promised bytes), then returns `Unknown` with `UnexpectedEof`. A separate probe supplies exactly the erase caller's one-byte source and observes one command write, zero data writes, and the same failure. See `probe-results.txt`. The erase dispatch itself was traced statically; the probe does not run the device-opening CLI.

**Baseline evidence:** The baseline `firehose_program_storage` creates zeroed chunks and ignores the local read length; these callers relied on that padding/zero-source behavior. The current patch rightly removes this unsafe library behavior but changes these callers only for return-type compatibility. A compile-only caller adaptation does not preserve their execution semantics.

**Minimum repair:** Keep strict reads in the protocol library. Supply a bounded zero stream for erase, and explicitly supply only the permitted final-sector padding from file callers. Check the requested range against the source length before sending when it cannot be satisfied by the allowed padding. This is necessary adaptation of existing callers, not implementation of the later product restore/flash workflows.

**Regression needed:** Exercise the real caller input preparation with a nonempty erase, a file smaller than one sector, and a file spanning more than one send chunk with a partial final sector. Assert the exact payload and final ACK. Include the real XML parser with the nine-byte/three-sector case. Retain the existing premature-EOF and hard-read-error tests so this repair cannot restore silent zero filling of truncated input.

### COV-02 — P2: Recognize trailing XML whitespace as a welcome document boundary

**Current-source location:** `qdl/src/lib.rs:233–239`, together with the exact `</data>` consumption at `250–257`. The real CLI invokes this path at `cli/src/main.rs:292–293`.

**Trigger and consequence:** A loader emits a complete welcome log document ending in `</data>\r\n`, does not emit the optional `INFO: End of supported functions` log, and then becomes idle. `firehose_read_xml` returns at `</data>`, leaving the legal trailing XML whitespace. Its next invocation puts that whitespace in `pending`. The idle timeout is accepted only when `pending.is_empty()`, so welcome collection fails as though a partial XML tag were received. The CLI exits before sending configure despite having received a complete welcome document.

**Executed evidence:** The proposed regression `completed_welcome_document_with_trailing_whitespace_should_end_on_idle` scripts `[LOG + "\r\n", TimedOut]`. It fails with `Err(scripted read failure)` in the current snapshot. The existing `welcome_timeout_only_completes_at_document_boundary` test passes because its `LOG` fixture ends exactly at `</data>` without whitespace. Full probe output is in `probe-results.txt`.

**Baseline evidence:** The baseline welcome path accepted timeout after receiving the complete log, including this trailing-whitespace case. Separating welcome collection from strict command ACK waits is correct, but the new boundary test rejects a previously accepted complete XML document.

**Minimum repair:** In welcome collection, distinguish inter-document XML whitespace from a partially received document before accepting an idle timeout. Preserve strict failure for a partial tag and preserve the command-response requirement for an actual response. Do not generally consume arbitrary trailing bytes after command ACKs, because those bytes may be raw storage data.

**Regression needed:** Cover space, LF, and CRLF after a complete welcome log, including whitespace fragmented across reads, followed by timeout. Keep a partial `<data>`/`<response>` followed by timeout failing, and keep log-only command waits failing.

## Coverage and transport assessment

The new tests exercise the actual `QdlDevice<ScriptedIo>` Sahara/Firehose functions and separate local file faults from transport faults. They cover short input reads, serial continuation, USB short-write rejection, partial command failure, initial/final NAK, final-ACK timeout, local output short writes and flush failure, response fragmentation, malformed fixed Sahara lengths, selection closure counts, and output suppression. Shared traces survive Drop and demonstrate that Drop sends no commands.

`ScriptedIo::Read` and `BufRead` share a position, repeated `fill_buf` does not consume data, and an unscripted read panics. Zero-length destination reads do not consume input. These semantics fit the protocol tests. Output is intentionally permissive after its action queue is exhausted (`support/mod.rs:107`), so an exact write-count/command assertion is still necessary to establish absence of additional writes. The main failure tests do include those assertions. The helper cannot by itself prove backend completion semantics or device behavior.

I traced real USB and serial selection through `select_and_open`; the selection tests match the approved narrow closure-based boundary. I also inspected the locked `nusb 0.2.1` reader and the direct USB write completion path. The dependency's streaming reader skips USB packet delimiters, supporting removal of the dummy zero-length storage read. Its normal blocking timeout already returns `TimedOut`; the new cancellation helper test is only a conversion test, not an exercised USB transport. These limits do not constitute additional blockers without a concrete failing path.

The test suite has no existing CLI tests, which explains why the exact-input contract change can pass all 62 tests while leaving built-in callers broken. Missing later product verification/stop-policy tests are not findings at phase 1.

## Scope conclusion

The implementation remains within the approved lean first stage: protocol fixes, test transport, selection seam, result types, and affected caller adaptation. COV-01 requires completing that caller adaptation; COV-02 is a regression in the newly introduced welcome reader. Neither asks for a product workflow, backup manifest, persistence system, saved plan, patch simulator, hardware test, or cross-platform run.

Returning a configure NAK as a failure is an approved correction. I do not request restoring hidden configure retries or automatic payload negotiation. Existing upstream CLI successful-run reset policy and automatic bootable selection are explicitly retained upstream behavior, not grounds to demand later product policy in this round.

## Read inventory

- Full assigned `persona.md` and `review-standard.md`.
- Full `design.md`, lines 1–375.
- Full `source.patch`, lines 1–2820.
- Complete modified production files: `qdl/src/lib.rs`, `parsers.rs`, `sahara.rs`, `serial.rs`, `types.rs`, `usb.rs`, `cli/src/main.rs`, and `cli/src/programfile.rs`.
- All new tests in full through the patch: `qdl/tests/firehose_io.rs`, `sahara_transfer.rs`, `selection.rs`, and `support/mod.rs`. Complete current `sahara.rs` and `usb.rs` include their unit tests.
- Complete snapshot `AGENTS.md`, `README.md`, workspace and qdl Cargo manifests, `cli/src/flasher.rs`, `cli/src/util.rs`, and `qramdump/src/main.rs`; relevant locked dependency versions in `Cargo.lock`.
- Targeted baseline `git show` reads for Firehose response/program behavior and the CLI main/XML callers.
- Complete locked `nusb-0.2.1/src/io/read.rs`, plus relevant serial timeout implementation locations. No external sources were necessary for this code review.

## Executed checks and limits

All execution used `coverage_scope/probe-source`, copied from the immutable snapshot. No production code was modified in that copy; only a new review-only test file and its fixture were added after the existing-suite run.

1. `cargo test --workspace --all-features --locked --offline`: passed, 62 existing tests (7 library, 39 Firehose, 12 Sahara, 4 selection); CLI and qramdump have zero tests.
2. `cargo test -p qdl --all-features --locked --offline --test coverage_review_probes -- --nocapture`: two characterization tests passed and the proposed welcome-whitespace regression failed as described. This nonzero result is deliberate evidence of COV-02, not a failure of an existing test.
3. `cargo build --workspace --all-features --locked --offline`: passed.

Formatting and Clippy were not rerun; this review concentrated on behavior and missing integration coverage. Hardware, USB/serial enumeration, device CLIs, and cross-platform execution were not performed. No memory/history or other reviewer work was read. No commit or push was performed.
