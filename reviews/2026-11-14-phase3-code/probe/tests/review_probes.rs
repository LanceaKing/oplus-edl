mod support;
use oplus_edl::{backup, restore, sparse, program_xml, output::Report, storage::Partition};
use std::{fs,io::Cursor};
use support::*;
fn part(name:&str)->Partition { Partition{lun:0,name:name.into(),start_sector:64,sectors:8,sector_size:512} }
fn sparse_fixture(block:u32, zero:bool)->Vec<u8>{
 let mut b=Vec::new(); b.extend(sparse::MAGIC.to_le_bytes()); b.extend(1u16.to_le_bytes()); b.extend(0u16.to_le_bytes());b.extend(28u16.to_le_bytes());b.extend(12u16.to_le_bytes());
 for n in [block,4096/block,if zero{2}else{1},0] {b.extend(n.to_le_bytes());}
 if zero {b.extend(0xcac3u16.to_le_bytes());b.extend(0u16.to_le_bytes());b.extend(0u32.to_le_bytes());b.extend(12u32.to_le_bytes());}
 b.extend(0xcac1u16.to_le_bytes());b.extend(0u16.to_le_bytes());b.extend((4096/block).to_le_bytes());b.extend(4108u32.to_le_bytes());b.extend([7;4096]);b
}
#[test] fn reproduces_aligned_sparse_rejection(){
 let e=sparse::parse(&mut Cursor::new(sparse_fixture(512,false)),4096).unwrap_err();println!("aligned chunk rejected: {e}");
 assert!(sparse::parse(&mut Cursor::new(sparse_fixture(4096,false)),4096).is_ok());
 let e=sparse::parse(&mut Cursor::new(sparse_fixture(4096,true)),4096).unwrap_err();println!("zero DONT_CARE rejected: {e}");
}
#[test] fn reproduces_backup_restore_case_asymmetry(){
 let d=disk(512,&["boot","Boot"]);let h=gptman::GPTHeader::read_from(&mut Cursor::new(&d[512..])).unwrap();
 let mut input=gpt_script(&d,512);input.extend([bytes(ACK),bytes(&d[(h.last_usable_lba as usize+1)*512..]),bytes(ACK),bytes(ACK),bytes(&[3;1024]),bytes(ACK)]);
 let (mut dev,trace)=storage_device(input,512);let tmp=tempfile::tempdir().unwrap();let mut report=Report::new(Some("backup"),None);
 backup::directory(&mut dev,&[0],&["boot".into()],tmp.path(),false,&mut report);
 assert_eq!(report.status,oplus_edl::output::Status::Success);
 let e=restore::directory(tmp.path(),Some(0),&["boot".into()],&[part("boot"),part("Boot")]).unwrap_err();println!("successful backup rejected on restore: {e}");assert_no_reset(&trace);
}
#[test] fn reproduces_preflight_failed_status_without_read(){
 let d=disk(512,&["../bad"]);let (mut dev,trace)=storage_device(gpt_script(&d,512),512);let tmp=tempfile::tempdir().unwrap();let mut report=Report::new(Some("backup"),None);
 backup::directory(&mut dev,&[0],&[],tmp.path(),false,&mut report);
 let item=report.items.last().unwrap();assert_eq!(item.execution,"failed");assert_eq!(item.transferred_bytes.as_deref(),Some("0"));assert_eq!(trace.lock().unwrap().writes.len(),2);
 println!("preflight result: {}",serde_json::to_string(item).unwrap()); report.emit(false);
}
#[cfg(unix)]
#[test] fn reproduces_fifo_block_before_type_check(){
 for firmware in [false,true] {
  let tmp=tempfile::tempdir().unwrap();let fifo=tmp.path().join("fifo");assert!(std::process::Command::new("mkfifo").arg(&fifo).status().unwrap().success());
  let xml=tmp.path().join("program.xml");fs::write(&xml,r#"<data><program filename="fifo" SECTOR_SIZE_IN_BYTES="512" physical_partition_number="0" start_sector="64" num_partition_sectors="1"/></data>"#).unwrap();
  let path=fifo.clone();let (tx,rx)=std::sync::mpsc::channel();let worker=std::thread::spawn(move||{let result=if firmware{program_xml::parse(&[xml],&[],512,false).map(|_|())}else{restore::single(&path,"boot",Some(0),&[part("boot")]).map(|_|())};tx.send(result.map_err(|e|e.to_string())).unwrap();});
  assert!(rx.recv_timeout(std::time::Duration::from_millis(300)).is_err());
  let peer=fs::OpenOptions::new().write(true).open(&fifo).unwrap();let result=rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap();assert!(result.is_err());drop(peer);worker.join().unwrap();println!("{} FIFO blocked until writer opened",if firmware{"XML"}else{"restore"});
 }
}
