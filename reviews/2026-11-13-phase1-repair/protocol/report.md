# Protocol repair review

Outcome: **R2 and R3 are closed as reported. No actionable defect introduced by these repairs was established.** The submitted Firehose regression suite passes. An additional failing handoff diagnostic reproduces on upstream as well, so it is recorded below as a preexisting limitation, not a repair finding.

Reviewed on 2026-11-13. Scope: `qdl/src/lib.rs` response, welcome, read and modifying-response paths; `qdl/src/types.rs`; `qdl/src/parsers.rs`; `qdl/tests/firehose_io.rs`; and the shared scripted transport. Inputs were the immutable `source/` snapshot, `source.patch`, `repair.diff`, approved `design.md`, and `previous-review.md`. R1 and CLI input adaptation were outside this review assignment.

Input hashes:

- `source.patch`: `d2c7f72927b4878faf634792aa221021457711f9aa47b33d5964e5c0c4466f12`
- `repair.diff`: `ec0cbf237bed930191b33a2d095de5083497f000fc777fa223fdea25a3a0aabf`
- Upstream baseline identified by the supplied inputs: `75fe2f7e42546a1d75969040378d4616419a9dcf`.

## Findings and closure

No P0, P1, P2, or P3 repair finding.

**R2 — closed.** At [lib.rs:712](../source/qdl/src/lib.rs#L712), `firehose_read_storage` retains the response attributes long enough to decide whether an initial NAK can become `ReadRejected`. The checks at lines 715–719 reject any presence of `AttemptRestart` or `AttemptRetry`, and any `rawmode` value other than the exact string `false`. Thus absent/false rawmode without hints remains the distinguishable ordinary rejection; restart/retry hints, true, empty, and invalid rawmode values become other errors, whose contract requires stopping the session. The error includes the attributes. The function sends no retry/reset and does not consume raw bytes on these errors.

This restriction is local to the initial read NAK. A final read NAK after raw data is still an error, never `ReadRejected` ([lib.rs:748](../source/qdl/src/lib.rs#L748)). The common response function still preserves complete NAK status even when hints are present ([lib.rs:326](../source/qdl/src/lib.rs#L326)), so program initial/final NAKs and patch, set-bootable, and explicit reset NAKs remain `MutationConfirmation::Rejected`. The `ReadRejected` documentation at [types.rs:60](../source/qdl/src/types.rs#L60) matches this contract. No general session framework or automatic recovery was introduced.

The submitted tests at `firehose_io.rs:368`, `:387`, and `:670` cover ordinary rejection, relevant hints/rawmode values, initial/final program NAKs, empty output, and absence of extra writes after drop. Independent probes additionally covered every split point for 11 attribute combinations, preceding logs, preserved unread tail bytes, all final-read NAK variants, and hinted NAKs for patch/set-bootable/reset.

**R3 — closed for the reported idle-timeout and inter-document scenarios.** At [lib.rs:248](../source/qdl/src/lib.rs#L248), only welcome parsing with an empty pending document discards the four XML whitespace bytes (`space`, `tab`, `CR`, `LF`). Once any non-whitespace document byte has entered `pending`, the new branch cannot run until the next document. Partial XML therefore still errors on timeout, malformed completed XML still goes through the existing XML parser, non-XML whitespace is not discarded, and EOF remains an error. XML size checks still apply to the document.

The branch is disabled for command responses. Initial command ACK parsing returns at `</data>` without consuming following storage bytes. The submitted tests at `firehose_io.rs:462` through `:519` cover the reported whitespace forms, fragmented whitespace before an XML declaration, partial/invalid tails, command log timeout, and leading storage whitespace. Independent probes also passed every split of a two-document welcome stream, byte-at-a-time delivery, interrupted reads, malformed completed XML after whitespace, nonbreaking-space/vertical-tab/form-feed rejection, XML size limits, EOF after whitespace, and all-whitespace raw storage at every split point.

## Regression sufficiency

The added tests detect the actual repaired behavior rather than merely duplicating the new implementation. In a private copy, I reversed only the production `qdl/src/lib.rs` and `qdl/src/types.rs` hunks from `repair.diff` while retaining the submitted tests. The same 45-test Firehose suite then produced **42 passes and three expected failures**:

- `read_nak_with_restart_retry_or_raw_mode_requires_session_stop`
- `welcome_accepts_trailing_xml_whitespace_before_idle_timeout`
- `welcome_accepts_fragmented_whitespace_between_declared_documents`

See [reversion log](scratch/prerepair-reverse.log) and [pre-repair regression results](scratch/prerepair-regressions.log). On the repaired production snapshot, all 45 pass. The tests are sufficient to close the two reported regressions. The private probes strengthen edge coverage but are not part of the submitted patch.

`ScriptedIo` uses a shared cursor for `Read` and `BufRead`; consuming XML cannot silently reset raw-data position. Unexpected extra reads panic when the script is exhausted. The shared trace survives drop and distinguishes attempted writes from accepted bytes. Those properties make the boundary and no-followup assertions meaningful, while still providing no evidence about a physical controller.

## Preexisting limitation observed, not a repair finding

An explicit `INFO: End of supported functions` welcome log followed by CRLF returns immediately at [lib.rs:374](../source/qdl/src/lib.rs#L374), leaving CRLF buffered. If the next response begins with `<?xml ...?>`, ordinary response parsing rejects the declaration because the retained whitespace precedes it. The additional `sentinel_welcome_trailing_whitespace_handoff_diagnostic` fails on the repaired snapshot with that exact XML parser error.

I reconstructed upstream in a private copy by reversing the complete supplied `source.patch`; all reverse hunks applied. An equivalent test using upstream's original `firehose_read` for welcome collection reproduces the same error. See [baseline reverse log](scratch/baseline-reverse.log), [baseline diagnostic source](scratch/baseline-source/qdl/tests/review_sentinel.rs), and [baseline result](scratch/baseline-sentinel.log). This is an existing handoff limitation, not introduced by R2/R3. The prior R3 report specifically described welcome collection without the explicit end sentinel, followed by idle timeout; that defect is fixed. This review does not claim all XML framing behavior is now correct.

Generic ACK/rawmode validation remains outside these findings, as expressly excluded by the assignment and prior review. The repair changes initial NAK classification; it does not establish a general rawmode state validator.

## Executed checks and limits

Host: `aarch64-apple-darwin`; Rust `1.99.0 (b940084d7 2026-09-28)`. All Cargo invocations used `--offline --locked`, and all compilation/probe changes were inside `protocol/scratch/`.

| Check | Result | Evidence |
| --- | --- | --- |
| `cargo test --offline --locked -p qdl --all-features --test firehose_io` on repaired copy | Exit 0; 45 passed | [log](scratch/firehose-existing.log) |
| `cargo test --offline --locked -p qdl --all-features --test review_protocol` | Exit 101; seven probes passed, one preexisting sentinel diagnostic failed as described above | [probe source](scratch/source/qdl/tests/review_protocol.rs), [log](scratch/review-probes.log) |
| Same submitted Firehose suite with R2/R3 production hunks reversed | Exit 101; 42 passed, three expected regression failures | [log](scratch/prerepair-regressions.log) |
| `cargo test --offline --locked -p qdl --all-features --test review_sentinel -- --nocapture` on reconstructed upstream | Exit 0; one test confirmed the existing sentinel failure | [log](scratch/baseline-sentinel.log) |

This reviewer did not run the full workspace suite, formatting, Clippy, Windows/Linux checks, or physical-device tests. No hardware enumeration/opening, application CLI execution, network changes, commits, or pushes occurred. The real fork and immutable input snapshot were not edited. No other current reviewer reports were inspected. Context was independently supplied, but the filesystem is shared; no strong isolation claim is made.
