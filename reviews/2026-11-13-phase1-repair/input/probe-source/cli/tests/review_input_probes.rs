// Reviewer-only probes. Not part of the immutable repair snapshot.
#[path = "../src/input.rs"]
mod input;
#[path = "../src/programfile.rs"]
mod programfile;
#[path = "../../qdl/tests/support/mod.rs"]
mod support;

use input::ProgramInput;
use qdl::{firehose_program_storage, types::{MutationConfirmation, MutationFailure}};
use std::{fs::{self, File, OpenOptions}, io::{Read, Write, ErrorKind}, path::PathBuf, sync::atomic::{AtomicU64, Ordering}};
use support::*;

struct Fixture(PathBuf);
impl Fixture {
    fn new(data: &[u8]) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!("review-input-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("image.bin"), data).unwrap();
        Self(dir)
    }
    fn path(&self) -> PathBuf { self.0.join("image.bin") }
    fn file(&self) -> File { File::open(self.path()).unwrap() }
}
impl Drop for Fixture { fn drop(&mut self) { fs::remove_dir_all(&self.0).unwrap(); } }

#[test]
fn range_matrix_matches_explicit_padding_contract() {
    let mut cases = 0;
    for sector in [1usize, 2, 3, 4, 7] {
        for len in 0usize..=20 {
            let source: Vec<u8> = (0..len as u8).map(|b| b + 1).collect();
            let fixture = Fixture::new(&source);
            for offset in 0..=len+2 {
                for requested in std::iter::once(None).chain((0..=7).map(Some)) {
                    let mut actual = ProgramInput::from_file(fixture.file(), sector, offset as u64, requested);
                    let expected = len.checked_sub(offset).and_then(|available| {
                        let sectors = requested.unwrap_or(available.div_ceil(sector));
                        let target = sectors * sector;
                        let taken = target.min(available);
                        (target - taken < sector).then_some((sectors, target, taken))
                    });
                    cases += 1;
                    match expected {
                        None => assert!(actual.is_err(), "sector={sector} len={len} offset={offset} requested={requested:?}"),
                        Some((sectors, target, taken)) => {
                            let reader = actual.as_mut().unwrap();
                            assert_eq!(reader.num_sectors(), sectors);
                            assert_eq!(reader.read(&mut []).unwrap(), 0);
                            let mut out = Vec::new();
                            let mut chunk = [0x5a; 3];
                            loop {
                                let n = reader.read(&mut chunk).unwrap();
                                if n == 0 { break; }
                                out.extend_from_slice(&chunk[..n]);
                            }
                            let mut expected_bytes = source[offset..offset+taken].to_vec();
                            expected_bytes.resize(target, 0);
                            assert_eq!(out, expected_bytes, "sector={sector} len={len} offset={offset} requested={requested:?}");
                            assert_eq!(reader.read(&mut chunk).unwrap(), 0);
                        }
                    }
                }
            }
        }
    }
    println!("checked {cases} source-range cases");
}

#[test]
fn growth_does_not_change_prepared_file_range_or_padding() {
    let fixture = Fixture::new(b"abcde");
    let mut reader = ProgramInput::from_file(fixture.file(), 4, 0, None).unwrap();
    OpenOptions::new().append(true).open(fixture.path()).unwrap().write_all(b"XYZ").unwrap();
    let mut out = Vec::new();
    reader.read_to_end(&mut out).unwrap();
    assert_eq!(out, b"abcde\0\0\0");
}

#[test]
fn selected_range_detects_only_loss_of_required_source_bytes() {
    for truncated_len in [7, 8] {
        let fixture = Fixture::new(b"abcdefghijkl");
        let mut reader = ProgramInput::from_file(fixture.file(), 4, 0, Some(2)).unwrap();
        OpenOptions::new().write(true).open(fixture.path()).unwrap().set_len(truncated_len).unwrap();
        let script = if truncated_len == 8 { vec![bytes(ACK), bytes(ACK)] } else { vec![bytes(ACK)] };
        let (mut dev, trace) = device(script);
        let result = firehose_program_storage(&mut dev, &mut reader, "p", 2, 0, 0, "0");
        if truncated_len == 8 {
            result.unwrap();
            assert_eq!(trace.lock().unwrap().writes[1..].concat(), b"abcdefgh");
        } else {
            let err = result.unwrap_err();
            assert_eq!(err.confirmation, MutationConfirmation::Unknown);
            assert_eq!(err.cause.downcast_ref::<std::io::Error>().unwrap().kind(), ErrorKind::UnexpectedEof);
            assert_eq!(trace.lock().unwrap().writes.len(), 1);
        }
        drop(dev);
        assert_no_reset(&trace);
    }
}

#[test]
fn real_xml_default_offset_and_failure_preserve_type_and_stop_next_entry() {
    for (final_response, confirmation) in [(bytes(NAK), MutationConfirmation::Rejected), (Input::Error(ErrorKind::TimedOut), MutationConfirmation::Unknown)] {
        let fixture = Fixture::new(b"a");
        let entry = "<program SECTOR_SIZE_IN_BYTES=\"4\" num_partition_sectors=\"1\" physical_partition_number=\"0\" start_sector=\"0\" label=\"p\" filename=\"image.bin\"/>";
        let xml = xmltree::Element::parse(format!("<data>{entry}{entry}</data>").as_bytes()).unwrap();
        let (mut dev, trace) = device([bytes(ACK), final_response]);
        let err = programfile::parse_program_xml(&mut dev, &xml, &fixture.0, &fixture.0, true, false).unwrap_err();
        assert_eq!(err.downcast_ref::<MutationFailure>().unwrap().confirmation, confirmation);
        assert_eq!(trace.lock().unwrap().writes.len(), 2);
        assert_eq!(trace.lock().unwrap().writes[1], b"a\0\0\0");
        drop(dev);
        assert_no_reset(&trace);
    }
}
