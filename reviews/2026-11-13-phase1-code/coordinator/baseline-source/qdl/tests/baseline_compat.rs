mod support;
#[path="../../cli/src/programfile.rs"] mod programfile;
use support::*;
use qdl::{firehose_read,firehose_program_storage,parsers::firehose_parser_ack_nak,types::FirehoseStatus};
use std::io::ErrorKind;
#[test]
fn baseline_welcome_whitespace_succeeds() {
 let (mut dev, _)=device([bytes(&[LOG,b"\r\n"].concat()),Input::Error(ErrorKind::TimedOut)]);
 assert_eq!(firehose_read(&mut dev,firehose_parser_ack_nak).unwrap(),FirehoseStatus::Ack);
}
#[test]
fn baseline_erase_one_byte_source_supplies_full_zero_payload() {
 let (mut dev, trace)=device([bytes(ACK),bytes(ACK)]);
 firehose_program_storage(&mut dev,&mut &[0u8][..],"p",2,0,0,"0").unwrap();
 assert_eq!(trace.lock().unwrap().writes[1],vec![0;8]);
}
#[test]
fn baseline_xml_caller_pads_last_sector() {
 let dir=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
 std::fs::create_dir_all(&dir).unwrap();std::fs::write(dir.join("p.bin"),b"abcdefghi").unwrap();
 let xml=xmltree::Element::parse(&b"<data><program SECTOR_SIZE_IN_BYTES=\"4\" num_partition_sectors=\"3\" physical_partition_number=\"0\" start_sector=\"0\" label=\"p\" filename=\"p.bin\"/></data>"[..]).unwrap();
 let (mut dev, trace)=device([bytes(ACK),bytes(ACK)]);
 programfile::parse_program_xml(&mut dev,&xml,&dir,&dir,false,false).unwrap();
 let r=trace.lock().unwrap();assert_eq!(r.writes.len(),3);assert_eq!(r.writes[1],b"abcdefgh");assert_eq!(r.writes[2],b"i\0\0\0");
}
