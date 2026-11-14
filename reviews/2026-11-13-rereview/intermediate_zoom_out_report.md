# Stage 8: Intermediate Zoom Out Re-review

**Decision:** Pass architecturally. No new Blocker, Major, or Nit findings; no R2-Z8 IDs assigned.
**Reviewer:** `/root/r2_stage8_zoom`, fresh parent-turn context, completed.
**Target:** `docs/designs/oplus-edl.md`, lines 1–375.
**Standard:** `zoom-out-standard.md`.

## Independent challenges

Final-content verification could become a firmware simulator. The design instead assigns overlaps to later writes, treats DONT_CARE as no new write, and leaves unknown patch contents unverified. Unknown patch positions conservatively exclude the LUN from verification. This preserves readback without expression evaluation (116–126).

Failure reporting could become a general execution framework. The proposed failure type carries only not-sent, rejected, and unconfirmed outcomes. The result object concerns one invocation and creates no job store or recovery engine (169, 263–296, 322).

Portable directory mapping could recreate a partition-identity system. Explicit LUN directories, isolated GPT captures, and rejection of unsupported names solve the retained workflow without requiring every possible partition name to work (243–259).

Offline tests could require a new transport architecture. The design reuses its stated existing interfaces, separates opening from using a session, and keeps scripted faults in tests. Separate repository helpers do not justify another shared package yet (175–188).

None of these challenges establishes a new scope or simplicity defect.

## Prior-action disposition

| Action | Disposition | Current evidence |
| --- | --- | --- |
| A1 | Closed at design level | 112–128, 281–296: mode policy separates continuing execution from a successful command result. |
| A2 | Closed at design level | 110–112, 126, 169, 283, 351–357: later modifications and resets stop; read failures preserve prior write confirmation. |
| A3 | Closed at design level | 82–94, 243–259: concrete mapping and rejection rules without provenance or layout proofs. |
| A4 | Closed architecturally | 263–322: execution, confirmation, verification, skips, and aggregate outcomes remain separate. Serialization still needs later implementation tests. |
| A5 | Documentation gap closed; upstream suitability unverified | 175–188, 214–218, 229–232: named interfaces, faults, observations, and test files without a general framework. |

## Confirmations and scope

The reviewer found no basis to overturn the Stage 5 or Stage 7 architectural conclusions. Detail is placed in the implementation section and addresses retained requirements. A4 closure does not replace eventual serialization tests, and A5 closure does not mean upstream signatures were independently verified.

The design adds no manifest, journal, saved plan, automatic resume, identity proof, resource database, patch simulator, plugin system, or fastboot integration. The product/fork responsibility split remains suitable. Intermediate writing commands stay unavailable until their stopping and verification behavior is complete. RMX3800 remains outside the general release gate.

## Read inventory and limits

The reviewer reported complete reads of its persona, standard, target (375 lines), concept brief (207 lines), original consolidated report (169 lines), and the three preceding reports from this re-review. Bounded follow-up reads recovered truncated output; no required read remained missing.

Local inventory and symbol searches found no production source, Cargo, README, or context implementation files. No unrelated draft, recursive link, external source, history, or memory was read. No edits, builds, device operations, or nested agents occurred. Upstream behavior, signatures, licenses, platform compatibility, and device support remain unverified.
