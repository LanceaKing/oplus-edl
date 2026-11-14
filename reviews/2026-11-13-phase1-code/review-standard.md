# Phase 1 code-review standard

Review current code against baseline 75fe2f7e42546a1d75969040378d4616419a9dcf and the approved design. This is one independent review round, not a design rewrite or repair loop.

Find actionable correctness defects introduced by the changes, or failures to repair an explicitly required phase-1 defect. For each finding provide a stable ID, P1/P2/P3 severity, exact current-source file and line range, concrete trigger, observed or reasoned consequence, minimal repair direction, and regression coverage needed. Distinguish executed reproduction from static inference. Do not inflate hypothetical risks, preexisting out-of-scope bugs, or style preferences into blockers.

The scope is protocol correctness, offline regression sufficiency, and preservation of the approved lean first release. Do not demand phase 2-6 product workflows, verification policy, cross-platform runs, hardware tests, manifests, databases, saved plans, patch simulation, or unrequested redesign. Tests passing does not prove behavior outside their exercised boundary. Check integration into real backends/callers, not only extracted pure helpers.

Read your persona, this standard, the full design, the full patch, and the relevant complete source/test files. Baseline source is available read-only with git -C /Users/Apple/Documents/qdlrs show <baseline>:<path>. Locked dependency source under /Users/Apple/.cargo/registry/src is available for verifying API behavior. Use primary sources if external research is necessary.

Do not edit the real fork, design, or archived implementation. Do not commit, push, contact others, enumerate/open hardware, or invoke device CLIs. Write only your assigned report and scratch/probes below your assigned reviewer directory. You may copy the snapshot to your scratch for offline tests. Do not read other reviewer reports or parent conversation/memory. Fresh conversation context does not isolate the shared filesystem.

Return status completed/partial/failed, read inventory, findings, executed checks, unrun checks, and limits. A missing required read is incomplete, not a clean result. Report evidence even if no findings.
