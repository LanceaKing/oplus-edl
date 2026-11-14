# Welcome handoff coverage review

Status: **changes requested**. The intended welcome-to-configure fix is covered and works. One behavior regression outside that handoff can turn a failed raw read into success. This report concerns the immutable `source/` snapshot and patch against `bd136c484ba4c21e09c14230d10cd0c8956a2fcf`; it does not repair production.

## Actionable finding

### P1 — Keep welcome whitespace recovery out of final raw-read responses

Location: `source/qdl/src/lib.rs:249–255` (changed condition and whitespace consumption). Affected call path: `firehose_read_storage`, lines 731–749, calls the same XML reader for the final ACK after reading the requested byte count.

Changing `if welcome && pending.is_empty()` to `if pending.is_empty()` permits arbitrary XML whitespace before **every** command response, including a final raw-read ACK when no welcome collection has occurred. That can hide an invalid raw length and return success with framing bytes in the output.

Exact reproducible case, using the existing ScriptedIo defaults (4-byte sectors), requests two sectors, or 8 bytes:

```text
<data><response value="ACK" rawmode="true"/></data>
[only seven raw bytes: abcdefg]
[CR LF]
<?xml version="1.0"?><data><response value="ACK" rawmode="false"/></data>
<data><response value="NAK"/></data>  [unrelated sentinel]
```

The displayed line breaks are explanatory only; exact concatenated bytes are in `short_declared.bin` and `probe-inputs.json`. No welcome function is called. The raw read takes the seven data bytes plus CR to reach eight. The final XML read now discards the remaining LF and accepts the declared ACK. `firehose_read_storage` returns `Ok(())` with output `61 62 63 64 65 66 67 0d` (`abcdefg\r`). The sentinel is still available afterward. A caller can therefore treat corrupted output as a completed read.

The exact baseline `qdl/src/lib.rs`, obtained using read-only `git show bd136c484ba4c21e09c14230d10cd0c8956a2fcf:qdl/src/lib.rs`, returns an error for the same stream: `Invalid Firehose XML ... "<?xml"-like PI is only valid at the beginning of the document`. It has already emitted the same eight bytes, but **does not report the operation successful**. This is a success/failure regression, not just a different error message.

Evidence:

- `current-probe-tests.log`: `short raw / declared ACK: success=true ... error=None`.
- `baseline-exact-probe-tests.log`: same input, `success=false`, original XML error retained.
- `work/qdl/tests/review_probe.rs`: executable probe assertions; each declared case covers the fully coalesced stream, every two-fragment split, and one-byte fragments, 170 arrangements each.
- A second case sends `abcdefgh\n` as nine raw bytes before an immediately declared final ACK. Current code discards the excess LF and succeeds; baseline fails. This demonstrates the same widened boundary acceptance, not a second finding.

Suggested bounded correction: restrict the new normalization to the pending welcome handoff, and add a no-welcome, short-raw-plus-declared-final-ACK negative regression. Do not use this finding to expand the patch into a redesign of generic ACK/rawmode validation. Any broader permission for whitespace after raw data needs its own explicit boundary contract; the welcome fix alone does not establish one.

Baseline distinction: repeating the short-raw case **without an XML declaration** already succeeds on the baseline because the XML parser accepts leading whitespace before `<data>`. `baseline_undeclared_final_ack_ambiguity` proves that in both logs. That existing limitation is not attributed to this patch. The finding is the new acceptance of the declared-response case, which the old production code rejects, and its unnecessary reach beyond the welcome handoff. Both probes use sensible `rawmode=true`/`false` attributes; missing or invalid rawmode is not the cause.

## Test adequacy

The intended fix has strong positive coverage:

- `firehose_io.rs:448–472` exercises welcome end marker, CR/LF/space/tab, XML declaration, the real configure send and response parser, negotiated send-buffer value, and a following ACK. It includes fully coalesced input, every two-fragment split, and byte-at-a-time input. This matches `cli/src/main.rs:295–301`.
- `firehose_io.rs:476–503` compares all eight returned raw whitespace bytes exactly and then reads a NAK sentinel. It detects consuming raw whitespace after the initial ACK or consuming the following response. It exercises every two-fragment boundary. Existing tests at lines 291–317 and 625–632 separately cover fully coalesced raw input, fragmented data, interrupted reads, and a raw prefix containing all four allowed whitespace bytes.
- New timeout/EOF and malformed-XML tests (lines 507–537), together with existing log-only timeout, partial XML, initial/final NAK, and missing final ACK tests, keep an absent or incomplete response from becoming an ACK. Whitespace skips do not convert timeout to success in command mode.
- The receive-limit test (lines 542–553) checks that the actual declared XML still fits exactly or fails one byte short. It also explicitly encodes the broader non-welcome prefix behavior.
- Successful handoff tests inspect write counts after Drop; configure/read paths emit one expected request, welcome and parser calls emit none, and reset/patch/set-bootable sends are excluded. The production delta itself adds no write path.

Sensitivity check: reverting only the production condition in the private copy causes three of the 50 Firehose tests to fail: `welcome_handoff_accepts_declared_configure_response_at_every_split`, `welcome_handoff_preserves_raw_whitespace_at_every_split`, and `response_prefix_whitespace_preserves_xml_size_limit`. The remaining 47 pass. The new tests therefore detect the original declared-XML handoff bug; they are not merely asserting behavior that already passed before the fix.

The actionable gap is narrower than a request for exhaustive cases. All positive raw tests provide exactly the expected raw byte count. The newly added raw test also includes CRLF before the final declared ACK at lines 484–486, so it silently blesses the expanded final-response policy. It never removes one raw byte or appends one whitespace byte to show that this policy can hide a length mismatch. The reviewer probe supplies that concrete counterexample.

## ScriptedIo fidelity and limits

`qdl/tests/support/mod.rs:58–94` provides a single pending byte buffer and cursor for both Read and BufRead. `fill_buf` does not consume bytes; `consume` alone advances the cursor; `read` copies from the same remainder. The existing `scripted_read_and_bufread_share_position` test checks this explicitly. This is faithful to the handoff contract used by `QdlDevice` and the serial/USB buffers: both real adapters drain any unread internal buffer before another transport read (`serial.rs:26–61`, `usb.rs:79–114`). The mock asserts on over-consumption and unscripted reads, which makes boundary errors visible.

Unlike real adapters, the mock fetches the next scripted chunk via `fill_buf` even for a raw Read; real adapters can read directly from their transport when their buffer is empty. The exact-byte stream and fragmentation sweeps cover the behavior relevant here. The mock does not establish real device timing, USB packet/ZLP behavior, serial driver behavior, or actual loader support. No such claims follow from these tests.

## Scope and execution evidence

Read: patch, review scope, approved minimal-scope design, snapshot AGENTS/README, Firehose/parser/types/backend code, real CLI call sequence, and relevant offline tests/support. No other current reviewer's report was opened.

Verified all 33 snapshot file hashes and the patch hash from `review_scope.json` before testing. Rechecked immutable snapshot hashes after probes. All writes stayed in `coverage/`. The private production file was restored to the current snapshot after each mutation.

Executed tests and evidence:

| Actual command / operation | Result | Evidence |
| --- | --- | --- |
| Copy `source/` to `coverage/work` after SHA-256 verification | 34 checks passed, including patch | `integrity.json` |
| In `coverage/work`: `cargo test --workspace --all-features --locked --offline` | 81 tests passed: 7 library, 50 Firehose, 12 Sahara, 4 selection, 8 CLI input; other targets/docs have zero tests | `current-workspace-tests.log` |
| Private-copy condition reversal, then `cargo test -p qdl --all-features --test firehose_io --locked --offline` | Expected regression result: 47 passed, 3 failed | `reversed-production-tests.log` |
| `REVIEW_EXPECT_TRIM=1 cargo test -p qdl --all-features --test review_probe --locked --offline -- --nocapture` | Current behavior matches all three probe assertions | `current-probe-tests.log` |
| Same probe command with `REVIEW_EXPECT_TRIM=0` and reversed condition | Baseline-condition behavior matches all three probe assertions | `reversed-probe-tests.log` |
| Read-only `git show bd136c484ba4c21e09c14230d10cd0c8956a2fcf:qdl/src/lib.rs` in `/Users/Apple/Documents/qdlrs`, saved as `coverage/baseline-lib.rs`; install only in private copy and run probes with `REVIEW_EXPECT_TRIM=0` | Exact baseline production source rejects both declared counterexamples; undeclared control succeeds | `baseline-exact-probe-tests.log` |
| In `coverage`: `python3 run_probes.py` | Repeats current/exact-baseline comparison, restores private production source | `run_probes.py`, same current/baseline logs |

Probe tests intentionally assert the observed current/baseline behavior, so their green result is reproduction evidence, not an endorsement of the current false success. Exact test streams, raw/separator hex, and fragmentation counts are preserved in `probe-inputs.json` and the three `.bin` files.

Limitations: this review did not run fmt, Clippy, separate build, cross-platform checks, or hardware tests. Cargo tests used cached dependencies with `--offline --locked`. No application CLI, device enumeration/opening, device operation, network mutation, commit, or push was performed. Running the existing test harness's quiet-output subprocess is still an offline test, not an application CLI invocation.

## Focused reassessment addendum — original P1 withdrawn

This addendum preserves the original report and all probe evidence above. It **supersedes the original “changes requested” status and P1 classification**. Revised coverage-review conclusion: **no actionable new data-integrity defect established by these probes**. The observed acceptance change is real, but the original claim of corrupted/short raw data depends on an external sender-intent assertion that the byte stream does not encode.

### What the input proves, and what it does not

The request is for eight bytes. After the initial ACK, the exact stream begins:

```text
61 62 63 64 65 66 67 0d 0a 3c 3f 78 6d 6c ...
a  b  c  d  e  f  g  CR LF <  ?  x  m  l ...
```

The original report assigned the first seven bytes to raw data and CRLF to response framing. The same input equally represents **eight valid raw bytes `abcdefg\r`, followed by LF framing and a declared final ACK**. The receiver requests eight bytes, returns exactly those first eight bytes, and only then removes the remaining LF before parsing XML. No byte inside the requested eight-byte raw range is discarded or changed by this production delta.

Likewise, the “excess raw” probe can represent eight valid raw bytes `abcdefgh` followed by LF framing. Its label asserts that LF was intended as a ninth raw byte; no observable field or transport event in the probe supplies that fact. `Input::Bytes` boundaries are arbitrary fragments, not typed raw/XML message boundaries. Sweeping every fragmentation cannot resolve this ambiguity.

The original P1 wording therefore overstated what was observed. The logs establish `Ok` versus `Err` for the same stream, and establish the emitted bytes. They do **not** establish that the eight emitted bytes differ from the requested storage contents. A baseline XML-declaration parse failure is not proof that the baseline had validated the sender's raw length.

### Owning contract/source check

I found no explicit prohibition on XML whitespace at the raw-to-final-XML boundary in the supplied design or unchanged read API contract:

- `design.md:84` requires the complete expected byte count and final ACK before publishing a backup. `design.md:184` and `design.md:350` require handling short reads/EOF/errors without false completion. These statements do not identify response framing independently of the byte-count boundary, and do not say that CR/LF/space/tab are forbidden before a final XML response. The probe observes eight raw-range bytes and a complete ACK, without EOF or transport error.
- `source/qdl/src/lib.rs:693–697, 730–749` owns the read boundary: compute the requested byte count, read and write exactly that many bytes, then call the XML response reader. The change does not modify that count or the raw loop.
- `source/README.md:28` describes complete output writes, flush, and final ACK. Updated `README.md:33–34` and `AGENTS.md:69` explicitly document prefix whitespace for XML reads generally. These changed documents are evidence of the proposed policy, not independent external protocol authority, but they show that broadening was documented rather than concealed.
- The new raw handoff regression at `source/qdl/tests/firehose_io.rs:477–486` explicitly places CRLF **after** all eight raw bytes and before the final XML declaration. Thus the implementation and its test both treat such final-response whitespace as framing. The size-limit test at lines 542–553 covers XML reads without welcome as well.
- The undeclared-ACK control succeeds in both baseline and current code, as the existing logs demonstrate. That supports classifying the external seven-byte interpretation as a pre-existing, unobservable sender-length ambiguity, not a newly proven loss of valid raw bytes.

No external Firehose specification or captured device boundary evidence was introduced in this reassessment. Consequently I cannot cite an owning contract that makes the ambiguous stream definitively invalid, and cannot sustain a definite P1 or a lower-severity correctness finding solely from it.

### Revised recommendation and coverage judgment

Withdraw the proposed blocking fix and the demand that the ambiguous stream must fail. A negative test based only on “the sender intended seven bytes” would encode an extra framing prohibition absent from the supplied contract. Restricting normalization to welcome might preserve the old rejection of this declared XML syntax, but these probes do not establish that this restriction is required for data integrity.

The patch does broaden XML-prefix acceptance beyond welcome; that is a factual scope observation. It is documented, directly tested, and uses the shared XML reader. With no distinct invalid-input counterexample or owning prohibition, it is not an actionable finding from this review. The existing exact-output tests remain adequate evidence that the new reader leaves the requested raw range intact. Their limits remain ordinary offline-test limits: they do not prove actual device content, USB packet semantics, or detect a sender that supplies a different byte sequence from its true storage contents.

The positive coverage, 81 passing workspace tests, three failures upon production-condition reversal, ScriptedIo fidelity analysis, and execution limitations in the original report remain valid. Only the asserted data-integrity defect, its severity, and the related corrective recommendation are withdrawn.

Reassessment work: read-only `rg` search of `source/README.md`, `AGENTS.md`, `qdl/src/lib.rs`, and `design.md`, plus reinspection of saved current/exact-baseline probe logs. No new production edits or device operations; no rerun was needed because the counterargument changes interpretation of the identical preserved bytes, not the observed program behavior.
