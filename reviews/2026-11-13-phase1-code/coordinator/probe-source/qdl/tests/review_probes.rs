// Review-only counterexamples; no devices are opened.
mod support;
use support::*;
use qdl::{firehose_read_welcome,firehose_program_storage,firehose_read_storage,types::ReadRejected};
use std::io::ErrorKind;

#[test]
fn welcome_trailing_xml_whitespace_should_allow_idle_timeout() {
    let (mut dev, _) = device([bytes(b"<?xml version=\"1.0\"?><data><log value=\"ready\"/></data>\n"), Input::Error(ErrorKind::TimedOut)]);
    let result = firehose_read_welcome(&mut dev);
    assert!(result.is_ok(), "complete XML plus valid trailing whitespace was rejected: {result:?}");
}

#[test]
fn program_final_ack_still_in_raw_mode_should_not_confirm_completion() {
    let raw_ack = b"<data><response value=\"ACK\" rawmode=\"true\"/></data>";
    let (mut dev, _) = device([bytes(raw_ack),bytes(raw_ack)]);
    let result=firehose_program_storage(&mut dev,&mut &b"abcdefgh"[..],"p",2,0,0,"0");
    assert!(result.is_err(), "program returned success while final response still declares rawmode=true");
}

#[test]
fn restart_request_should_not_be_reported_as_safe_read_rejection() {
    let (mut dev, _) = device([bytes(b"<data><response value=\"NAK\" AttemptRestart=\"1\"/></data>")]);
    let error=firehose_read_storage(&mut dev,&mut Vec::new(),2,0,0,0).unwrap_err();
    assert!(error.downcast_ref::<ReadRejected>().is_none(), "restart-required session was classified as ordinary ReadRejected");
}

#[test]
fn initial_ack_without_raw_mode_should_not_send_program_data() {
    let ack = b"<data><response value=\"ACK\" rawmode=\"false\"/></data>";
    let (mut dev, trace)=device([bytes(ack),bytes(ack)]);
    let result=firehose_program_storage(&mut dev,&mut &b"abcdefgh"[..],"p",2,0,0,"0");
    assert!(result.is_err(), "program sent {} writes without raw-mode acceptance",trace.lock().unwrap().writes.len());
}


#[test]
fn rawmode_nak_should_not_be_reported_as_safe_read_rejection() {
    let (mut dev, _) = device([bytes(b"<data><response value=\"NAK\" rawmode=\"true\"/></data>")]);
    let error=firehose_read_storage(&mut dev,&mut Vec::new(),2,0,0,0).unwrap_err();
    assert!(error.downcast_ref::<ReadRejected>().is_none(), "raw-mode session was classified as ordinary ReadRejected");
}
