use offer_flow_lib::{command::manual_review::job51_resolve_delivery,
    dao::{self, job_detail_dao, manual_review_dao, model::{JobDetail, ManualReviewReason}}};
use serde_json::json;

#[test]
fn clearing_notifications_preserves_pending_state_until_explicit_confirmation_or_retry() {
    let directory=tempfile::tempdir().unwrap();
    dao::init(directory.path()).unwrap();
    let id="51job:173034896";
    let pending:JobDetail=serde_json::from_value(json!({"id":id,"platform":"51job","title":"Java开发","company_name":"测试公司",
        "detail":"开发职责","salary":"1万","location":"深圳","is_reply":false,"is_send_resume":false,
        "resume_delivery_pending":true,"source_url":"https://jobs.51job.com/shenzhen/173034896.html?original=1",
        "created_at":"now","resume_sent_at":null,"updated_at":"now"})).unwrap();
    job_detail_dao::create(pending.clone()).unwrap();
    manual_review_dao::upsert(manual_review_dao::ReviewRequest { platform:"51job",conversation_id:"173034896",job_id:id,
        job_name:&pending.title,company_name:&pending.company_name,reason:ManualReviewReason::ResumeDelivery,
        detail:"提交待确认".into(),last_message:String::new() }).unwrap();
    manual_review_dao::clear().unwrap();
    assert!(job_detail_dao::get_by_id(id).unwrap().unwrap().resume_delivery_pending);
    assert!(job51_resolve_delivery(id.into(),false).success);
    let retry=job_detail_dao::get_by_id(id).unwrap().unwrap();
    assert!(!retry.resume_delivery_pending && !retry.is_send_resume);
    assert_eq!(retry.source_url,pending.source_url);
    job_detail_dao::update(id,pending).unwrap();
    assert!(job51_resolve_delivery(id.into(),true).success);
    let confirmed=job_detail_dao::get_by_id(id).unwrap().unwrap();
    assert!(!confirmed.resume_delivery_pending && confirmed.is_send_resume);
    assert!(confirmed.resume_sent_at.is_some());
    assert!(job51_resolve_delivery(id.into(),true).success);
    assert_eq!(job_detail_dao::get_by_id(id).unwrap().unwrap().resume_sent_at,confirmed.resume_sent_at);
    assert!(!job51_resolve_delivery(id.into(),false).success);
}
