use offer_flow_lib::config::{default_app_config, resolve_job_profile, PlatformFilterConfig};
use serde_json::json;

#[test]
fn platform_filter_roundtrip_preserves_51job_choices_and_profile_snapshot() {
    let input =
        json!({"job51":{"salary":["201","07"],"functions":["0121"],"company_size":["02","03"]}});
    let filters: PlatformFilterConfig = serde_json::from_value(input.clone()).unwrap();
    assert_eq!(
        serde_json::to_value(&filters).unwrap()["job51"],
        input["job51"]
    );
    let mut config = default_app_config();
    config.job_profiles[0].platform_filter_config = filters;
    let resolved = resolve_job_profile(&config, None).unwrap();
    assert_eq!(
        serde_json::to_value(resolved.config.platform_filter_config).unwrap()["job51"],
        input["job51"]
    );
}
