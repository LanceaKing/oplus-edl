# Desktop migration

The desktop uses the same `CoreService` as the CLI. Checkpoint D adds release packaging. Checkpoint A used an in-memory scripted adapter for shell qualification. [Approved design](../designs/oplus-edl-desktop.md#detailed-implementation) · [Desktop guide](../desktop.md)

## Run the desktop workspace

Run the app from the repository root after caching the locked dependencies.

```sh
cargo run --locked --offline -p oplus-edl-desktop
```

## Build the release archive

Fetch the locked dependencies. Run `sh scripts/package-desktop.sh` on an arm64 Mac or x86_64 Ubuntu host. Run `./scripts/package-desktop.ps1` in PowerShell 7 on x86_64 Windows. The scripts verify the archive before placing it in `dist/`. [Development guide](../development.md) · [Distribution guide](../distribution.md)

## Checkpoint A capability mapping

| Capability | Checkpoint A view | CLI path at Checkpoint A |
| --- | --- | --- |
| USB and serial selection, loader startup | Loader and optional signed materials. Simulated UFS state. | `--backend`, `--device`, `--loader`, `--signed-digest`, `--digest-signature` |
| Partition inspection and backup | Simulated GPT with 10,000 rows. Explicit selection and backup folder. | `backup --output ...` with optional `--partition` |
| Image and directory restore | Sample image mappings and write confirmation. | `restore --image ... --partition ...` or `restore --from ...` |
| XML and package flash | Ordered sample steps, exclusions, NV ID, and write confirmation. | `flash --program ... [--patch ...]` or `flash --package ... --nv-id ...` |
| Offline package preparation | Planned package inputs and output selection. | `flash --package ... --nv-id ... --prepare-only --prepare-output ...` |
| Cloud acquisition | Planned offline acquisition action. | `flash --cloud-config ... --cloud-output ... --cloud-fetch-only` |
| Verification and final actions | Sample transfer and readback counts. Final device actions are pending. | `--verify`, flash `--set-bootable`, flash `--reboot` |

The `prototype` feature supplied the scripted adapter in Checkpoint A. Checkpoint B moved the shared core and CLI. Checkpoint C removed the feature and connected the desktop views to the shared service.

## Build inputs

- eframe, egui, and egui_extras 0.37.0. eframe uses Glow, X11, Wayland, clipboard, and AccessKit features. [eframe crate documentation](https://docs.rs/eframe/0.37.0/eframe/)
- rfd 0.18.0. It uses native file and folder dialogs and the XDG Desktop Portal backend on Linux. The macOS dialogs open from the window thread. [rfd crate documentation](https://docs.rs/rfd/0.18.0/rfd/)
- Noto Sans CJK SC Regular 2.004. The font bytes come from revision `523d033d6cb47f4a80c58a35753646f5c3608a78`. SHA-256 is `2c76254f6fc379fddfce0a7e84fb5385bb135d3e399294f6eeb6680d0365b74b`. Copyright © 2014–2021 Adobe. [Upstream font and license](https://github.com/notofonts/noto-cjk/tree/523d033d6cb47f4a80c58a35753646f5c3608a78)
- The CLI in the qualification archive used `wreq` 0.16.3 and qdl at `205c7d4d9f15add22d78c422ba77843ab72c95d4`. The workspace uses the qdl repair commit recorded below.

The macOS build sets a deployment target of 14.0. Test the archive on macOS 14 before claiming support at that version. The CLI build uses the native dependencies listed in CI. On Linux, test dialogs with the GNOME XDG Desktop Portal and Zenity installed. [CI configuration](../../.github/workflows/ci.yml)

The workspace owns three packages. `oplus-edl-core` owns `device`, `firehose`, `firmware`, `input`, `package`, `project`, `public_api`, `result`, `session`, `storage`, `workflows`, and `writes`. `oplus-edl-cli` owns `cli` and the independent `json_stdout` executable with `harness = false`. The package CLI parse and subprocess cases moved into `cli`. `oplus-edl-desktop` owns `app`, `frontend_parity`, and its picker, preferences, and diagnostics unit tests. The core `service` target covers the two work slots and loopback acquisition.

## Native qualification

The procedures and results in this section record Checkpoint A shell qualification. Checkpoint D acceptance uses the extracted production package on each selected host. [Current acceptance checks](../testing.md#检查点-d-发布包)

Use the same archive for startup, layout, input, dialogs, and timing. Record the host OS, architecture, display scale, toolkit inputs, and archive checksum. Run the extracted app from an ordinary user account.

1. Set the client area to 1280 × 800 logical pixels. Drag both dividers. Make sure that every mode and its primary action remain reachable.
2. Repeat at 1024 × 768 and at 100%, 150%, and 200% scale. Open the collapsed input and status panels where applicable.
3. Use the native Chinese input method in a path field. Type and paste paths with spaces. Use Tab to move between fields and buttons.
4. Select a file and a folder. Cancel each dialog. Make sure that the existing paths stay in their fields after cancellation.
5. Start the 30-second job with 10,000 rows. Measure the time from input to its visible effect for selection, scrolling, and Stop.
6. Repeat the delayed job without requesting a stop. Make sure that the final result remains visible. Start another job and close the window during work.

Selection, scrolling, and Stop must respond within 250 ms. Measure Stop at the displayed stop request. The delay until a terminal result depends on the current work step. Test both GNOME X11 and GNOME Wayland on Ubuntu 24.04.

| Host | Evidence |
| --- | --- |
| macOS 26.6.2 arm64 | Extracted archive launch, three modes, layout, keyboard focus, clipboard, native dialogs, and delayed work. See the results below. |
| macOS 14 arm64 | Pending selected baseline. |
| Windows 11 x86_64 | Desktop `cargo check` for the MSVC target passed from macOS. Native package tests are pending. |
| Ubuntu 24.04 x86_64 GNOME on X11 and Wayland | Desktop `cargo check` for the GNU target passed from macOS. Native package tests are pending. |

### macOS local results

The test host used an Apple M1 Pro, macOS 26.6.2 build 25G83, Rust 1.99.0, and macOS SDK 27.0. The Retina display used two pixels per point. We launched the extracted app through Launch Services. The bundled CLI returned `oplus-edl 0.1.0` from `--version`.

| Test | Result |
| --- | --- |
| Panel and action reachability | Both side dividers moved. All three modes and primary actions were reachable at both window sizes and all three in-app scales. |
| Input and dialogs | Tab moved to the browse button. Chinese paths with spaces survived paste, native file and folder selection, and picker cancellation. |
| Delayed job | The full job finished in 30.16 s. Clearing the visible log preserved the result. |
| Responsiveness with 10,000 rows | Selection 30.5 ms, scrollbar 153.3 ms, and Stop 218.0 ms. Timing used event injection and observed accessibility state. |
| Normal window close during work | The window showed the stop request. The process exited 0.79 s after the click. |
| Write confirmation | Escape dismissed the restore summary. One approval produced distinct simulated acknowledgement and readback counts. |
| Backup and flash | The scripted backup completed for one selected partition. Both XML and extracted-package confirmation completed. The package summary kept the leading zeros in the NV ID. |
| Native input composition | The synthetic Pinyin keystrokes produced literal letters in both the desktop app and a native Cocoa text field. Physical input method testing remains pending. |

Physical wheel and trackpad checks remain pending. Record these with the native baseline runs. The local workspace passed format, Clippy, build, and offline test checks. The test run passed 138 tests and also ran the `json_stdout` executable. One live cloud probe remained ignored.

The packaged desktop executable has SHA-256 `92583b749cc0f810a39b99894ac4882102b66447b49f893f23d55f97b7077d3a`. Local transcripts and window captures are under `runs/desktop-qualification/`.

A later archive check matched that executable hash. The preserved prototype ZIP is 19,614,773 bytes. Its SHA-256 is `2f45b9fd67fe2b458fa528587af4fbb9e2ad95c9aaa6a765ff4d697c336cc9bf`. [Prototype ZIP](../../runs/desktop-qualification/checkpoint-a-prototype-macos-arm64.zip) · [Run transcript](../../runs/desktop-qualification/macos-ui.log)

## qdl prerequisite

The product manifest and lock pin qdl to `9161c0f46cef245f3b5fc5394dba1ec5e8c3f6ac`. The DCO-signed commit is available on `LanceaKing/qdlrs` branch `codex/desktop-response-wait`. Its diff from `205c7d4d9f15add22d78c422ba77843ab72c95d4` matches the reviewed handoff patch. The repair adds a USB refill for XML responses and deadline checks around refills and after complete logs. The author closed round eleven and carried five Major findings and three Nits forward. [Signed repair commit](https://github.com/LanceaKing/qdlrs/commit/9161c0f46cef245f3b5fc5394dba1ec5e8c3f6ac) · [Review and handoff](../../reviews/2026-11-17-qdl-response-wait/round-11/goldfish_review_report.md#作者处置与交接)

Format, Clippy, build, and offline tests passed on the signed qdl checkout and in the product workspace. The qdl checkout passed 111 workspace tests and also passed 14 `response_stop` tests in a separate run with default features disabled. The product passed 138 tests and the independent `json_stdout` test. One live cloud probe remained ignored. Cargo verified locked resolution to the repair and `nusb` 0.2.9 for macOS arm64, Windows x86_64, and Linux x86_64. [Pinned dependency verification](../../runs/desktop-qualification/qdl-response-wait-handoff/pinned-dependency-verification.log)

## Checkpoint B response wait integration

The product CLI now submits device work to `CoreService`. The device worker keeps the transport and uses the same monotonic clock as the job handle. It records the first accepted stop time and reason. The qdl response hook enforces the ten-second grace period using this time. `CountChannel` forwards both qdl hooks, and `digest_welcome` uses the response refill with deadline checks around attempts and after complete logs. [Service](../../crates/oplus-edl-core/src/service.rs) · [Session](../../crates/oplus-edl-core/src/session.rs) · [CLI adapter](../../crates/oplus-edl-cli/src/run.rs)

Initialization, storage discovery, GPT and OCDT reads, backup, program, verification, and final actions check stop at their transaction boundaries. The terminal result retains completed steps, ACKs, byte counts, owned partial outputs, and typed expiry errors. The service releases a failed or explicitly closed session before making its result available. The `partitions` command shares one job between initialization and inspection. The CLI relay and renderer use that job handle and its result. [Session regressions](../../crates/oplus-edl-core/tests/session.rs) · [CLI regressions](../../crates/oplus-edl-cli/tests/cli.rs)

The workspace passed 167 tests and the independent `json_stdout` check. The live cloud probe was ignored. The isolated `public_api` and `session` targets passed 2 and 18 tests, respectively. Format, Clippy, and build passed. The selected native platform tests and USB latency measurements for a read already in flight remain pending. [Offline verification](../../runs/desktop-qualification/checkpoint-b-response-stop-offline.log)


## Checkpoint B CLI catalog and prepared-package reuse

The product executable uses the approved command catalog. Device discovery selects one backend. Backup and directory restore require explicit targets or `--all`. Local initialization and integrated cloud acquisition use distinct argument groups. Offline package preparation and cloud acquisition run as separate commands. Cloud requests resolve relative paths against the invocation directory and open an existing SWM identity by default. [CLI guide](../cli.md) · [Argument parser](../../crates/oplus-edl-cli/src/args.rs)

Package preparation and reopening share the selection logic. Reopening compares `selection.json` with the original package selection, checks the generated XML and original patch operations, and validates the sparse super image against the LP layout. New manifests record the selected patch operations for comparison. Reopening older manifests uses their original patch paths and validates the current patch operations and ranges. A reopened plan retains its package guard and uses a new in-memory consent decision. Metadata checks retain the opened sources through execution and comparison. [Package preparation and reopening](../../crates/oplus-edl-core/src/package.rs) · [Execution checks](../../crates/oplus-edl-core/src/write.rs)

The two-process CLI regression prepares an SFP fixture offline and reopens it in a fresh CLI adapter process with a scripted device. A project mismatch terminates after GPT and OCDT reads and leaves the prepared outputs intact. Further cases check missing or mixed evidence, reordered or modified XML, relocated bindings, changed sources, explicit consent, and backup output in all four modes. The native platform acceptance matrix and in-flight USB timing measurements remain pending.

The locked offline workspace run passed 184 tests and the independent `json_stdout` check. The live cloud probe remained ignored. Runs without default core features passed 2 `public_api` tests, 22 `session` tests with `test-support`, and 11 `package` tests. Format, workspace Clippy and build, and the Windows CLI cross-target check passed. [Offline verification](../../runs/desktop-qualification/checkpoint-b-cli-prepared-offline.log)


## Checkpoint C real desktop adapter

The desktop submits discovery, connection, inspection, backup, restore, XML flash, package flash, offline package preparation, and cloud jobs to the shared service. A healthy session continues across operations. An explicit disconnect or window close waits for the device worker to release the channel. The desktop binds selected partitions to their session and GPT snapshot. Fresh GPT reads reject a changed layout before selected data is accessed. Each write uses the accepted job's preparation and one decision. [Desktop adapter](../../crates/oplus-edl-desktop/src/app.rs) · [Core service](../../crates/oplus-edl-core/src/service.rs)

Package reopening retains package origin, device project matching, range checks, exclusions, and the new consent requirement. The desktop keeps typed paths, remembers the chosen cloud base directory, offers a separate SWM identity creation action, and exports diagnostics to a local file. Log eviction leaves terminal results available. [Desktop guide](../desktop.md)

The offline parity suite compares complete operations from the real adapters. It checks setup inspection separately from backup and restore. It compares ordered commands, transfer counts, item outcomes, outputs, verification, unknown writes, and response-wait expiry. The GUI Stop and CLI signal cases retain their expected distinct reasons. Loopback cloud tests run the legacy enrollment, list, and ticket requests and reject an invalid HMAC before asset publication. Native platform acceptance and in-flight USB read timing remain pending. [Adapter parity tests](../../crates/oplus-edl-desktop/tests/frontend_parity.rs)

The locked offline workspace run passed 211 tests and the independent JSON stdout check. One live cloud probe remained ignored. The isolated desktop run passed 8 app tests and 12 adapter parity tests with native transport features disabled. The isolated core runs passed 5 public API, 22 session, and 11 package tests. Format, Clippy, workspace build, and the explicit desktop production-feature check passed. [Offline verification](../../runs/desktop-qualification/checkpoint-c-desktop-offline.log)

## Checkpoint D release packaging

The Windows ZIP contains the GUI and CLI executables. The macOS ZIP contains `Oplus EDL.app` and the CLI. The Ubuntu tar.gz contains both executables and a desktop entry. The release scripts build both frontends offline with lockfile enforcement, defaults disabled, and explicit `usb`, `serial`, and `cloud` features. They check the production dependency graph, record the selected features and lockfile hash, and verify each archive before placing it in `dist/`. [Distribution guide](../distribution.md) · [Build scripts](../../scripts/)

CI is configured to build and test on Windows Server 2025 x86_64, macOS 15 arm64, and Ubuntu 24.04 x86_64. The macOS binaries and app metadata target macOS 14.0. The packaged application uses an ad hoc signature to seal the bundle metadata and executable. [CI configuration](../../.github/workflows/ci.yml) · [Apple code-signing documentation](https://developer.apple.com/documentation/security/seccodesignatureflags/adhoc?language=objc)

The initial local macOS 26.6.2 arm64 run built and extracted `oplus-edl-0.1.0-macos-arm64.zip` with Rust 1.99.0. This local evidence is preserved in `runs/desktop-qualification/checkpoint-d-initial-macos-arm64.zip`. The initial ZIP is 23,783,297 bytes and has SHA-256 `f2887b490a4f1044e14ad0639cefd662c58f10a764f091f11118545773a3878b`. All 542 payload file hashes matched. Both executables are arm64 with deployment target 14.0. The extracted app bundle and CLI passed strict code-signature verification. The extracted CLI printed version 0.1.0 and returned one successful JSONL result for `flash package --help --json -q`.

Format, Clippy, and workspace build passed. The locked offline workspace run passed 211 tests and the independent `json_stdout` check. The live cloud probe remained ignored. Separate runs passed 5 public API, 22 session, 11 package, 8 desktop app, and 12 frontend parity tests. Windows and Ubuntu passed locked dependency graph and archive layout checks using synthetic PE and ELF headers. Their production archive builds, all clean-host desktop acceptance, and native in-flight USB read timing remain pending. [Local validation log](../../runs/desktop-qualification/checkpoint-d-release-packaging.log)

## Implementation review round 1 repairs

The author accepted all 18 findings from the first independent implementation review. B checks stop after local I/O and before patch, set-bootable, reboot, and backup dispatch. A blocked second primary GPT read leaves the probe bytes intact. An explicit contradictory raw mode on a configure or storage-query ACK closes the session before another command. The inspection result reports actual GPT scope. Preparation and cloud publication record owned files as they are created and update their state after writing or publishing. [Round 1 report](../../reviews/2026-11-18-desktop-implementation/round-1/goldfish_review_report.md) · [Session boundary regressions](../../crates/oplus-edl-core/tests/session.rs)

Integrated cloud acquisition starts on the reserved acquisition worker and shares the device job's stop state. SWM checks the same state before identity access and each new request. The CLI now samples known byte progress, keeps warnings and actual output paths in both terminal formats, and applies the bounded renderer to help, version, and early errors. A broken pipe requests `ConsumerLost`. Early argument recovery preserves an independent `--json` when a value is missing. `flash xml` keeps the selectable RAW policy; package flows use the file-sized policy. [Worker regression](../../crates/oplus-edl-core/tests/service.rs) · [CLI regressions](../../crates/oplus-edl-cli/tests/cli.rs)

The desktop retains selections after refresh only when the generation, entry, name, and geometry match. It shows start and length before approving a write, preserves non-Unicode paths until explicit replacement, and exports diagnostics on a background worker. Scripted runs check reuse with an actual next operation after backup, successful verification, and mismatched verification. Tests cover source metadata disappearance and a fixed SWM base from two launch directories. [Desktop regressions](../../crates/oplus-edl-desktop/tests/app.rs) · [Adapter parity](../../crates/oplus-edl-desktop/tests/frontend_parity.rs)

The native test guide gives the extracted production app a repeatable absent-device wait for Stop and normal close. The separately built same-views qualification example loads a scripted 10,000-row GPT and delays offline package preparation by 30 seconds. Its offline fixture test checks the real desktop adapter and reported row count. CI records the checked-out commit and logs each archive's SHA-256. Record the CI run link with evidence from the fixed candidate. Local arm64 output remains evidence from a working tree. [Native procedures](../testing.md#检查点-d-发布包) · [Qualification source](../../crates/oplus-edl-desktop/examples/native-qualification.rs)

Format, workspace Clippy, build, and 230 locked offline workspace tests passed. The independent JSON stdout check also passed. The live cloud probe remained ignored. Separate runs passed 5 public API, 27 session, 11 package, 9 desktop app, 12 parity, and 1 qualification fixture tests. The [first-repair macOS arm64 ZIP](../../runs/desktop-qualification/implementation-round-1-macos-arm64.zip) is 23,848,356 bytes, with SHA-256 `b0bf223b5019414648ebdcf2436725b0e1bd85263be1f4b57b2bf4c89495d27b`. All 542 payload hashes matched after extraction. The CLI and app bundle passed strict signature verification. Both executables target macOS 14.0. The extracted CLI reports `oplus-edl 0.1.0` and passes JSONL help and early-error checks. Windows and Ubuntu production archives, the native acceptance matrix, and in-flight USB timing remain pending. [Offline and archive verification](../../runs/desktop-qualification/implementation-round-1-repair.log)

## Implementation review round 2 repairs

The author accepted all 16 findings from the second independent implementation review. When replacing a session, the service invalidates the old generation, releases the old transport, and checks stop before opening another channel. The standalone Connect result and new session share the stop lock. A stop accepted before that commit releases the new transport and retains completed initialization steps. The GUI shows the accepted Stop through terminal collection and keeps the feedback through the next render. [Round 2 report](../../reviews/2026-11-18-desktop-implementation/round-2/goldfish_review_report.md) · [Session regressions](../../crates/oplus-edl-core/tests/session.rs) · [Desktop state tests](../../crates/oplus-edl-desktop/src/app.rs)

The service commits ready package and asset data only with success. The terminal report carries retained output paths and states after stop or publication failure. SWM records private identity ownership at exclusive open and completeness after sync. The work slots bind resolved identity and release paths before use. Offline cases drive real registration branching through a test-only exchange and verify retained identity outputs, completed-reply checks, and the absence of later challenge or ticket requests. [Service regressions](../../crates/oplus-edl-core/tests/service.rs) · [SWM regressions](../../crates/oplus-edl-core/src/swm.rs) · [Cloud publication regressions](../../crates/oplus-edl-core/src/cloud.rs)

The material loader reads the bound file handles and preserves single-image and CPIO newc decoding at the locked qdl revision. Regressions cover a Unix FIFO replacing the loader path and a same-directory source rename while a write awaits consent. An ambiguous terminal NAK retains unknown-write and byte facts. An ordinary backup with a completed ACK retains its completed item and exact byte count after a local publish failure. The same typed write slot drives confirmation and wire dispatch. [Core session regressions](../../crates/oplus-edl-core/tests/session.rs) · [Decoder parity](../../crates/oplus-edl-core/src/programmer.rs)

The human CLI and GUI render session reason and ordered initialization history from the terminal result. CLI tests exercise real subprocesses with a full unread stdout pipe and script output loss during a program response wait. The desktop saves the logical native client size and app zoom independently. The native guide identifies the checkbox, accepted stop, terminal result, and geometry observations. The USB timing procedure identifies the first stop and actual USB refill events, the original deadline, and the read-only terminal evidence. Both approved designs record ad hoc macOS signatures. [CLI regressions](../../crates/oplus-edl-cli/tests/cli.rs) · [Native acceptance guide](../testing.md#检查点-d-发布包) · [USB timing procedure](../testing.md#usb-在途读取计时)

Format, Clippy, build, 248 locked offline workspace tests, and the independent JSON stdout check passed. The live cloud probe remained ignored. Separate runs passed 5 public API, 34 cloud-disabled session, 11 package, 20 cloud-only core, 9 desktop app, 12 parity, and 1 qualification fixture tests. The [ZIP reviewed in round 3](../../runs/desktop-qualification/implementation-round-2-macos-arm64.zip) uses explicit `usb,serial,cloud` production features. It is 23,829,402 bytes with SHA-256 `1e47988a85c45ead41c66e32cd289bb8e55712adc7ddea553f50426d4119ac90`. All 542 payload hashes matched. Both extracted executables target macOS 14.0 and passed strict signature verification. The extracted CLI passed normal JSONL delivery and exited 74 after approximately 5.02 seconds for help, version, and a parse error with a full unread stdout pipe. The standalone qualification executable built for macOS 14.0 arm64. Windows and Ubuntu production archives, three-platform native acceptance, and in-flight USB timing remain pending. [Round 2 repair verification](../../runs/desktop-qualification/implementation-round-2-repair.log)

## Implementation review round 3 repairs

The author accepted all seven findings from the third independent implementation review. Connection materials are rechecked after device discovery, after old-channel release, and after the new backend opens before the first initialization exchange. Offline cases change each of the three materials during discovery and change a loader while the old channel releases or the backend opens. They verify the changed path, zero replacement opens at the pre-open boundaries, and channel release with no upload at the post-open boundary. [Round 3 report](../../reviews/2026-11-18-desktop-implementation/round-3/goldfish_review_report.md) · [Connection regressions](../../crates/oplus-edl-core/tests/session.rs)

Backup resolves and reserves the actual LUN and GPT output directories in the path overlap check before file creation or GPT or partition reads. Unix symlink regressions exercise LUN and GPT aliases separately with both SWM and backup starting first. The shared request distinguishes `PartitionTargets::All` from `Selected`. An empty Selected request fails before worker or provider access. The no-default-feature public API regression backs up all eligible partitions on a discovered LUN, then restores only the valid images present in its requested scope. [Cross-slot regressions](../../crates/oplus-edl-core/tests/service.rs) · [Public API regressions](../../crates/oplus-edl-core/tests/public_api.rs)

Desktop preferences round-trip 110% keyboard zoom and logical window size independently. The native guide for round 3 recorded core result commit and worker channel drop. It specified a read-only production GUI fixture for an existing partition `.partial`. The round 4 procedure below adds the native transfer event and the fixture command capture. [Preferences regression](../../crates/oplus-edl-desktop/src/preferences.rs) · [Existing partial procedure](../testing.md#已有分区部分文件) · [USB timing procedure](../testing.md#usb-在途读取计时)

Format, Clippy, locked offline build, 255 workspace tests, and the independent JSON stdout check passed. The live cloud probe remained ignored. Separate runs passed 7 public API, 37 cloud-disabled session, 11 package, 20 cloud-only core, 6 cloud and test-support service, 10 desktop unit, 9 desktop app, 12 parity, and 1 qualification fixture tests. The [verified macOS arm64 ZIP](../../runs/desktop-qualification/implementation-round-3-macos-arm64.zip) uses explicit `usb,serial,cloud` production features and is 23,833,282 bytes. Its SHA-256 is `49a6886e056eabfa60ce0b0afdf1516b3f09e13611194322c3ae0b9a77cb24ca`. The archive verifier and extracted checksum manifest confirmed 542 payload files. Both extracted executables are arm64, target macOS 14.0, and passed strict signature verification. The extracted CLI passed JSONL output checks and exited 74 in approximately 5.02 seconds for help, version, and a parse error with full unread stdout. The standalone macOS 14.0 arm64 qualification executable also built offline. Windows and Ubuntu production archives, clean-host native acceptance on all three platforms, and in-flight USB timing remain pending. [Local build and verification log](../../runs/desktop-qualification/implementation-round-3-repair.log)

## Implementation review round 4 repairs

The author accepted all six fourth-round findings. Backup now uses its reserved LUN and GPT directory paths for file checks, partial file creation, and publication. It rechecks each bound path before it accesses the corresponding output. The Unix cases retarget either caller alias after reservation while a SWM job holds the conflicting input. The identity bytes stay intact and the completed files stay under the reserved paths. A case also replaces the reserved GPT path itself with an alias. Backup rejects it before any GPT read. [Round 4 report](../../reviews/2026-11-18-desktop-implementation/round-4/goldfish_review_report.md) · [Backup/SWM regressions](../../crates/oplus-edl-core/tests/service.rs)

The retained restore helper uses the requested scan LUNs for GPT reads. Its `selected_lun` determines the directory mapping scope. The flat and LUN-directory cases both scan LUNs 0 and 1 and send one program to LUN 0. [Restore helper regression](../../crates/oplus-edl-core/tests/workflows.rs)

The GUI result panel shows each item's partition, LUN, byte range, execution, byte count, and associated error when present. The offline view covers a completed GPT read and a rejected partition. In both frontends, the collision cases preserve the existing final file or `.partial`, record zero transferred bytes for the rejected partition, and keep the completed GPT items. The wire traces confirm that no read overlaps the rejected partition. The cases also compare the retained files and the outcome after a stop. [GUI view regression](../../crates/oplus-edl-desktop/src/views.rs) · [Frontend parity regressions](../../crates/oplus-edl-desktop/tests/frontend_parity.rs)

The two designs and native guide now identify core result commit, worker channel drop, and native USB transfer completion or cancellation as separate events. The existing `.partial` procedure uses a trace from the fixture's Firehose receiver. It uses LUN and sector geometry to compare the read ranges, then checks the item state and transferred bytes in the extracted production GUI. [English design](../designs/oplus-edl-desktop.md) · [Chinese design](../designs/oplus-edl-desktop.zh-CN.md) · [USB procedure](../testing.md#usb-在途读取计时) · [Existing partial procedure](../testing.md#已有分区部分文件)

Format, Clippy, and the locked offline build passed. The workspace ran 260 tests and the independent JSON stdout check. The live cloud probe stayed ignored. Separate runs passed 7 public API, 37 cloud-disabled session, 11 package, 20 cloud-only core, 8 cloud and test-support service, 11 desktop unit, 9 desktop app, 13 parity, and 1 qualification fixture tests. [Round 4 verification log](../../runs/desktop-qualification/implementation-round-4-repair.log)

The macOS arm64 production build used explicit `usb,serial,cloud` features. The ZIP is 23,850,967 bytes with SHA-256 `93e131e1c317bb9f384de47c71ac0876b1486220376e4439e17d8bd6c82fb6a7`. All 542 payload hashes matched after extraction. Both executables target macOS 14.0 and passed strict signature verification. The extracted CLI passed the version and JSONL checks. With full unread stdout, help, version, and parse error each exited 74 in about five seconds. The standalone qualification executable also built for macOS 14.0 arm64. [ZIP for round 5](../../runs/desktop-qualification/implementation-round-4-macos-arm64.zip)

The ZIP reviewed in round 4 keeps its recorded SHA-256 `49a6886e056eabfa60ce0b0afdf1516b3f09e13611194322c3ae0b9a77cb24ca`. Windows and Ubuntu production archives, clean-host native acceptance on all three platforms, and in-flight USB timing remain pending. [ZIP reviewed in round 4](../../runs/desktop-qualification/implementation-round-3-macos-arm64.zip)

## Implementation review round 5 repairs

The author accepted all ten fifth-round findings. Backup rejects duplicate resolved LUN and GPT destinations before the first GPT read. A final path that exists with overwrite disabled, an existing partial path, and an exclusive-open collision each reject only the affected partition with zero bytes. The tests keep the original entries, retain the completed GPTs, and read the next eligible partition. The earlier test still verifies the different outcome of a publication error after the read ACK. [Round 5 report](../../reviews/2026-11-18-desktop-implementation/round-5/goldfish_review_report.md) · [Directory and slot tests](../../crates/oplus-edl-core/tests/service.rs) · [Backup results](../../crates/oplus-edl-core/tests/session.rs)

SWM uses the resolved identity and release locations for reservation and file access. It rechecks both bindings after reservation. Identity creation confirms the resolved location shown by the GUI. The offline slot tests retarget the identity and release aliases while backup holds the other reservation. Another test changes the identity destination between review and confirmation. SWM verification requires regular release files and checks stop before opening and between bounded hash reads. Registration uses host evidence from `HostServices`. The scripted challenge and ticket cases build the real registration body with injected evidence. [SWM bindings](../../crates/oplus-edl-core/src/cloud.rs) · [SWM tests](../../crates/oplus-edl-core/src/swm.rs)

Service startup events use a fixed message for device logs. The queue limits and initialization phase stay visible. Both production adapters exercise the shared event path with scripted startup logs. Core writes use typed preparation and decisions. The CLI owns process exit codes, and its XML write test covers both the missing `--yes` rejection and the approved execution. The GUI renders the actual verification subranges and the recorded byte-difference observation. Diagnostic export retains the subranges and redacts free-text reasons. [Core public API](../../crates/oplus-edl-core/tests/public_api.rs) · [CLI regression](../../crates/oplus-edl-cli/tests/cli.rs) · [GUI result](../../crates/oplus-edl-desktop/src/views.rs) · [Diagnostics](../../crates/oplus-edl-desktop/src/diagnostics.rs)

XML, package, and super-image paths check stop during sparse metadata parsing. The offline cases stop the real XML preparation path and stop between two sparse chunks. The stop audit records the boundaries for device opening, interface claim, response loops, and file operations. A paused open provider proves that Stop is retained while open waits and that the worker drops the returned channel before initialization. The two designs list the actual core source-change tests in `session.rs`, `input.rs`, `writes.rs`, and `package.rs`. CLI output coverage belongs to `tests/cli.rs` and the tests in `src/render.rs` and `src/run.rs`. [Stop audit](../testing.md#停止边界审计) · [Sparse tests](../../crates/oplus-edl-core/src/sparse.rs) · [English design](../designs/oplus-edl-desktop.md) · [Chinese design](../designs/oplus-edl-desktop.zh-CN.md)

Format, Clippy, and the locked offline build passed. The workspace passed 274 tests and the independent JSON stdout check. The live cloud probe stayed ignored. Separate runs passed 7 public API, 42 cloud-disabled session, 11 package, 22 cloud-only core, 11 cloud and test-support service, 14 desktop unit, 9 desktop app, 13 parity, and 1 qualification fixture tests. [Round 5 verification log](../../runs/desktop-qualification/implementation-round-5-repair.log)

The macOS arm64 production build used explicit `usb,serial,cloud` features. The ZIP is 23,875,999 bytes. Its SHA-256 is `fe4ed2bfb895ddaefad7ad508544b0faca30a32dd811ad85e0645f4987b4305b`. All 542 payload hashes matched after extraction. Both executables target macOS 14.0 and passed strict signature verification. The extracted CLI passed version and JSONL checks. With full unread stdout, help, version, and parse error each exited 74 in about five seconds. The separate macOS qualification executable built offline. [ZIP for round 6](../../runs/desktop-qualification/implementation-round-5-macos-arm64.zip)

The ZIP reviewed in round 5 keeps SHA-256 `93e131e1c317bb9f384de47c71ac0876b1486220376e4439e17d8bd6c82fb6a7`. Windows and Ubuntu production archives, all three native acceptance baselines, and the in-flight USB measurements remain pending. [ZIP reviewed in round 5](../../runs/desktop-qualification/implementation-round-4-macos-arm64.zip)

## Implementation review round 6 repairs

The author requested repairs for all ten sixth-round findings. Backup checks the final path after local preparation and before dispatch. The gated regressions occupy a final name before partial open and just before the read. They verify a zero-byte rejected item, the owned partial file, and the next completed partition. The post-ACK collision regression still keeps the retained read facts. Both backup entry points reject duplicate resolved LUN or GPT destinations before the first read. [Round 6 report](../../reviews/2026-11-18-desktop-implementation/round-6/goldfish_review_report.md) · [Dispatch regressions](../../crates/oplus-edl-core/tests/session.rs) · [Public backup regression](../../crates/oplus-edl-core/tests/workflows.rs)

Cloud uses the same resolved paths for reservation, config access, and asset publication. One regression retargets each alias while backup holds the other slot. Another changes the caller's output alias just before publication and verifies the original destination. SWM confirmation keeps the reviewed location when the new location cannot resolve. It reports `InputChanged` and prompts for a new review. SWM checks Stop again at identity access after path revalidation. [Path regressions](../../crates/oplus-edl-core/tests/service.rs) · [Cloud publication](../../crates/oplus-edl-core/src/cloud.rs) · [SWM identity](../../crates/oplus-edl-core/src/swm.rs)

The core records typed execution, write, and verification states. The CLI maps item and range states explicitly to schema v1. An acknowledged write invalidates the partition snapshot even if the session stays reusable. The executor reports whether later items were blocked by cooperative Stop or by an earlier failure. Cloud and SWM response errors retain their stage and safe parser facts. The loopback ticket and SWM tests check that terminal errors contain no synthetic response marker. [CLI schema check](../../crates/oplus-edl-cli/src/render.rs) · [Frontend parity](../../crates/oplus-edl-desktop/tests/frontend_parity.rs)

XML and package preparation check Stop between ordinary inputs. The regressions stop after the first RAW input and verify that later input work and publication do not start. The stop audit and both designs include those boundaries. The Checkpoint A record links the exact prototype ZIP with its retrospective hash and size. [Stop audit](../testing.md#停止边界审计) · [English design](../designs/oplus-edl-desktop.md) · [Chinese design](../designs/oplus-edl-desktop.zh-CN.md) · [Prototype ZIP](../../runs/desktop-qualification/checkpoint-a-prototype-macos-arm64.zip)

Format, Clippy, and the locked offline build passed. The workspace passed 285 tests and the independent JSON stdout check. The live cloud probe stayed ignored. Separate runs passed 7 public API, 46 cloud-disabled session, 11 package, 25 cloud-only core, 12 cloud and test-support service, 14 desktop unit, 9 desktop app, 14 parity, and 1 qualification fixture tests. [Round 6 verification log](../../runs/desktop-qualification/implementation-round-6-repair.log)

The macOS arm64 production ZIP used explicit `usb,serial,cloud` features. It is 23,879,382 bytes with SHA-256 `1fadb59a822db34cffd0864987b304f68490be19cbddd1993596e90a1751ee49`. All 542 payload hashes matched after extraction. Both executables target macOS 14.0 and passed strict signature verification. The extracted CLI passed version and JSONL checks. Help, version, and parse error each exited 74 after about five seconds with full unread stdout. The standalone qualification executable built offline. [ZIP for round 7](../../runs/desktop-qualification/implementation-round-6-macos-arm64.zip)

The ZIP reviewed in round 6 keeps SHA-256 `fe4ed2bfb895ddaefad7ad508544b0faca30a32dd811ad85e0645f4987b4305b`. Windows and Ubuntu production archives, clean-host native acceptance, and in-flight USB timing remain pending. [ZIP reviewed in round 6](../../runs/desktop-qualification/implementation-round-5-macos-arm64.zip)
