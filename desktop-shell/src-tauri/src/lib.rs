mod commands;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(commands::DesktopState::default())
        .invoke_handler(tauri::generate_handler![
            commands::desktop_info,
            commands::resolve_runtime_profile,
            commands::resolve_runtime_status,
            commands::start_runtime,
            commands::repair_runtime,
            commands::reset_runtime,
            commands::open_workspace_file,
            commands::run_agent,
            commands::run_cron_job,
            commands::start_cron_scheduler,
            commands::stop_cron_scheduler,
            commands::cron_scheduler_status,
            commands::resume_approval,
            commands::clear_session,
            commands::stop_session,
            commands::list_approvals,
            commands::resolve_approval,
            commands::list_skills,
            commands::view_skill,
            commands::view_workspace_file,
            commands::browser_stream_endpoint,
            commands::list_workspace_tree,
            commands::extensions_overview,
            commands::list_providers,
            commands::resolve_provider_status,
            commands::load_shared_provider_config,
            commands::save_shared_provider_config,
            commands::save_cron_job,
            commands::delete_cron_job,
            commands::inspect_mcp_server,
            commands::list_sessions,
            commands::load_session,
            commands::search_sessions,
            commands::list_delegate_runs,
            commands::cancel_delegate_run,
            commands::retry_delegate_run,
            commands::pick_workspace_folder,
        ])
        .run(tauri::generate_context!())
        .expect("error while running hermes-agent-rs desktop shell");
}
