## What this proposes

The document proposes a new Rust command-line tool for backing up, restoring, and flashing devices through Sahara and Firehose. It will build on a pinned, corrected qdlrs fork and selected behavior from two other reference tools. The first release accepts ordinary partition images, checks inputs and transfer results, attempts readback by default, and reports unsupported unlock or lock operations without accessing the device.

## How the existing system works, per this document

This project has no production implementation yet. The reference projects provide the existing behavior: qdlrs supplies protocol and transport support, bkerler/edl supplies partition backup and restore patterns, and linux-msm/qdl supplies firmware XML and sparse-image examples. Static inspection identified defects in the pinned qdlrs version, but this design work has not reproduced them on a device.

## Findings

- **C6-01 — Major — Lines 111–115 and 210.** The command contract offers `--verify best-effort|required|none`, but the document never explains the difference between `best-effort` and `required`. The verification section provides one reporting policy for unavailable readback and ranges whose expected contents cannot be calculated, so a reader cannot tell what choosing `required` changes. **Minimal remedy:** Define each mode’s effect on execution and the final result, including unavailable readback and ranges with no calculable expected contents.

- **C6-02 — Nit — Lines 83, 205, and 207.** The backup description says a full operation traverses all discovered storage units, while the command contract says the default backs up the “selected units.” The document does not state which units are selected when `--lun` is omitted. **Minimal remedy:** State the default LUN selection explicitly and connect that rule to the meaning of a full backup.

## Read-completion inventory

- Read the complete supplied comprehension persona.
- Read the complete target document, lines 1–237, in two untruncated reads.
- Covered FeatureRange 1–133 and DetailedRange 134–237.
- Consulted no other project files, linked sources, reports, or history. Made no changes and performed no builds or device operations.

## Access failures or limitations

No access failures. This review assesses the document’s self-containment and connections between sections; it does not verify the behavior of the linked reference implementations.

## Execution record

Stage 6 completed by `egm-comprehension` (`/root/stage6_comprehension`) in a fresh parent-turn context. No standard or GoldfishContext was supplied. The complete reviewer result is retained above.
