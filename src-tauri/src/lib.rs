pub mod browser;
pub mod command;
pub mod config;
pub mod credential;
pub mod dao;
pub mod error;
pub mod llm;
pub mod logger;
pub mod rpa;
pub mod storage;
pub mod utils;
pub mod verify;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let handle = app.handle().clone();
            let data_dir = handle.path().app_data_dir().map_err(|e| e.to_string())?;
            let config_path = config::config_path(&handle).map_err(|e| e.to_string())?;
            storage::migration::migrate_v0_to_v1(
                &storage::migration::MigrationPaths::new(config_path, data_dir.clone()),
                &credential::KeyringCredentialBackend,
            )
            .map_err(|e| e.to_string())?;
            browser::init_app_handle(handle.clone()).map_err(|e| e.to_string())?;
            logger::init(&handle).map_err(|e| e.to_string())?;
            dao::init(&data_dir).map_err(|e| e.to_string())?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            command::config::load_app_config,
            command::config::save_app_config,
            command::config::import_app_config,
            command::config::export_app_config,
            command::config::parse_resume_pdf,
            command::config::check_browser_env,
            command::user_resumes::load_user_resumes,
            command::user_resumes::save_user_resumes,
            command::resume_templates::get_resume_templates,
            command::rpa::run_flow::check_env,
            command::rpa::run_flow::get_readiness_report,
            command::rpa::run_flow::preflight_job_task,
            command::rpa::run_flow::boss_flow,
            command::rpa::run_flow::rpa_flow,
            command::rpa::run_flow::get_job_task_status,
            command::rpa::run_flow::stop_job_task,
            command::rpa::run_flow::read_log_file,
            command::job::job_list,
            command::job::job_get,
            command::job::job_create,
            command::job::job_update,
            command::job::job_delete,
            command::job::job_query,
            command::job::job_analyze,
            command::job::chat_messages_by_job,
            command::communicated_jobs::job_collect_communicated,
            command::analysis::analysis_list,
            command::analysis::analysis_get_by_job_id,
            command::analysis::analysis_create,
            command::analysis::analysis_delete,
            command::llm::debug_generate_replay,
            command::llm::debug_generate_greet,
            command::llm::generate_job_filter_rules,
            command::llm::predict_resume_questions,
            command::llm::optimize_resume_with_answer,
            command::llm_provider::get_llm_credential_status,
            command::llm_provider::set_llm_api_key,
            command::llm_provider::clear_llm_api_key,
            command::llm_provider::test_llm_connection,
            command::mock_interview::stream_mock_interview_question,
            command::mock_interview::stream_mock_interview_summary
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
