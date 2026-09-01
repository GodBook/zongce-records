mod db;
mod error;
mod excel;
mod models;
mod transfer;

use std::time::Duration;

use db::AppState;
use error::{AppError, AppResult};
use models::{
    AppInitialization, AssessmentRecord, BackupInspection, Category, CategoryDraft, ImportPreview,
    MaterialPreview, OperationResult, RecordDraft, RecordFilter, RecordListResult,
    StatisticsResult, StorageStatus, UpdateInfo,
};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_updater::UpdaterExt;

#[tauri::command]
fn initialize_app(state: State<'_, AppState>, app: AppHandle) -> AppResult<AppInitialization> {
    db::initialize_app(&state, &app.package_info().version.to_string())
}

#[tauri::command]
fn list_records(state: State<'_, AppState>, filter: RecordFilter) -> AppResult<RecordListResult> {
    db::list_records(&state, filter)
}

#[tauri::command]
fn list_academic_years(state: State<'_, AppState>) -> AppResult<Vec<String>> {
    db::list_academic_years(&state)
}

#[tauri::command]
fn get_record(state: State<'_, AppState>, id: String) -> AppResult<AssessmentRecord> {
    db::get_record(&state, &id)
}

#[tauri::command]
fn save_record(state: State<'_, AppState>, draft: RecordDraft) -> AppResult<AssessmentRecord> {
    db::save_record(&state, draft)
}

#[tauri::command]
fn move_records_to_trash(
    state: State<'_, AppState>,
    ids: Vec<String>,
) -> AppResult<OperationResult> {
    db::move_records_to_trash(&state, &ids)
}

#[tauri::command]
fn restore_records(state: State<'_, AppState>, ids: Vec<String>) -> AppResult<OperationResult> {
    db::restore_records(&state, &ids)
}

#[tauri::command]
fn permanently_delete_records(
    state: State<'_, AppState>,
    ids: Vec<String>,
) -> AppResult<OperationResult> {
    db::permanently_delete_records(&state, &ids)
}

#[tauri::command]
fn list_categories(state: State<'_, AppState>) -> AppResult<Vec<Category>> {
    db::list_categories(&state)
}

#[tauri::command]
fn save_category(state: State<'_, AppState>, category: CategoryDraft) -> AppResult<Category> {
    db::save_category(&state, category)
}

#[tauri::command]
fn set_category_active(
    state: State<'_, AppState>,
    id: String,
    is_active: bool,
) -> AppResult<Category> {
    db::set_category_active(&state, &id, is_active)
}

#[tauri::command]
fn get_statistics(state: State<'_, AppState>, filter: RecordFilter) -> AppResult<StatisticsResult> {
    db::get_statistics(&state, filter)
}

#[tauri::command]
fn export_excel(
    state: State<'_, AppState>,
    path: String,
    filter: Option<RecordFilter>,
    template_only: Option<bool>,
) -> AppResult<OperationResult> {
    transfer::export_excel(
        &state,
        std::path::Path::new(&path),
        filter,
        template_only.unwrap_or(false),
    )
}

#[tauri::command]
fn preview_excel(state: State<'_, AppState>, path: String) -> AppResult<ImportPreview> {
    transfer::preview_excel(&state, std::path::Path::new(&path))
}

#[tauri::command]
fn commit_excel(
    state: State<'_, AppState>,
    token: String,
    include_duplicates: Option<bool>,
) -> AppResult<OperationResult> {
    transfer::commit_excel_with_options(&state, &token, include_duplicates.unwrap_or(false))
}

#[tauri::command]
fn export_material_package(
    state: State<'_, AppState>,
    record_ids: Vec<String>,
    path: String,
) -> AppResult<OperationResult> {
    transfer::export_material_package(&state, &record_ids, std::path::Path::new(&path))
}

#[tauri::command]
fn export_backup(
    state: State<'_, AppState>,
    app: AppHandle,
    path: String,
) -> AppResult<OperationResult> {
    transfer::export_backup(
        &state,
        std::path::Path::new(&path),
        &app.package_info().version.to_string(),
    )
}

#[tauri::command]
fn inspect_backup(state: State<'_, AppState>, path: String) -> AppResult<BackupInspection> {
    transfer::inspect_backup(&state, std::path::Path::new(&path))
}

#[tauri::command]
fn restore_backup(
    state: State<'_, AppState>,
    token: String,
    mode: String,
) -> AppResult<OperationResult> {
    transfer::restore_backup(&state, &token, &mode)
}

#[tauri::command]
fn get_storage_status(state: State<'_, AppState>) -> AppResult<StorageStatus> {
    db::get_storage_status(&state)
}

#[tauri::command]
fn migrate_data_root(
    state: State<'_, AppState>,
    destination: String,
) -> AppResult<OperationResult> {
    db::migrate_data_root(&state, &destination)
}

#[tauri::command]
fn open_material(state: State<'_, AppState>, material_id: String) -> AppResult<OperationResult> {
    db::open_material(&state, &material_id)
}

#[tauri::command]
fn get_material_preview(
    state: State<'_, AppState>,
    app: AppHandle,
    material_id: String,
) -> AppResult<MaterialPreview> {
    let preview = db::get_material_preview(&state, &material_id)?;
    app.asset_protocol_scope()
        .allow_file(&preview.path)
        .map_err(|error| {
            AppError::new(
                "PREVIEW_SCOPE_ERROR",
                format!("无法授权证明材料预览：{error}"),
            )
        })?;
    Ok(preview)
}

#[tauri::command]
async fn check_for_update(app: AppHandle) -> AppResult<UpdateInfo> {
    let updater = app
        .updater_builder()
        .timeout(Duration::from_secs(8))
        .build()
        .map_err(|error| AppError::new("UPDATER_ERROR", format!("无法初始化更新器：{error}")))?;
    let current_version = app.package_info().version.to_string();
    let update = updater
        .check()
        .await
        .map_err(|error| AppError::new("UPDATE_CHECK_FAILED", format!("检查更新失败：{error}")))?;

    Ok(match update {
        Some(update) => UpdateInfo {
            available: true,
            current_version,
            version: update.version,
            published_at: update.date.map(|date| date.to_string()).unwrap_or_default(),
            notes: update
                .body
                .unwrap_or_else(|| "该版本没有发布说明。".to_string()),
        },
        None => UpdateInfo {
            available: false,
            version: current_version.clone(),
            current_version,
            published_at: String::new(),
            notes: "当前已经是最新版本。".to_string(),
        },
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let state = AppState::new().expect("无法初始化综测记录本地数据");

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_opener::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            initialize_app,
            list_records,
            list_academic_years,
            get_record,
            save_record,
            move_records_to_trash,
            restore_records,
            permanently_delete_records,
            list_categories,
            save_category,
            set_category_active,
            get_statistics,
            export_excel,
            preview_excel,
            commit_excel,
            export_material_package,
            export_backup,
            inspect_backup,
            restore_backup,
            get_storage_status,
            migrate_data_root,
            open_material,
            get_material_preview,
            check_for_update,
        ])
        .run(tauri::generate_context!())
        .expect("运行综测记录失败");
}
