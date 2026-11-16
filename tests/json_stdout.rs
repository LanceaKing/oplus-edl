//! A separate process captures real stdout, including dependency logs and progress.
//! No test harness output is mixed into the protocol result.
mod support;

use clap::Parser;
use oplus_edl::{
    cli::{Cli, Verify},
    device,
    output::{Report, Status},
    restore,
    storage::Partition,
    write::Writes,
};
use support::*;

fn packet(command: u32, args: &[u32]) -> Vec<u8> {
    [command, 8 + 4 * args.len() as u32]
        .into_iter()
        .chain(args.iter().copied())
        .flat_map(u32::to_le_bytes)
        .collect()
}

fn child(with_digest: bool) {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("image.bin");
    std::fs::write(&path, [7; 511]).unwrap();
    let mappings = restore::single(
        &path,
        "boot",
        Some(0),
        &[Partition {
            lun: 0,
            name: "boot".into(),
            start_sector: 64,
            sectors: 2,
            sector_size: 512,
        }],
    )
    .unwrap();
    let writes = Writes::restore(vec![mappings], 512).unwrap();
    let mut expected = vec![7; 511];
    expected.push(0);
    let mut input = vec![
        bytes(&packet(1, &[2, 1, 4096, 0, 0, 0, 0, 0, 0, 0])),
        bytes(&packet(3, &[13, 0, 4])), bytes(&packet(4, &[13, 0])), bytes(&packet(6, &[1])),
        bytes(b"<data><log value=\"INFO: End of supported functions\"/></data>"),
        bytes(b"<data><response value=\"ACK\" MinVersionSupported=\"1\" Version=\"1\" MaxPayloadSizeToTargetInBytesSupported=\"4096\" MaxPayloadSizeToTargetInBytes=\"4096\" MaxXMLSizeInBytes=\"4096\"/></data>"),
    ];
    if with_digest {
        input.insert(5, bytes(ACK));
    }
    input.extend([
        bytes(LOG),
        bytes(ACK),
        bytes(ACK),
        bytes(LOG),
        bytes(ACK),
        bytes(&expected),
        bytes(LOG),
        bytes(ACK),
    ]);
    let (mut dev, trace) = storage_device(input, 512);
    let cli = Cli::parse_from([
        "oplus-edl",
        "--memory",
        "emmc",
        "--json",
        "restore",
        "--image",
        "unused",
        "--partition",
        "boot",
    ]);
    dev.fh_cfg = device::configuration(&cli).unwrap();
    if with_digest {
        device::initialize_with_progress(
            &mut dev,
            &mut [Some(b"abcd".to_vec())],
            Some(&[0xa5; 512]),
            |message| eprintln!("{message}"),
        )
        .unwrap();
    } else {
        device::initialize(&mut dev, &mut [Some(b"abcd".to_vec())]).unwrap();
    }
    let mut report = Report::new(Some("restore"), Some("best-effort"));
    writes.execute(&mut dev, Verify::BestEffort, &mut report);
    drop(dev);
    assert_no_reset(&trace);
    assert_eq!(report.status, Status::Success);
    assert_eq!(report.items[0].verification, "matched");
    report.emit(true);
}

fn main() {
    if std::env::args().any(|arg| arg == "--protocol-child") {
        child(std::env::args().any(|arg| arg == "--with-digest"));
        return;
    }
    for with_digest in [false, true] {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command.arg("--protocol-child");
        if with_digest {
            command.arg("--with-digest");
        }
        let result = command.output().unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let value: serde_json::Value = serde_json::from_slice(&result.stdout)
            .expect("exactly one JSON object, without dependency logs or progress");
        assert_eq!(value["status"], "success");
        assert_eq!(value["items"][0]["write"], "acknowledged");
        assert_eq!(value["items"][0]["verification"], "matched");
        if with_digest {
            assert!(
                String::from_utf8_lossy(&result.stderr)
                    .contains("signed digest table acknowledged")
            );
        } else {
            assert!(
                result.stderr.is_empty(),
                "quiet protocol emitted stderr: {}",
                String::from_utf8_lossy(&result.stderr)
            );
        }
    }
    println!("JSON stdout protocol subprocess: passed");
}
