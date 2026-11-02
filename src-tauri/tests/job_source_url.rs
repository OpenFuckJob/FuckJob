use offer_flow_lib::dao::model::JobDetail;
use serde_json::json;

#[test]
fn original_url_survives_job_roundtrip_and_legacy_records_still_load() {
    let mut value = json!({"id":"abc", "platform":"boss", "title":"开发", "company_name":"公司",
        "detail":"职责", "salary":"10k", "location":null, "is_reply":false,
        "is_send_resume":true, "created_at":"now", "resume_sent_at":"now", "updated_at":"now"});
    let legacy: JobDetail = serde_json::from_value(value.clone()).unwrap();
    assert!(serde_json::to_value(legacy).unwrap()["source_url"].is_null());
    value["source_url"] = json!("https://www.zhipin.com/job_detail/abc.html?securityId=original");
    let job: JobDetail = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(
        serde_json::to_value(job).unwrap()["source_url"],
        value["source_url"]
    );
}

#[test]
fn pending_submission_survives_storage_separately_from_manual_review_notifications() {
    let input=json!({"id":"51job:173034896", "platform":"51job", "title":"开发", "company_name":"公司",
        "detail":"职责", "salary":"10k", "location":null, "is_reply":false,
        "is_send_resume":false, "resume_delivery_pending":true, "created_at":"now", "resume_sent_at":null, "updated_at":"now"});
    let job:JobDetail=serde_json::from_value(input).unwrap();
    assert_eq!(serde_json::to_value(job).unwrap()["resume_delivery_pending"],json!(true));
}
