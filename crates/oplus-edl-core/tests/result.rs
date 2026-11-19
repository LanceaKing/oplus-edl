use oplus_edl_core::result::{ExecutionState, VerificationState, WriteState};
use oplus_edl_core::result::{Item, Report, Status};

#[test]
fn result_priority_retains_all_facts_and_decimal_byte_strings() {
    let mut report = Report::new(Some("backup"), None);
    for status in [
        Status::VerificationIncomplete,
        Status::Unsupported,
        Status::InputError,
        Status::ExecutionFailed,
        Status::UnknownWrite,
    ] {
        let mut item = Item::read(0, Some("boot".into()), u64::MAX, 4096);
        item.fail(status, "test");
        report.items.push(item);
        report.summarize();
        assert_eq!(report.status, status);
    }
    let value = serde_json::to_value(&report).unwrap();
    assert_eq!(value["items"][0]["start_byte"], u64::MAX.to_string());
    assert_eq!(value["items"].as_array().unwrap().len(), 5);
    for (i, item) in value["items"].as_array().unwrap().iter().enumerate() {
        assert_eq!(item["id"], i + 1);
    }
    assert_eq!(Report::new(Some("backup"), None).status, Status::Success);
}
#[test]
fn typed_mutation_failures_map_without_error_string_matching() {
    use qdl::types::MutationFailure;
    for (error, expected, status) in [
        (
            MutationFailure::not_sent(anyhow::anyhow!("x")),
            WriteState::NotAttempted,
            Status::ExecutionFailed,
        ),
        (
            MutationFailure::rejected(anyhow::anyhow!("x")),
            WriteState::Failed,
            Status::ExecutionFailed,
        ),
        (
            MutationFailure::unknown(anyhow::anyhow!("x")),
            WriteState::Unknown,
            Status::UnknownWrite,
        ),
    ] {
        let mut item = Item::read(0, None, 0, 512);
        item.operation = "program".into();
        item.mutation_failure(&error);
        assert_eq!(item.write, expected);
        assert_eq!(item.execution, ExecutionState::Failed);
        assert_eq!(item.verification, VerificationState::NotApplicable);
        assert_eq!(item.errors[0].category, status);
    }
}
