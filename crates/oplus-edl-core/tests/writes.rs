use oplus_edl_core::result::{ExecutionState, VerificationState, WriteState};
mod support;
use oplus_edl_core::{
    model::Verify,
    program_xml::{Candidate, Operation},
    restore,
    result::{Report, Status},
    storage::Partition,
    write::Writes,
};
use std::{fs, io::ErrorKind, path::Path};
use support::*;
fn program(root: &Path, name: &str, start: &str, data: &[u8]) -> Candidate {
    let source = root.join(name);
    fs::write(&source, data).unwrap();
    Candidate {
        xml: root.join("raw.xml"),
        xml_node: 1,
        lun: 0,
        slot: 0,
        sector_size: 512,
        start_sector: start.into(),
        label: Some(name.into()),
        operation: Operation::Program {
            source,
            source_bytes: data.len() as u64,
            source_offset: 0,
            transfer_bytes: (data.len() as u64).div_ceil(512) * 512,
            span_bytes: (data.len() as u64).div_ceil(512) * 512,
            sparse: None,
        },
    }
}
fn patch(start: &str) -> Candidate {
    Candidate {
        xml: "patch.xml".into(),
        xml_node: 1,
        lun: 0,
        slot: 0,
        sector_size: 512,
        start_sector: start.into(),
        label: None,
        operation: Operation::Patch {
            byte_offset: 1,
            size: 4,
            value: "1".into(),
        },
    }
}
fn commands(trace: &Trace, name: &str) -> usize {
    trace
        .lock()
        .unwrap()
        .writes
        .iter()
        .filter(|b| String::from_utf8_lossy(b).contains(&format!("<{name} ")))
        .count()
}
fn run(writes: Writes, mode: Verify, input: Vec<Input>) -> (Report, Trace) {
    let (mut dev, trace) = storage_device(input, 512);
    let mut report = Report::new(Some("flash"), Some(mode.as_str()));
    writes.execute(&mut dev, mode, &mut report);
    drop(dev);
    (report, trace)
}
fn restoring(root: &Path) -> Writes {
    let targets: Vec<_> = ["a", "b"]
        .into_iter()
        .enumerate()
        .map(|(n, name)| {
            fs::write(root.join(format!("{name}.bin")), [n as u8 + 1; 511]).unwrap();
            Partition {
                lun: 0,
                name: name.into(),
                start_sector: 64 + n as u64 * 4,
                sectors: 2,
                sector_size: 512,
            }
        })
        .collect();
    Writes::restore(
        restore::directory(root, Some(0), &[], &targets).unwrap(),
        512,
    )
    .unwrap()
}
#[test]
fn restore_padding_and_per_image_verification_order() {
    let dir = tempfile::tempdir().unwrap();
    let writes = restoring(dir.path());
    let mut a = vec![1; 511];
    a.push(0);
    let mut b = vec![2; 511];
    b.push(0);
    let (r, t) = run(
        writes,
        Verify::BestEffort,
        vec![
            bytes(ACK),
            bytes(ACK),
            bytes(ACK),
            bytes(&a),
            bytes(ACK),
            bytes(ACK),
            bytes(ACK),
            bytes(ACK),
            bytes(&b),
            bytes(ACK),
        ],
    );
    assert_eq!(r.status, Status::Success);
    assert!(
        r.items
            .iter()
            .all(|i| i.verification == VerificationState::Matched)
    );
    assert_eq!(r.items[0].transfer_bytes.as_deref(), Some("512"));
    assert_eq!(r.items[0].partition_bytes.as_deref(), Some("1024"));
    let trace = t.lock().unwrap();
    let ops: Vec<_> = trace
        .writes
        .iter()
        .filter_map(|b| {
            let s = String::from_utf8_lossy(b);
            if s.contains("<program ") {
                Some("p")
            } else if s.contains("<read ") {
                Some("r")
            } else {
                None
            }
        })
        .collect();
    assert_eq!(ops, ["p", "r", "p", "r"]);
}
#[test]
fn refused_read_continues_only_in_best_effort() {
    for mode in [Verify::BestEffort, Verify::Required] {
        let dir = tempfile::tempdir().unwrap();
        let writes = restoring(dir.path());
        let mut input = vec![bytes(ACK), bytes(ACK), bytes(NAK)];
        if matches!(mode, Verify::BestEffort) {
            input.extend([bytes(ACK), bytes(ACK), bytes(NAK)]);
        }
        let (r, t) = run(writes, mode, input);
        assert_eq!(r.status, Status::VerificationIncomplete);
        assert_eq!(r.items[0].write, WriteState::Acknowledged);
        assert_eq!(r.items[0].execution, ExecutionState::Done);
        assert_eq!(
            commands(&t, "program"),
            if matches!(mode, Verify::BestEffort) {
                2
            } else {
                1
            }
        );
        assert_no_reset(&t);
    }
}
#[test]
fn lost_ack_nak_and_data_failure_stop_all_modes_and_final_actions() {
    for mode in [Verify::BestEffort, Verify::Required, Verify::None] {
        for rejected in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let ops = vec![
                program(dir.path(), "a", "64", &[7; 512]),
                program(dir.path(), "b", "65", &[8; 512]),
            ];
            let input = vec![
                bytes(ACK),
                if rejected {
                    bytes(NAK)
                } else {
                    Input::Error(ErrorKind::TimedOut)
                },
            ];
            let (r, t) = run(Writes::flash(ops, 512, Some(0), true).unwrap(), mode, input);
            assert_eq!(
                r.status,
                if rejected {
                    Status::ExecutionFailed
                } else {
                    Status::UnknownWrite
                }
            );
            assert_eq!(r.items[0].transferred_bytes.as_deref(), Some("512"));
            assert_eq!(r.items[0].verification, VerificationState::NotApplicable);
            assert!(
                r.items[1..]
                    .iter()
                    .all(|i| i.execution == ExecutionState::NotAttempted)
            );
            assert_eq!(commands(&t, "program"), 1);
            assert_eq!(commands(&t, "read"), 0);
            assert_no_reset(&t);
        }
    }
}
#[test]
fn timeout_truncation_and_unsafe_nak_stop_even_best_effort() {
    for mode in [Verify::BestEffort, Verify::Required] {
        for fault in 0..4 {
            let dir = tempfile::tempdir().unwrap();
            let writes = restoring(dir.path());
            let mut input = vec![bytes(ACK), bytes(ACK)];
            match fault {
                0 => input.push(Input::Error(ErrorKind::TimedOut)),
                1 => input.extend([bytes(ACK), bytes(&[1; 20]), Input::Eof]),
                2 => input.push(bytes(
                    b"<data><response value=\"NAK\" rawmode=\"true\"/></data>",
                )),
                _ => {
                    let mut data = vec![1; 511];
                    data.push(0);
                    input.extend([bytes(ACK), bytes(&data), bytes(NAK)]);
                }
            }
            let (r, t) = run(writes, mode, input);
            assert_eq!(r.items[0].write, WriteState::Acknowledged);
            assert_eq!(r.status, Status::VerificationIncomplete);
            assert_eq!(commands(&t, "program"), 1);
            assert_no_reset(&t);
        }
    }
}
#[test]
fn mismatch_stops_restore_and_flash_final_actions() {
    let dir = tempfile::tempdir().unwrap();
    let (r, t) = run(
        restoring(dir.path()),
        Verify::BestEffort,
        vec![
            bytes(ACK),
            bytes(ACK),
            bytes(ACK),
            bytes(&[9; 512]),
            bytes(ACK),
        ],
    );
    assert_eq!(r.status, Status::ExecutionFailed);
    assert_eq!(r.items[0].verification, VerificationState::Mismatch);
    assert_eq!(commands(&t, "program"), 1);
    let w = Writes::flash(
        vec![program(dir.path(), "raw", "64", &[1; 512])],
        512,
        Some(0),
        true,
    )
    .unwrap();
    let (r, t) = run(
        w,
        Verify::BestEffort,
        vec![
            bytes(ACK),
            bytes(ACK),
            bytes(ACK),
            bytes(&[9; 512]),
            bytes(ACK),
        ],
    );
    assert_eq!(r.status, Status::ExecutionFailed);
    assert!(
        r.items[1..]
            .iter()
            .all(|i| i.execution == ExecutionState::NotAttempted)
    );
    assert_no_reset(&t);
}
#[test]
fn none_has_no_reads_and_final_actions_are_explicit_once() {
    let dir = tempfile::tempdir().unwrap();
    let w = Writes::flash(
        vec![program(dir.path(), "raw", "64", &[1; 512]), patch("64")],
        512,
        Some(0),
        true,
    )
    .unwrap();
    let (r, t) = run(w, Verify::None, (0..5).map(|_| bytes(ACK)).collect());
    assert_eq!(r.status, Status::Success);
    assert_eq!(commands(&t, "read"), 0);
    assert_eq!(commands(&t, "patch"), 1);
    assert_eq!(commands(&t, "setbootablestoragedrive"), 1);
    assert_eq!(commands(&t, "power"), 1);
    assert_eq!(r.items[0].verification, VerificationState::NotRequested);
    assert_eq!(r.items[1].verification, VerificationState::NotRequested);
    assert_eq!(r.items[2].verification, VerificationState::NotApplicable);
}
#[test]
fn required_rejects_unknown_expectations_before_any_mutation() {
    let dir = tempfile::tempdir().unwrap();
    for ops in [
        vec![program(dir.path(), "a", "64", &[1; 512]), patch("64")],
        vec![program(dir.path(), "a", "NUM_DISK_SECTORS-1.", &[1; 512])],
    ] {
        let (r, t) = run(
            Writes::flash(ops, 512, Some(0), true).unwrap(),
            Verify::Required,
            vec![],
        );
        assert_eq!(r.status, Status::Unsupported);
        assert!(t.lock().unwrap().writes.is_empty());
        assert!(
            r.items
                .iter()
                .all(|i| i.execution == ExecutionState::NotAttempted)
        );
    }
}
#[test]
fn partial_overlap_verifies_last_writer_and_superseded_has_no_read() {
    let dir = tempfile::tempdir().unwrap();
    let ops = vec![
        program(dir.path(), "old", "64", &[1; 1024]),
        program(dir.path(), "new", "65", &[2; 512]),
        program(dir.path(), "last", "65", &[3; 512]),
    ];
    let mut script: Vec<_> = (0..6).map(|_| bytes(ACK)).collect();
    script.extend([
        bytes(ACK),
        bytes(&[1; 512]),
        bytes(ACK),
        bytes(ACK),
        bytes(&[3; 512]),
        bytes(ACK),
    ]);
    let (r, t) = run(
        Writes::flash(ops, 512, None, false).unwrap(),
        Verify::Required,
        script,
    );
    assert_eq!(r.status, Status::Success);
    assert_eq!(commands(&t, "read"), 2);
    assert_eq!(r.items[0].ranges[0].length_bytes.as_deref(), Some("512"));
    assert_eq!(r.items[1].reason.as_deref(), Some("superseded"));
    assert_eq!(r.items[1].verification, VerificationState::NotApplicable);
}
#[test]
fn patch_masks_only_its_bytes_and_expression_masks_whole_lun() {
    for expression in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let ops = vec![
            program(dir.path(), "a", "64", &[1; 512]),
            patch(if expression {
                "NUM_DISK_SECTORS-1."
            } else {
                "64"
            }),
        ];
        let mut script = vec![bytes(ACK), bytes(ACK), bytes(ACK)];
        if !expression {
            let mut data = vec![1; 512];
            data[1..5].fill(9);
            for _ in 0..2 {
                script.extend([bytes(ACK), bytes(&data), bytes(ACK)]);
            }
        }
        script.extend([bytes(ACK), bytes(ACK)]);
        let (r, t) = run(
            Writes::flash(ops, 512, Some(0), true).unwrap(),
            Verify::BestEffort,
            script,
        );
        assert_eq!(r.status, Status::VerificationIncomplete);
        assert_eq!(r.items[0].verification, VerificationState::Incomplete);
        assert_eq!(commands(&t, "read"), if expression { 0 } else { 2 });
        assert_eq!(commands(&t, "power"), 1);
    }
}
#[test]
fn final_action_failure_stops_reboot_and_preserves_verified_write() {
    for unknown in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let w = Writes::flash(
            vec![program(dir.path(), "a", "64", &[1; 512])],
            512,
            Some(0),
            true,
        )
        .unwrap();
        let (r, t) = run(
            w,
            Verify::Required,
            vec![
                bytes(ACK),
                bytes(ACK),
                bytes(ACK),
                bytes(&[1; 512]),
                bytes(ACK),
                if unknown { Input::Eof } else { bytes(NAK) },
            ],
        );
        assert_eq!(
            r.status,
            if unknown {
                Status::UnknownWrite
            } else {
                Status::ExecutionFailed
            }
        );
        assert_eq!(r.items[0].verification, VerificationState::Matched);
        assert_eq!(r.items[2].execution, ExecutionState::NotAttempted);
        assert_eq!(commands(&t, "power"), 0);
    }
}
#[test]
fn flash_safe_nak_allows_final_actions_but_required_blocks_them() {
    for mode in [Verify::BestEffort, Verify::Required] {
        let dir = tempfile::tempdir().unwrap();
        let w = Writes::flash(
            vec![program(dir.path(), "a", "64", &[1; 512])],
            512,
            Some(0),
            true,
        )
        .unwrap();
        let mut input = vec![bytes(ACK), bytes(ACK), bytes(NAK)];
        if matches!(mode, Verify::BestEffort) {
            input.extend([bytes(ACK), bytes(ACK)]);
        }
        let (r, t) = run(w, mode, input);
        assert_eq!(r.status, Status::VerificationIncomplete);
        assert_eq!(
            commands(&t, "power"),
            usize::from(matches!(mode, Verify::BestEffort))
        );
    }
}
#[test]
fn sparse_raw_fill_and_dont_care_preserve_prior_verification() {
    use oplus_edl_core::sparse::{Chunk, Data, Image};
    let dir = tempfile::tempdir().unwrap();
    let old = program(dir.path(), "old", "64", &[1; 1536]);
    let mut sparse = program(dir.path(), "sparse", "64", &[2; 512]);
    if let Operation::Program {
        sparse,
        transfer_bytes,
        span_bytes,
        ..
    } = &mut sparse.operation
    {
        *transfer_bytes = 1024;
        *span_bytes = 1536;
        *sparse = Some(Image {
            expanded_bytes: 1536,
            transfer_bytes: 1024,
            chunks: vec![
                Chunk {
                    offset: 0,
                    bytes: 512,
                    data: Data::Raw { file_offset: 0 },
                },
                Chunk {
                    offset: 512,
                    bytes: 512,
                    data: Data::DontCare,
                },
                Chunk {
                    offset: 1024,
                    bytes: 512,
                    data: Data::Fill([3, 4, 5, 6]),
                },
            ],
        });
    }
    let fill: Vec<_> = (0..512).map(|i| [3, 4, 5, 6][i % 4]).collect();
    let mut input: Vec<_> = (0..6).map(|_| bytes(ACK)).collect();
    for data in [vec![1; 512], vec![2; 512], fill.clone()] {
        input.extend([bytes(ACK), bytes(&data), bytes(ACK)]);
    }
    let (r, t) = run(
        Writes::flash(vec![old, sparse], 512, None, false).unwrap(),
        Verify::Required,
        input,
    );
    assert_eq!(r.status, Status::Success);
    assert_eq!(commands(&t, "program"), 3);
    assert_eq!(r.items[0].ranges[0].start_byte.as_deref(), Some("33280"));
    let raw: Vec<_> = t
        .lock()
        .unwrap()
        .writes
        .iter()
        .filter(|b| !b.starts_with(b"<?xml"))
        .cloned()
        .collect();
    assert!(raw.iter().any(|b| b == &fill));
    assert!(!raw.iter().any(|b| b == &vec![0; 512]));
}
#[test]
fn mutation_partial_transport_count_is_not_input_prefetch_count() {
    let dir = tempfile::tempdir().unwrap();
    let w = Writes::flash(
        vec![program(dir.path(), "a", "64", &[1; 512])],
        512,
        Some(0),
        true,
    )
    .unwrap();
    let (mut dev, t) = storage_device(vec![bytes(ACK)], 512);
    dev.rw.output.extend([
        Output::Accept(usize::MAX),
        Output::Accept(100),
        Output::Error(ErrorKind::BrokenPipe),
    ]);
    let mut r = Report::new(Some("flash"), Some("none"));
    w.execute(&mut dev, Verify::None, &mut r);
    drop(dev);
    assert_eq!(r.status, Status::UnknownWrite);
    assert_eq!(r.items[0].transferred_bytes.as_deref(), Some("100"));
    assert_no_reset(&t);
}
#[test]
fn truncated_source_before_program_is_rejected_without_mutation() {
    let dir = tempfile::tempdir().unwrap();
    let w = Writes::flash(
        vec![program(dir.path(), "a", "64", &[1; 512])],
        512,
        Some(0),
        true,
    )
    .unwrap();
    fs::write(dir.path().join("a"), [1; 20]).unwrap();
    let (r, t) = run(w, Verify::BestEffort, vec![]);
    assert_eq!(r.status, Status::InputChanged);
    assert_eq!(r.items[0].write, WriteState::NotAttempted);
    assert_eq!(r.items[0].transferred_bytes.as_deref(), Some("0"));
    assert!(t.lock().unwrap().writes.is_empty());
    assert_no_reset(&t);
}
#[test]
fn patch_failure_stops_verification_and_final_actions() {
    for unknown in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let w = Writes::flash(
            vec![
                program(dir.path(), "a", "64", &[1; 512]),
                patch("64"),
                patch("65"),
            ],
            512,
            Some(0),
            true,
        )
        .unwrap();
        let (r, t) = run(
            w,
            Verify::BestEffort,
            vec![
                bytes(ACK),
                bytes(ACK),
                if unknown { Input::Eof } else { bytes(NAK) },
            ],
        );
        assert_eq!(
            r.status,
            if unknown {
                Status::UnknownWrite
            } else {
                Status::ExecutionFailed
            }
        );
        assert_eq!(r.items[0].write, WriteState::Acknowledged);
        assert_eq!(r.items[0].verification, VerificationState::Incomplete);
        assert!(!r.items[0].ranges.is_empty());
        assert_eq!(commands(&t, "patch"), 1);
        assert_eq!(commands(&t, "read"), 0);
        assert_eq!(commands(&t, "power"), 0);
        assert_eq!(commands(&t, "setbootablestoragedrive"), 0);
    }
}
#[test]
fn unknown_program_address_is_incomplete_and_skip_is_preserved() {
    let dir = tempfile::tempdir().unwrap();
    let mut skip = patch("64");
    skip.operation = Operation::Skip {
        reason: "local file patch skipped: boot.bin".into(),
    };
    let w = Writes::flash(
        vec![
            program(dir.path(), "a", "NUM_DISK_SECTORS-1.", &[1; 512]),
            skip,
        ],
        512,
        Some(0),
        true,
    )
    .unwrap();
    let (r, t) = run(w, Verify::BestEffort, (0..4).map(|_| bytes(ACK)).collect());
    assert_eq!(r.status, Status::VerificationIncomplete);
    assert_eq!(commands(&t, "read"), 0);
    assert_eq!(r.items[1].execution, ExecutionState::Skipped);
    assert_eq!(r.items[1].write, WriteState::NotApplicable);
    let json = serde_json::to_value(&r).unwrap();
    assert!(json["items"][0]["ranges"][0]["start_byte"].is_null());
    assert_eq!(r.items[0].write, WriteState::Acknowledged);
    assert_eq!(r.items[0].verification, VerificationState::Incomplete);
}
#[test]
fn directory_mapping_failure_after_gpt_reads_never_programs() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("missing.bin"), [1; 512]).unwrap();
    let disk = disk(512, &["boot"]);
    let (mut dev, t) = storage_device(gpt_script(&disk, 512), 512);
    let mut r = Report::new(Some("restore"), Some("best-effort"));
    assert!(
        oplus_edl_core::write::prepare_restore(
            &mut dev,
            &[0],
            Some(dir.path()),
            None,
            &[],
            Some(0),
            &mut r
        )
        .is_err()
    );
    drop(dev);
    assert_eq!(r.items.len(), 2);
    assert_eq!(commands(&t, "program"), 0);
    assert_no_reset(&t);
}
#[test]
fn stopped_later_verification_does_not_reclassify_superseded_item() {
    let dir = tempfile::tempdir().unwrap();
    let w = Writes::flash(
        vec![
            program(dir.path(), "a", "64", &[1; 512]),
            program(dir.path(), "b", "64", &[2; 512]),
        ],
        512,
        None,
        false,
    )
    .unwrap();
    let (r, _) = run(
        w,
        Verify::Required,
        vec![bytes(ACK), bytes(ACK), bytes(ACK), bytes(ACK), bytes(NAK)],
    );
    assert_eq!(r.status, Status::VerificationIncomplete);
    assert_eq!(r.items[0].verification, VerificationState::NotApplicable);
    assert_eq!(r.items[0].reason.as_deref(), Some("superseded"));
}

#[test]
fn stop_before_unvisited_overlap_reports_only_final_effective_ranges() {
    for full in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let ops = vec![
            program(dir.path(), "first", "70", &[1; 512]),
            program(dir.path(), "old", "64", &[2; 1024]),
            program(
                dir.path(),
                "new",
                if full { "64" } else { "65" },
                &vec![3; if full { 1024 } else { 512 }],
            ),
        ];
        let mut input: Vec<_> = (0..6).map(|_| bytes(ACK)).collect();
        input.push(bytes(NAK));
        let (r, t) = run(
            Writes::flash(ops, 512, Some(0), true).unwrap(),
            Verify::Required,
            input,
        );
        assert_eq!(r.status, Status::VerificationIncomplete);
        let old = &r.items[1];
        assert_eq!(old.write, WriteState::Acknowledged);
        assert_eq!(old.execution, ExecutionState::Done);
        if full {
            assert_eq!(old.verification, VerificationState::NotApplicable);
            assert_eq!(old.reason.as_deref(), Some("superseded"));
            assert!(old.ranges.is_empty());
            assert!(old.errors.is_empty());
        } else {
            assert_eq!(old.verification, VerificationState::Incomplete);
            assert_eq!(old.ranges.len(), 1);
            assert_eq!(old.ranges[0].start_byte.as_deref(), Some("32768"));
            assert_eq!(old.ranges[0].length_bytes.as_deref(), Some("512"));
        }
        assert_eq!(commands(&t, "read"), 1);
        assert_no_reset(&t);
    }
}

#[test]
fn stopped_mutations_do_not_supersede_with_unattempted_or_unconfirmed_writes() {
    for mode in [Verify::BestEffort, Verify::Required, Verify::None] {
        let dir = tempfile::tempdir().unwrap();
        let ops = vec![
            program(dir.path(), "old", "64", &[1; 1024]),
            program(dir.path(), "failed", "65", &[2; 512]),
            program(dir.path(), "unattempted", "64", &[3; 1024]),
        ];
        let (r, t) = run(
            Writes::flash(ops, 512, Some(0), true).unwrap(),
            mode,
            vec![bytes(ACK), bytes(ACK), bytes(ACK), Input::Eof],
        );
        assert_eq!(r.status, Status::UnknownWrite);
        assert_ne!(r.items[0].reason.as_deref(), Some("superseded"));
        assert_eq!(
            r.items[0]
                .ranges
                .iter()
                .map(|r| r.length_bytes.as_ref().unwrap().parse::<u64>().unwrap())
                .sum::<u64>(),
            1024
        );
        assert_eq!(
            r.items[0].verification,
            if matches!(mode, Verify::None) {
                VerificationState::NotRequested
            } else {
                VerificationState::Incomplete
            }
        );
        assert_eq!(r.items[1].verification, VerificationState::NotApplicable);
        assert_eq!(r.items[2].execution, ExecutionState::NotAttempted);
        assert_eq!(commands(&t, "read"), 0);
        assert_no_reset(&t);
    }
}

#[test]
fn matching_read_data_without_completion_ack_is_incomplete_and_stops() {
    for mode in [Verify::BestEffort, Verify::Required] {
        for fault in [0, 1, 2] {
            let dir = tempfile::tempdir().unwrap();
            let w = restoring(dir.path());
            let mut data = vec![1; 511];
            data.push(0);
            let (r, t) = run(
                w,
                mode,
                vec![
                    bytes(ACK),
                    bytes(ACK),
                    bytes(ACK),
                    bytes(&data),
                    match fault {
                        0 => bytes(NAK),
                        1 => Input::Eof,
                        _ => Input::Error(ErrorKind::TimedOut),
                    },
                ],
            );
            assert_eq!(r.status, Status::VerificationIncomplete);
            assert_eq!(r.items[0].write, WriteState::Acknowledged);
            assert_eq!(r.items[0].execution, ExecutionState::Done);
            assert_eq!(r.items[0].verification, VerificationState::Incomplete);
            assert_eq!(
                r.items[0].errors[0].category,
                Status::VerificationIncomplete
            );
            assert_eq!(r.items[1].execution, ExecutionState::NotAttempted);
            assert_eq!(commands(&t, "program"), 1);
            assert_no_reset(&t);
            let w = Writes::flash(
                vec![program(dir.path(), "raw", "64", &data)],
                512,
                Some(0),
                true,
            )
            .unwrap();
            let (r, t) = run(
                w,
                mode,
                vec![bytes(ACK), bytes(ACK), bytes(ACK), bytes(&data), Input::Eof],
            );
            assert_eq!(r.status, Status::VerificationIncomplete);
            assert!(
                r.items[1..]
                    .iter()
                    .all(|i| i.execution == ExecutionState::NotAttempted)
            );
            assert_no_reset(&t);
        }
    }
}

#[test]
fn reboot_nak_and_missing_ack_keep_prior_results_and_never_retry() {
    for mode in [Verify::BestEffort, Verify::Required, Verify::None] {
        for rejected in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let w = Writes::flash(
                vec![program(dir.path(), "a", "64", &[1; 512])],
                512,
                Some(0),
                true,
            )
            .unwrap();
            let mut input = vec![bytes(ACK), bytes(ACK)];
            if !matches!(mode, Verify::None) {
                input.extend([bytes(ACK), bytes(&[1; 512]), bytes(ACK)]);
            }
            input.extend([
                bytes(ACK),
                if rejected {
                    bytes(NAK)
                } else {
                    Input::Error(ErrorKind::TimedOut)
                },
            ]);
            let (r, t) = run(w, mode, input);
            assert_eq!(
                r.status,
                if rejected {
                    Status::ExecutionFailed
                } else {
                    Status::UnknownWrite
                }
            );
            assert_eq!(r.items[0].write, WriteState::Acknowledged);
            assert_eq!(
                r.items[0].verification,
                if matches!(mode, Verify::None) {
                    VerificationState::NotRequested
                } else {
                    VerificationState::Matched
                }
            );
            assert_eq!(r.items[1].write, WriteState::Acknowledged);
            assert_eq!(r.items[2].execution, ExecutionState::Failed);
            assert_eq!(
                r.items[2].write,
                if rejected {
                    WriteState::Failed
                } else {
                    WriteState::Unknown
                }
            );
            assert_eq!(r.items[2].verification, VerificationState::NotApplicable);
            assert_eq!(commands(&t, "power"), 1);
            assert_eq!(commands(&t, "setbootablestoragedrive"), 1);
        }
    }
}

#[test]
fn mutation_not_sent_stops_entire_execution_without_transport_writes() {
    for mode in [Verify::BestEffort, Verify::Required, Verify::None] {
        let dir = tempfile::tempdir().unwrap();
        let mut ops = vec![
            program(dir.path(), "a", "64", &[1; 512]),
            program(dir.path(), "b", "65", &[2; 512]),
        ];
        if !matches!(mode, Verify::Required) {
            ops.push(patch("64"));
        }
        let w = Writes::flash(ops, 512, Some(0), true).unwrap();
        let (mut dev, t) = storage_device(vec![], 512);
        dev.fh_cfg.send_buffer_size = 0;
        let mut r = Report::new(Some("flash"), Some(mode.as_str()));
        w.execute(&mut dev, mode, &mut r);
        drop(dev);
        assert_eq!(r.status, Status::ExecutionFailed);
        assert_eq!(r.items[0].execution, ExecutionState::Failed);
        assert_eq!(r.items[0].write, WriteState::NotAttempted);
        assert_eq!(r.items[0].verification, VerificationState::NotApplicable);
        assert_eq!(r.items[0].transferred_bytes.as_deref(), Some("0"));
        assert!(
            r.items[1..]
                .iter()
                .all(|i| i.execution == ExecutionState::NotAttempted && i.errors.is_empty())
        );
        assert!(t.lock().unwrap().writes.is_empty());
    }
}

#[test]
fn patches_mask_only_the_target_lun() {
    for expression in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let a = program(dir.path(), "a", "64", &[1; 512]);
        let mut b = program(dir.path(), "b", "64", &[2; 512]);
        b.lun = 1;
        let mut p = patch(if expression {
            "NUM_DISK_SECTORS-1."
        } else {
            "64"
        });
        p.lun = 1;
        let mut input: Vec<_> = (0..5).map(|_| bytes(ACK)).collect();
        input.extend([bytes(ACK), bytes(&[1; 512]), bytes(ACK)]);
        if !expression {
            let mut data = vec![2; 512];
            data[1..5].fill(7);
            for _ in 0..2 {
                input.extend([bytes(ACK), bytes(&data), bytes(ACK)]);
            }
        }
        input.extend([bytes(ACK), bytes(ACK)]);
        let (r, t) = run(
            Writes::flash(vec![a, b, p], 512, Some(0), true).unwrap(),
            Verify::BestEffort,
            input,
        );
        assert_eq!(r.status, Status::VerificationIncomplete);
        assert_eq!(r.items[0].verification, VerificationState::Matched);
        assert_eq!(r.items[0].ranges.len(), 1);
        assert_eq!(r.items[0].ranges[0].length_bytes.as_deref(), Some("512"));
        assert_eq!(r.items[1].verification, VerificationState::Incomplete);
        assert_eq!(r.items[2].verification, VerificationState::Incomplete);
        assert_eq!(commands(&t, "power"), 1);
    }
}

#[test]
fn mutation_stop_before_verification_still_marks_confirmed_overwrites_superseded() {
    for mode in [Verify::BestEffort, Verify::Required, Verify::None] {
        let dir = tempfile::tempdir().unwrap();
        let ops = vec![
            program(dir.path(), "old", "64", &[1; 512]),
            program(dir.path(), "new", "64", &[2; 512]),
            program(dir.path(), "failed", "70", &[3; 512]),
        ];
        let (r, t) = run(
            Writes::flash(ops, 512, Some(0), true).unwrap(),
            mode,
            vec![bytes(ACK), bytes(ACK), bytes(ACK), bytes(ACK), bytes(NAK)],
        );
        assert_eq!(r.status, Status::ExecutionFailed);
        assert_eq!(r.items[0].verification, VerificationState::NotApplicable);
        assert_eq!(r.items[0].reason.as_deref(), Some("superseded"));
        assert!(r.items[0].ranges.is_empty());
        assert!(r.items[0].errors.is_empty());
        assert_eq!(
            r.items[1].verification,
            if matches!(mode, Verify::None) {
                VerificationState::NotRequested
            } else {
                VerificationState::Incomplete
            }
        );
        assert_eq!(r.items[2].write, WriteState::Failed);
        assert_eq!(commands(&t, "read"), 0);
        assert_no_reset(&t);
    }
}

#[test]
fn explicit_wrong_read_ack_mode_stops_following_mutations() {
    for mode in [Verify::BestEffort, Verify::Required] {
        for initial in [true, false] {
            for restore in [true, false] {
                let dir = tempfile::tempdir().unwrap();
                let w = if restore {
                    restoring(dir.path())
                } else {
                    Writes::flash(
                        vec![program(dir.path(), "a", "64", &[1; 512])],
                        512,
                        Some(0),
                        true,
                    )
                    .unwrap()
                };
                let mut input = vec![bytes(ACK), bytes(ACK)];
                if initial {
                    input.push(bytes(
                        b"<data><response value=\"ACK\" rawmode=\"false\"/></data>",
                    ));
                } else {
                    let mut data = vec![1; 512];
                    if restore {
                        data[511] = 0;
                    }
                    input.extend([
                        bytes(b"<data><response value=\"ACK\" rawmode=\"true\"/></data>"),
                        bytes(&data),
                        bytes(b"<data><response value=\"ACK\" rawmode=\"true\"/></data>"),
                    ]);
                }
                let (r, t) = run(w, mode, input);
                assert_eq!(r.status, Status::VerificationIncomplete);
                assert_eq!(r.items[0].verification, VerificationState::Incomplete);
                assert_eq!(commands(&t, "program"), 1);
                assert_no_reset(&t);
                assert!(
                    r.items[1..]
                        .iter()
                        .all(|i| i.execution == ExecutionState::NotAttempted)
                );
            }
        }
    }
}

#[test]
fn wrong_xml_mutation_ack_mode_stops_patch_boot_and_reset_sequence() {
    for stage in 0..3 {
        let dir = tempfile::tempdir().unwrap();
        let w = Writes::flash(
            vec![program(dir.path(), "a", "64", &[1; 512]), patch("64")],
            512,
            Some(0),
            true,
        )
        .unwrap();
        let mut input = vec![bytes(ACK), bytes(ACK)];
        input.extend((0..stage).map(|_| bytes(ACK)));
        input.push(bytes(
            b"<data><response value=\"ACK\" rawmode=\"true\"/></data>",
        ));
        let (r, t) = run(w, Verify::None, input);
        assert_eq!(r.status, Status::UnknownWrite);
        assert_eq!(r.items[stage + 1].write, WriteState::Unknown);
        assert!(
            r.items[stage + 2..]
                .iter()
                .all(|i| i.execution == ExecutionState::NotAttempted)
        );
        assert_eq!(commands(&t, "patch"), 1);
        assert_eq!(
            commands(&t, "setbootablestoragedrive"),
            usize::from(stage >= 1)
        );
        assert_eq!(commands(&t, "power"), usize::from(stage >= 2));
    }
}

#[test]
fn usb_error_with_unavailable_completion_length_reports_unknown_byte_count() {
    use qdl::types::QdlBackend;
    for completed_blocks in [0, 1] {
        let dir = tempfile::tempdir().unwrap();
        let w = Writes::flash(
            vec![program(dir.path(), "a", "64", &[1; 8192])],
            512,
            Some(0),
            true,
        )
        .unwrap();
        let (mut dev, t) = storage_device([bytes(ACK)], 512);
        dev.fh_cfg.backend = QdlBackend::Usb;
        dev.rw.output.push_back(Output::Accept(usize::MAX)); // XML
        for _ in 0..completed_blocks {
            dev.rw.output.push_back(Output::Accept(4096));
        }
        // This is the adapter's public result for a successful short USB
        // completion; actual_len is no longer available to CountChannel.
        dev.rw.output.push_back(Output::Error(ErrorKind::WriteZero));
        let mut report = Report::new(Some("flash"), Some("none"));
        w.execute(&mut dev, Verify::None, &mut report);
        assert_eq!(report.status, Status::UnknownWrite);
        assert_eq!(report.items[0].transferred_bytes, None);
        assert!(
            report.items[1..]
                .iter()
                .all(|i| i.execution == ExecutionState::NotAttempted)
        );
        assert_eq!(t.lock().unwrap().writes.len(), completed_blocks + 2);
        assert_no_reset(&t);
    }
}

#[test]
fn preview_write_verification_uses_the_selected_wire_mode() {
    let dir = tempfile::tempdir().unwrap();
    let mut writes = Writes::flash(
        vec![program(dir.path(), "a", "64", &[1; 512])],
        512,
        None,
        false,
    )
    .unwrap();
    writes.set_wire_mode(oplus_edl_core::firehose::WireMode::PreviewBackupGpt);
    let (report, trace) = run(
        writes,
        Verify::Required,
        vec![
            bytes(ACK),
            bytes(ACK),
            bytes(ACK),
            bytes(&[1; 512]),
            bytes(ACK),
        ],
    );
    assert_eq!(report.status, Status::Success);
    let trace = trace.lock().unwrap();
    let read = trace
        .writes
        .iter()
        .filter_map(|w| xmltree::Element::parse(w.as_slice()).ok())
        .find_map(|xml| xml.get_child("read").cloned())
        .unwrap();
    assert_eq!(read.attributes["special_rw_mode"], "oplus_gptbackup");
    assert_eq!(read.attributes["filename"], "gpt_backup0.bin");
    assert_eq!(read.attributes["start_sector"], "64");
}

#[test]
fn count_channel_forwards_refill_and_expiry_at_both_program_responses() {
    for terminal in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let writes = Writes::flash(
            vec![program(temp.path(), "boot.bin", "64", &[3; 512])],
            512,
            None,
            true,
        )
        .unwrap();
        let clock = ManualClock::default();
        let mut input = Vec::new();
        if terminal {
            input.push(response(RAW_ACK));
        }
        input.extend([
            expect_response(Input::Error(ErrorKind::Interrupted)),
            response(LOG),
        ]);
        input.push(on_refill(Access::Response, bytes(ACK), {
            let clock = clock.clone();
            move || clock.set(10)
        }));
        let (mut channel, trace) = deadline_channel(input, &clock);
        let mut report = Report::new(Some("flash"), Some("none"));
        writes.execute(&mut channel, Verify::None, &mut report);
        let item = &report.items[0];
        assert_eq!(
            item.errors[0].cause,
            Some(oplus_edl_core::session::ResponseWaitExpired)
        );
        assert_eq!(report.status, Status::UnknownWrite);
        assert_eq!(
            item.transferred_bytes.as_deref(),
            Some(if terminal { "512" } else { "0" })
        );
        assert_eq!(
            wire_commands(&trace)
                .iter()
                .map(|cmd| cmd.name.as_str())
                .collect::<Vec<_>>(),
            ["program"]
        );
    }
}

#[test]
fn verification_timeout_uses_the_terminal_wrapper_without_losing_the_program_count() {
    let temp = tempfile::tempdir().unwrap();
    let writes = Writes::flash(
        vec![program(temp.path(), "boot.bin", "64", &[3; 512])],
        512,
        None,
        true,
    )
    .unwrap();
    let clock = ManualClock::default();
    let input = vec![
        response(RAW_ACK),
        response(ACK),
        response(RAW_ACK),
        bytes(&[8; 512]),
        on_refill(Access::Response, bytes(ACK), {
            let clock = clock.clone();
            move || clock.set(11)
        }),
    ];
    let (mut channel, trace) = deadline_channel(input, &clock);
    let mut report = Report::new(Some("flash"), Some("required"));
    writes.execute(&mut channel, Verify::Required, &mut report);
    let item = &report.items[0];
    assert_eq!(report.status, Status::VerificationIncomplete);
    assert_eq!(item.write, WriteState::Acknowledged);
    assert_eq!(item.transferred_bytes.as_deref(), Some("512"));
    assert_eq!(
        item.ranges[0].reason.as_deref(),
        Some("byte_difference_observed")
    );
    assert_eq!(
        item.errors[0].cause,
        Some(oplus_edl_core::session::ResponseWaitExpired)
    );
    assert_eq!(
        wire_commands(&trace)
            .iter()
            .map(|cmd| cmd.name.as_str())
            .collect::<Vec<_>>(),
        ["program", "read"]
    );
}
