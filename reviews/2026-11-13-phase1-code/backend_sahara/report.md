# Backend and Sahara review

Status: **completed**. One independent review round, with no production edits or device access. Two actionable P2 findings were confirmed by offline probes. No additional actionable defect was established in the USB/serial selection or Sahara range-validation changes.

Baseline: `75fe2f7e42546a1d75969040378d4616419a9dcf`.

Current-source paths below refer to the immutable snapshot at `/Users/Apple/Documents/oplus-edl/reviews/2026-11-13-phase1-code/source/`.

## Findings

### BS-01 — P2: Treat trailing welcome XML whitespace as a document boundary

- **Location:** `qdl/src/lib.rs:231–239`, particularly the `pending.is_empty()` condition at line 235. The delimiter consumption at lines 250–257 and `firehose_read_welcome` at lines 343–366 form the relevant path. The CLI uses this path before configure at `cli/src/main.rs:293`.
- **Trigger:** The loader emits a complete welcome log ending in `</data>\n`, without the special `INFO: End of supported functions` log, then becomes idle. A trailing newline is valid XML whitespace. The same case can occur over either byte-stream backend.
- **Consequence:** The parser returns the complete log while leaving the newline in the channel. Its next invocation consumes the newline into `pending`; the following idle timeout is then propagated as a fatal error. The CLI exits before configuring Firehose, although no XML document was truncated. This regresses the prior startup-log behavior and defeats the specifically intended idle-timeout completion path.
- **Executed evidence:** `scratch/qdl/tests/reviewer_regressions.rs`, test `welcome_trailing_xml_whitespace_is_a_document_boundary`, calls the actual `firehose_read_welcome` on `LOG + b"\n"` followed by a scripted `TimedOut`. It failed with `Err(scripted read failure)`. The first newline case failed before the later CRLF/tab cases in the test loop ran. See `reviewer-regressions.log`.
- **Baseline evidence:** The old startup path used `firehose_read`, whose timeout branch accepted timeout after log data. The fix correctly separates command acknowledgement from welcome collection, but the new welcome-only boundary test is too strict.
- **Minimum repair:** In the welcome collector only, allow XML whitespace between complete documents and regard an idle timeout with only that whitespace pending as an idle document boundary. Preserve failure for any partial XML markup and keep command-response timeouts fatal. Do not indiscriminately trim raw storage data.
- **Regression gap:** Existing welcome tests end the log at exactly `</data>` or use the special end log. Add newline and CRLF tails, split whitespace, several whitespace-separated logs, and a whitespace-plus-partial-next-document case. Only the complete-document cases should finish on timeout.

### BS-02 — P2: Migrate existing program callers to the strict input-length contract

- **Location:** Changed enforcement in `qdl/src/lib.rs:599–604`. Unmigrated callers are `cli/src/main.rs:345–352` (erase), `374–389` (overwrite-storage), and `409–433` (write). `cli/src/programfile.rs:167–181` also passes an unpadded raw file for XML-declared sectors.
- **Trigger:** Any normal erase operation supplies `&[0u8][..]` for a partition larger than one byte. A write or overwrite-storage operation supplies a file whose byte length is not a sector multiple, while calculating the request length with `div_ceil`.
- **Consequence:** After the device has acknowledged the program request and entered raw mode, `read_exact` fails with `UnexpectedEof`. Erase now fails for every ordinary partition. Partial-sector files also fail; for files larger than one transfer buffer, earlier chunks may already have been written before the final chunk fails. The strict library check is correct, but these existing callers still depend on the removed implicit zero padding.
- **Executed evidence:** Two review-only tests call the actual library with exactly the source shapes used by those CLI branches. With four-byte sectors, the one-byte erase source and a five-byte file rounded to two sectors both return `MutationFailure { confirmation: Unknown, cause: ... failed to fill whole buffer }` after one accepted program XML command and its initial ACK. Neither sends raw bytes in these small examples. See `existing_cli_erase_source_remains_usable` and `existing_cli_rounded_image_source_remains_usable` in `scratch/qdl/tests/reviewer_regressions.rs`, and `reviewer-regressions.log`. This reproduces the library/caller contract mismatch; it does not invoke the hardware CLI.
- **Baseline evidence:** The old library allocated zeroed transfer buffers and ignored the short source read, which made these particular sources work. Keeping that generic EOF behavior would reintroduce the defect that phase 1 correctly removes. The callers need explicit adaptation.
- **Minimum repair:** Keep `read_exact`. Give erase an intentional zero stream bounded by its requested byte count. For callers that deliberately round a known file length, supply only the permitted final-sector zero padding, and retain an error for true truncation or unrelated short input. Adapt the XML file caller where its supported input contract permits a final partial sector; otherwise reject its known length mismatch before sending program.
- **Regression gap:** Existing tests prove rejection of insufficient source bytes, but no test covers the actual CLI source construction. Add offline caller-level cases for erase, a non-sector-aligned file, and a file exceeding one transfer buffer, checking complete data including explicit padding and final ACK. Also keep a separate genuinely truncated-input failure case.

## Read inventory

- Complete assigned `persona.md`, `review-standard.md`, `design.md` (all 375 lines), and `source.patch` (all 2,820 lines).
- Complete snapshot `AGENTS.md` and all six core source files: `qdl/src/lib.rs`, `types.rs`, `parsers.rs`, `sahara.rs`, `usb.rs`, and `serial.rs`.
- Complete new test/support files through their full patch contents: `qdl/tests/firehose_io.rs`, `sahara_transfer.rs`, `selection.rs`, and `support/mod.rs`; existing Sahara and USB unit tests were also read in the full source files.
- Complete `cli/src/main.rs`, `programfile.rs`, `flasher.rs`, `util.rs`, and `qramdump/src/main.rs` for selection, startup, and storage callers.
- Workspace and library `Cargo.toml`; locked dependency entries for nusb 0.2.1, serial2 0.2.28, and bincode 1.3.3.
- Read-only baseline `git show`: complete `qdl/src/usb.rs`, relevant Sahara packet-reader/parser/state-machine section, and old Firehose read/program implementations.
- Locked nusb 0.2.1: complete `src/io/read.rs` and Linux USB transfer implementation; relevant endpoint submit/completion/`transfer_blocking` code in `src/device.rs`, and buffered write/`submit_end`/flush implementation in `src/io/write.rs`.
- Locked serial2 0.2.28: Unix and Windows read, write, timeout, and output-flush implementations in their `src/sys/*/mod.rs` files.

## Backend and Sahara evidence

- Real USB and serial opening paths call `select_and_open` before invoking the open closure. The location-key USB API and serial path selection reject absent or ambiguous matches through that helper. No hardware enumeration or open function was executed.
- USB OUT writes now call `transfer_blocking`, inspect both `Completion.status` and `actual_len`, and map cancellation to a non-retryable `TimedOut`. The locked nusb implementation consumes the submitted completion, including its cancellation path. This establishes the source-level basis for the helper tests, but does not reproduce real controller completion behavior.
- The nusb `EndpointRead` intentionally discards short-packet boundaries; its default stream reader skips ZLP delimiters. This supports removing the old empty-buffer drain on Firehose reads. Its idle timeout leaves a pending IN request rather than cancelling it, which permits a later startup/configure read to receive data on that request.
- USB and serial `Read` implementations drain their `BufRead` buffer before accessing the transport, preserving coalesced data when switching from XML parsing to raw reads. Empty reads now return immediately.
- Sahara image range addition is checked before host-sized conversion and slicing, missing IDs become errors, and exact packet body lengths are checked before dispatch. The existing single-image ID fallback and completion-status compatibility case are retained.
- I did not promote the changed USB packet-delimiter behavior to a finding: source inspection alone did not establish a concrete valid Sahara exchange that fails because of it. Real packet framing and device compatibility remain outside this offline result.

## Executed checks

All commands ran in the assigned reviewer's copied `scratch/` workspace. The archived source and real fork were not modified.

1. `cargo test --workspace --all-features --locked --offline --exclude qdl-cli --exclude qramdump --lib --test firehose_io --test sahara_transfer --test selection`
   - Exit 0; **62 tests passed**: 7 unit, 39 Firehose I/O, 12 Sahara transfer, and 4 selection tests.
   - Cargo warned that excluded package `qdl-cli` does not exist. The requested library and integration tests nevertheless ran successfully. No device CLI was run.
   - Full log: `existing-tests.log`.
2. `cargo test -p qdl --all-features --locked --offline --test reviewer_regressions`
   - Exit 101; **3 expected-behavior probes failed**, confirming BS-01 and the two BS-02 caller cases.
   - Probe source: `scratch/qdl/tests/reviewer_regressions.rs`.
   - Full log: `reviewer-regressions.log`.

## Unrun checks and limits

No USB/serial enumeration, port opening, hardware protocol exchange, device CLI execution, Windows/Linux execution, formatting run, Clippy run, release build, or repair iteration was performed. The tests use the existing `ScriptedIo` and prove the specified byte-stream cases only. Hardware completion behavior, physical identity persistence, throughput, and device-specific Sahara/Firehose compatibility are not established. No other reviewer reports, parent conversation history, or memory sources were consulted.
