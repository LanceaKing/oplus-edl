# Firehose review — completed

Status: **completed**. Three verified P2 findings. No fixes were made. The real fork and immutable snapshot were read only. All executed tests used a private copy under `firehose/scratch/source`; no device enumeration, device opening, or device CLI invocation occurred.

Source line numbers below refer to `/Users/Apple/Documents/oplus-edl/reviews/2026-11-13-phase1-code/source/`. Baseline: `75fe2f7e42546a1d75969040378d4616419a9dcf`.

## FH-01 — P2 — Adapt legacy program sources to the new exact-input contract

**Changed location:** `qdl/src/lib.rs:599–604`, specifically the replacement of unchecked `read` with `read_exact` at lines 602–604.

**Affected callers:** `cli/src/main.rs:345–353` (erase), `374–389` (overwrite-storage), `409–433` (write), and `cli/src/programfile.rs:167–182` (rawprogram).

**Trigger and consequence:** The retained CLI erase path supplies only `&[0u8][..]` for an entire partition. With a normal sector size greater than one, every erase now sends and receives the initial program ACK, then returns `Unknown`/`UnexpectedEof` without sending raw data. The write and overwrite paths round the file length up to sectors but still pass the original unpadded `File`. The rawprogram path also passes its original file. A valid image with a partial final sector therefore fails after program has begun; images larger than one payload can already have written their preceding full payloads. Merely converting the new return type in `programfile.rs` does not adapt these inputs.

**Baseline distinction:** The callers are unchanged, but baseline `firehose_program_storage` allocated a zero-filled buffer for every payload and ignored the short source-read length. That incidentally supplied the intended erase bytes and final-sector zero padding. The new exact-read requirement is the correct library fix for arbitrary EOF, but introduces these caller regressions unless the callers explicitly supply their intended zero bytes. The repair must not restore unchecked source reads or silently pad unexpected truncation.

**Executed evidence:** The review-only `legacy_erase_input_stops_after_initial_ack` probe uses the exact erase source expression and actual library function: one program command, zero raw bytes, `Unknown`/`UnexpectedEof`. The `actual_xml_caller_leaves_unaligned_image_partially_written` probe directly includes and calls the snapshot's `cli/src/programfile.rs::parse_program_xml`, using a 9-byte file, 4-byte sectors, 3 requested sectors, and an 8-byte payload. It records the program command and the first 8 raw bytes, then `Unknown`/`UnexpectedEof`; the final padded sector is never sent. CLI `main` was not run because that would require opening hardware; applicability to its unchanged source expressions is static.

**Minimal repair direction:** Adapt existing callers to the required input contract: an explicit zero stream for erase, and finite permitted tail padding for partial final sectors. Keep source-range/truncation failures distinguishable from that known padding, and detect known invalid lengths before sending program. No product workflow is required for this repair.

**Missing regression:** Offline tests through the real CLI program-input adapter, covering erase, aligned files, a file shorter than one sector, a multi-payload file with a partial final sector, and unexpected file truncation. Assert complete intended bytes and final ACK before success, and no command for known invalid input.

## FH-02 — P2 — Do not classify restart-required read NAKs as a safe refusal

**Changed locations:** `qdl/src/lib.rs:313–319` and `699–701`; new interface documentation is `qdl/src/types.rs:60–66`.

**Trigger and consequence:** A read response such as `<data><response value="NAK" AttemptRestart="1"/></data>` bypasses the new retry/restart rejection because the condition excludes every NAK. `firehose_read` then discards the response attributes and `firehose_read_storage` converts the status into the fieldless `ReadRejected`. An explicit `rawmode="true"` on a NAK is also discarded and produces the same type. A caller cannot distinguish these responses from the normal command-boundary refusal used by the approved best-effort continuation rule. It therefore lacks the evidence needed to stop on a restart request or a contradictory raw-mode state while allowing an ordinary refused read.

**Baseline distinction:** This finding is about the newly introduced typed refusal interface and changed restart handling. It does not claim that all rawmode/ACK validation defects are new. Baseline handled `AttemptRestart` before the generic parser and returned an error after its undesired reset attempt. Removing automatic reset is correct, but collapsing that response into the same `ReadRejected` as a reusable-session refusal loses the stop information. Baseline had no `ReadRejected` continuation signal.

**Executed evidence:** `restart_nak_is_exposed_as_safe_read_rejection` feeds both responses above into the real `firehose_read_storage`; both errors downcast successfully to `ReadRejected`. No later command was executed by the probe. The future workflow consequence is a static interface analysis; this phase does not yet contain the product best-effort caller.

**Minimal repair direction:** Preserve the complete NAK as a rejection for modification confirmation, while retaining enough response state for the read path to distinguish a usable command boundary. A restart/retry request or explicit contradictory rawmode must not return the ordinary `ReadRejected` continuation signal. This can remain a small internal response/read-path change and does not require a session framework or automatic recovery.

**Missing regression:** Ordinary initial NAK stays typed and leaves the next XML response available; NAK plus `AttemptRestart`, `AttemptRetry`, or explicit `rawmode="true"` requires stopping and must not be indistinguishable from the ordinary refusal. Existing hint tests check only no automatically sent commands, not the downstream continuation classification.

## FH-03 — P2 — Accept inter-document whitespace when welcome logs end

**Changed location:** `qdl/src/lib.rs:233–239`, with the byte accumulation at `246–259` and welcome caller at `343–346`.

**Trigger and consequence:** The programmer sends a complete welcome log without the optional `INFO: End of supported functions` marker, followed by an XML newline and then an idle timeout. The first read consumes through `</data>` only. The next welcome read accumulates the newline in `pending`, so the `pending.is_empty()` condition rejects the timeout as if an XML document had been truncated. `cli/src/main.rs:293` propagates this error and never reaches configure, even though all welcome XML was complete. XML inter-document whitespace is not a partial XML token.

**Baseline distinction:** Baseline welcome collection was the generic `firehose_read`. Once it had received the complete log (and its newline), its `TimedOut if got_any_data` branch completed collection. That rule was unsafe for command ACKs and correctly needed replacement; applying an empty-buffer test to welcome mode introduces this narrower valid-whitespace regression. Command responses must still require an actual ACK.

**Executed evidence:** `welcome_trailing_whitespace_timeout_is_rejected` supplies `b"<data><log value=\"hello\"/></data>\n"` and `TimedOut` to the actual `firehose_read_welcome`, which returns the original timed-out error. The existing welcome test covers a boundary with zero trailing bytes only.

**Minimal repair direction:** Treat only XML whitespace between welcome documents as remaining at a document boundary. Continue to reject timeout after any non-whitespace partial XML. Limit this handling to welcome/XML framing so it cannot discard legitimate raw-storage bytes.

**Missing regression:** Welcome log with trailing LF, CRLF, and fragmented whitespace followed by idle timeout succeeds; a partial `<data` or response token followed by timeout fails; the same log-plus-timeout in command-response mode still does not acknowledge a command.

## Read inventory

- Full `firehose/persona.md`, `review-standard.md`, `design.md`, and `source.patch:1–2820`.
- Complete current `qdl/src/lib.rs`, `qdl/src/types.rs`, `qdl/src/parsers.rs`, `qdl/src/usb.rs`, and `qdl/src/serial.rs`.
- Complete current `cli/src/main.rs`, `cli/src/programfile.rs`, `cli/src/flasher.rs`, and `cli/src/util.rs`.
- Complete current `qdl/tests/firehose_io.rs` and `qdl/tests/support/mod.rs`; all new `sahara_transfer.rs` and `selection.rs` tests as complete added files in the full patch.
- Current `AGENTS.md` and workspace, qdl, and CLI Cargo manifests. README additions/changes through the full patch.
- Baseline `qdl/src/lib.rs` and `cli/src/main.rs` through read-only `git show`, to separate new behavior from unchanged callers and old defects.

## Executed checks and exact probe evidence

Working directory for every Cargo command:

```text
/Users/Apple/Documents/oplus-edl/reviews/2026-11-13-phase1-code/firehose/scratch/source
```

First, on the unchanged private snapshot copy:

```sh
cargo test --workspace --all-features --locked --offline
```

Observed: exit 0; 7 library unit tests, 39 Firehose tests, 12 Sahara transfer tests, and 4 selection tests passed (62 total). CLI and qramdump had zero unit tests; doc tests had zero tests.

Then, after adding only the review probe file in that private copy:

```sh
cargo test -p qdl --all-features --locked --offline --test review_firehose -- --nocapture
```

Observed output (test interleaving omitted, probe messages preserved exactly):

```text
running 4 tests
complete welcome XML plus trailing newline returns TimedOut
erase: Unknown/UnexpectedEof, one program command, no raw data
ReadRejected for <data><response value="NAK" AttemptRestart="1"/></data>
ReadRejected for <data><response value="NAK" rawmode="true"/></data>
actual parse_program_xml: Unknown/UnexpectedEof after eight raw bytes; final padded sector never sent
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

These four probe tests intentionally assert the current faulty outcomes; their passing is evidence of reproduction, not a claim that the defects are fixed. Probe source: `firehose/scratch/source/qdl/tests/review_firehose.rs:1–57`.

## Limits and unrun checks

No standalone build, Clippy, formatter, Windows/Linux run, hardware test, or device CLI run was performed. Cargo test compiled the macOS workspace. Baseline comparison was static, not an executed baseline replay. The generic framing and transfer probes do not establish real-device timing or compatibility. Unchanged product-policy gaps in the upstream CLI, including its existing successful-run reset policy and implicit bootable selection, were not promoted into findings. Product phases 2–6 and later workflow policy remain outside this review.
