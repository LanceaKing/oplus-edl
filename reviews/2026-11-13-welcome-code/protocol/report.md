# Welcome handoff protocol review

Status: **completed for the scoped offline protocol review**. No actionable P0–P3 findings were established in the patch. No production files were edited. This is not a hardware or full protocol-conformance result.

The reviewed input is the immutable `source/` snapshot and `source.patch` relative to `bd136c484ba4c21e09c14230d10cd0c8956a2fcf`. All 34 manifest entries, including the patch, matched before testing. At completion, every original file in both `source/` and the private `protocol/current/` copy still matched the manifest. The baseline copy was obtained by reversing the exact supplied patch. Evidence: [input hashes](logs/input-hashes.json), [reverse-patch log](logs/baseline-reverse.log), and [final integrity checks](logs/final-source-integrity.json).

## Findings and required answers

**Raw-data preservation:** the changed whitespace handling does not skip bytes within a valid declared raw-data range. The skip occurs only while the XML accumulator is empty (`source/qdl/src/lib.rs:249–257`). Finding `</data>` consumes exactly through that marker and immediately returns (`:263–270`), leaving all following bytes in the shared stream. The read operation subsequently requests at most the remaining raw byte count and writes those bytes unchanged (`:730–745`). It does not invoke the XML parser again until that count is exhausted (`:749`). This includes raw data beginning entirely with space, tab, CR and LF, and raw data containing XML-looking sequences.

The shared-position requirement is present in production, not only the test double: `QdlDevice` delegates both `Read` and `BufRead` to the same object (`source/qdl/src/types.rs:184–215`); serial and USB reads drain their existing buffers first (`source/qdl/src/serial.rs:25–61`, `source/qdl/src/usb.rs:79–114`). Fragmented `</data>` is retained in `pending`; a discovered marker cannot precede the old accumulator length because the preceding loop iteration would already have returned. Thus `end - old_len` does not consume bytes beyond the current document.

**ACK strictness:** a timeout, EOF, log, welcome end marker, or incomplete XML still cannot stand in for a command response. The timeout-as-success branch remains restricted to `welcome && pending.is_empty()` (`source/qdl/src/lib.rs:235–239`); command responses always call the reader with `welcome = false` (`:302`). `firehose_response` still requires one `response` element and invokes the unchanged ACK/NAK parser (`:303–326`, `source/qdl/src/parsers.rs:31–41`). Program retains both initial and final confirmation checks (`source/qdl/src/lib.rs:604`, `:646–650`); storage read retains initial status processing and a final ACK check (`:713–723`, `:749–751`). NAK is not converted to ACK. This conclusion concerns required ACK/NAK presence and existing status handling, not new validation of ACK `rawmode` attributes; generic ACK/rawmode state validation is unchanged and outside this patch.

**Caller path:** the CLI performs welcome collection, sends configure, then checks the parsed configure ACK (`source/cli/src/main.rs:294–305`). GPT, partition and XML read callers all use `firehose_read_storage`, so none bypasses the exact-length raw loop. Other command helpers use `firehose_write_getack` or the same response reader. The fix therefore reaches the reported configure handoff without adding commands, resets, retries, or another read after the welcome end marker.

## Coordinator-supplied short-raw counterexample

This case was supplied by the coordinator after the independent initial review. I recreated it in [protocol_short_raw.rs](current/qdl/tests/protocol_short_raw.rs), without reading another reviewer's report or relying on a supplied conclusion.

For a request for eight raw bytes, the stream is:

```text
initial ACK + b"1234567\r\n" + XML declaration + final ACK
```

Current code returns success with `b"1234567\r"`. The exact baseline returns `Invalid Firehose XML` because the remaining LF precedes an XML declaration. This acceptance difference is real and comes from applying prefix trimming to the final read response as well as the welcome handoff. The independently run expected-error hypothesis fails current and passes baseline. See [current reproduction](logs/current-short-raw-independent.log) and [baseline reproduction](logs/baseline-short-raw-independent.log).

I do **not** classify the observation alone as an actionable P2 regression. The identical byte stream also represents eight valid raw bytes `b"1234567\r"`, followed by one LF of XML framing. The host has no information that distinguishes this from seven source bytes followed by CRLF. The raw loop takes precisely eight bytes in both versions; the changed skip runs afterward. A control without the final XML declaration succeeds in both baseline and current, demonstrating the preexisting byte-stream ambiguity. The baseline's rejection depends on XML declaration placement, not a general raw-completeness check.

The new documentation expressly permits prefix whitespace on XML reads, and the new raw-preservation test deliberately includes CRLF before the final declared ACK (`source/qdl/tests/firehose_io.rs:476–498`). Under that framing policy, requiring this indistinguishable stream to fail would contradict a permitted valid stream. The change is broader than a one-time welcome flag, but this alone does not establish an unintended behavior or incorrect byte consumption.

If the product instead requires no framing whitespace after raw data, that boundary rule must be stated explicitly and tested independently of XML declarations. The current patch does not provide that stronger guarantee. In particular, the passing preservation tests must not be presented as proof that an incorrectly short sender payload followed by whitespace can always be detected. The counterexample contains a real final ACK and therefore does not demonstrate success without ACK.

## Offline evidence

All Cargo commands used `--locked --offline`, ran only test binaries in private copies, and used scripted I/O. No application CLI, hardware enumeration, device opening, or device operation was executed.

| Private copy and command | Observed result | Log |
| --- | --- | --- |
| Current: `cargo test -p qdl --test firehose_io --all-features --locked --offline` | 50 passed | [current Firehose](logs/current-firehose.log) |
| Baseline: same command | 45 passed | [baseline Firehose](logs/baseline-firehose.log) |
| Current: `cargo test -p qdl --test protocol_probe --all-features --locked --offline` | 7 passed | [private probes](logs/current-protocol-probes.log) |
| Baseline: same test target with `protocol_declared_handoff_and_raw_are_preserved_at_all_splits -- --exact` | Expected regression failure: XML declaration after leftover whitespace | [baseline handoff](logs/baseline-handoff-regression.log) |
| Current: `cargo test -p qdl --test protocol_short_raw --all-features --locked --offline -- --nocapture` | Expected-error hypothesis fails; undeclared-ACK ambiguity control passes | [current counterexample](logs/current-short-raw-independent.log) |
| Baseline: same counterexample command | 2 passed; declared case returns error across all two-fragment splits | [baseline counterexample](logs/baseline-short-raw-independent.log) |

The seven [private probes](current/qdl/tests/protocol_probe.rs) add whole-buffer, every two-fragment split, one-byte fragments, and one-byte fragments interleaved with `Interrupted`; both serial and USB configuration values on scripted I/O; every possible first raw byte; embedded ACK/NAK/`</data>` and binary bytes in payloads; preservation of the next response; initial/final read and program failures for whitespace, partial XML, logs, NAK and malformed responses; and initial read NAK boundary preservation. They also check that welcome returns without touching any trailing bytes. An initial compile error in the reviewer-only test's slice concatenation was corrected; its log is retained as [probe authoring error](logs/current-protocol-probes-initial-compile-error.log).

The five added checked-in tests are useful regression coverage: the configure test covers coalescing, every two-fragment split and one-byte delivery; the raw test compares the exact output and a following NAK; negative tests cover timeout, EOF, invalid whitespace and malformed XML; the size test checks the XML document limit after prefix removal. They do not establish a new overall framing-byte budget or detect all malformed raw-stream length scenarios. Neither claim is necessary for the documented welcome handoff fix.

## Limits

This review inspected the full production XML reader, response and welcome processors, read/program and other response callers, configure parser, CLI handoff, and transport buffer ownership. Testing used the supplied fake transport; setting its backend field does not emulate USB transfer timing or prove actual serial/USB behavior on a device. I did not run a workspace build matrix, hardware tests, or application commands. XML prefix whitespace is outside the document size bound, and a continuously active sender is not subject to an overall command deadline here; the patch does not claim otherwise. Broader protocol hardening, including generic ACK/rawmode state validation and stricter malformed-sender framing rules, remains separate from this review's conclusion.
