use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tempfile::TempDir;
use uuid::Uuid;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::db::{
    self, create_daily_recovery, database_path, excel_rows_for_filter, format_score, hash_file,
    integrity_check, material_files_for_records, now_iso, parse_score_cents, safe_material_path,
    AppState, PendingImport,
};
use crate::error::{AppError, AppResult};
use crate::excel::{self, ExcelRecordRow};
use crate::models::{
    AssessmentLevel, BackupInspection, ImportPreview, ImportRowPreview, OperationResult,
    RecordFilter,
};

const BACKUP_FORMAT: &str = "zongce-records-backup";
const BACKUP_SCHEMA_VERSION: u32 = 1;
const MAX_EXCEL_BYTES: u64 = 50 * 1024 * 1024;
const MAX_IMPORT_ROWS: usize = 50_000;
const MAX_ARCHIVE_ENTRIES: usize = 25_000;
const MAX_ARCHIVE_UNCOMPRESSED: u64 = 8 * 1024 * 1024 * 1024;
const MAX_DATABASE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_MANIFEST_BYTES: u64 = 16 * 1024 * 1024;
const MAX_COMPRESSION_RATIO: u64 = 500;
const MIN_EXTRACTION_FREE_SPACE: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManifestFile {
    path: String,
    sha256: String,
    size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BackupManifest {
    format: String,
    schema_version: u32,
    app_version: String,
    created_at: String,
    record_count: i64,
    material_count: i64,
    database: ManifestFile,
    materials: Vec<ManifestFile>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PackageManifest {
    format: &'static str,
    version: u32,
    created_at: String,
    records: Vec<PackageRecord>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PackageRecord {
    id: String,
    name: String,
    date: String,
    level: String,
    score: String,
    folder: String,
    materials: Vec<PackageMaterial>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PackageMaterial {
    id: String,
    name: String,
    path: String,
    sha256: String,
    size: u64,
    mime_type: String,
}

pub fn export_excel(
    state: &AppState,
    destination: &Path,
    filter: Option<RecordFilter>,
    template_only: bool,
) -> AppResult<OperationResult> {
    ensure_extension(destination, "xlsx")?;
    if template_only {
        excel::write_template(destination)
            .map_err(|error| AppError::new("EXCEL_EXPORT_FAILED", error))?;
        return Ok(
            OperationResult::success("导入模板已保存").with_path(destination.to_string_lossy())
        );
    }

    let filter = filter.unwrap_or_default();
    let connection = state.connection()?;
    let rows = excel_rows_for_filter(&connection, filter.clone())?;
    let statistics = db::get_statistics(state, filter)?;
    let summary = statistics.summary;
    let stats = vec![
        ("记录数".to_string(), summary.record_count.to_string()),
        ("总分".to_string(), summary.total_score),
        ("附件数".to_string(), summary.material_count.to_string()),
        (
            "待补材料数".to_string(),
            summary.missing_material_count.to_string(),
        ),
    ];
    excel::write_records(destination, &rows, &stats)
        .map_err(|error| AppError::new("EXCEL_EXPORT_FAILED", error))?;
    Ok(
        OperationResult::success(format!("已导出 {} 条综测记录", rows.len()))
            .with_affected(rows.len())
            .with_path(destination.to_string_lossy()),
    )
}

pub fn preview_excel(state: &AppState, path: &Path) -> AppResult<ImportPreview> {
    preflight_excel(path)?;
    let parsed =
        excel::parse_records(path).map_err(|error| AppError::new("EXCEL_IMPORT_FAILED", error))?;
    if parsed.rows.len() > MAX_IMPORT_ROWS {
        return Err(AppError::new(
            "IMPORT_TOO_LARGE",
            format!("单次最多导入 {MAX_IMPORT_ROWS} 条记录"),
        ));
    }

    let connection = state.connection()?;
    let mut statuses = HashMap::new();
    let mut previews = Vec::new();
    let duplicate_rows = duplicate_import_id_rows(&parsed.rows, &parsed.row_numbers);
    let mut issue_groups: HashMap<u32, Vec<String>> = HashMap::new();
    for issue in &parsed.issues {
        let message = if issue.column.is_empty() {
            issue.message.clone()
        } else {
            format!("{}：{}", issue.column, issue.message)
        };
        issue_groups.entry(issue.row).or_default().push(message);
    }
    let mut issue_rows: Vec<_> = issue_groups.into_iter().collect();
    issue_rows.sort_by_key(|(row, _)| *row);
    for (row, messages) in issue_rows {
        previews.push(ImportRowPreview {
            row,
            status: "error".to_string(),
            name: String::new(),
            message: messages.join("；"),
        });
    }

    for (index, row) in parsed.rows.iter().enumerate() {
        let source_row = parsed
            .row_numbers
            .get(index)
            .copied()
            .unwrap_or(index as u32 + 2);
        let (status, message) = if let Some(message) = duplicate_rows.get(&source_row) {
            ("error".to_string(), message.clone())
        } else {
            classify_import_row(&connection, row)?
        };
        statuses.insert(source_row, status.clone());
        previews.push(ImportRowPreview {
            row: source_row,
            status,
            name: row.title.clone(),
            message,
        });
    }
    previews.sort_by_key(|row| row.row);

    let count = |name: &str| previews.iter().filter(|row| row.status == name).count();
    let token = Uuid::new_v4().to_string();
    let preview = ImportPreview {
        token: token.clone(),
        file_name: path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("综测记录.xlsx")
            .to_string(),
        total: previews.len(),
        new_count: count("new"),
        update_count: count("update"),
        skip_count: count("skip"),
        duplicate_count: count("duplicate"),
        error_count: count("error"),
        rows: previews,
    };
    state.imports.lock().insert(
        token,
        PendingImport {
            rows: parsed.rows,
            row_numbers: parsed.row_numbers,
            statuses,
        },
    );
    Ok(preview)
}

fn duplicate_import_id_rows(rows: &[ExcelRecordRow], row_numbers: &[u32]) -> HashMap<u32, String> {
    let mut occurrences: HashMap<Uuid, Vec<u32>> = HashMap::new();
    for (index, row) in rows.iter().enumerate() {
        let Some(id) = row
            .id
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .and_then(|value| Uuid::try_parse(value).ok())
        else {
            continue;
        };
        let source_row = row_numbers.get(index).copied().unwrap_or(index as u32 + 2);
        occurrences.entry(id).or_default().push(source_row);
    }

    let mut duplicates = HashMap::new();
    for source_rows in occurrences.into_values().filter(|items| items.len() > 1) {
        let locations = source_rows
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join("、");
        let message = format!("记录 ID 在 Excel 第 {locations} 行重复，请为每条记录使用唯一 ID");
        for source_row in source_rows {
            duplicates.insert(source_row, message.clone());
        }
    }
    duplicates
}

fn ensure_unique_import_ids(rows: &[ExcelRecordRow], row_numbers: &[u32]) -> AppResult<()> {
    let duplicates = duplicate_import_id_rows(rows, row_numbers);
    if duplicates.is_empty() {
        return Ok(());
    }
    let mut rows: Vec<_> = duplicates.into_keys().collect();
    rows.sort_unstable();
    Err(AppError::new(
        "DUPLICATE_IMPORT_ID",
        format!(
            "Excel 包含重复记录 ID，涉及第 {} 行，请修正后重新预览",
            rows.iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join("、")
        ),
    ))
}

fn classify_import_row(
    connection: &Connection,
    row: &ExcelRecordRow,
) -> AppResult<(String, String)> {
    let _: i64 = parse_score_cents(&row.score)?;
    let _ =
        AssessmentLevel::parse(&row.level).ok_or_else(|| AppError::validation("综测级别无效"))?;
    let category_id: Option<String> = connection
        .query_row(
            "SELECT id FROM categories WHERE name = ?1 COLLATE NOCASE",
            [&row.category],
            |result| result.get(0),
        )
        .optional()?;
    if category_id.is_none() {
        return Ok((
            "error".to_string(),
            format!("活动类别“{}”不存在，请先在设置中添加", row.category),
        ));
    }

    if let Some(id) = row.id.as_deref().filter(|id| !id.trim().is_empty()) {
        if Uuid::try_parse(id).is_err() {
            return Ok(("error".to_string(), "记录 ID 格式无效".to_string()));
        }
        let exists: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM records WHERE id = ?1)",
            [id],
            |result| result.get(0),
        )?;
        return Ok(if exists {
            (
                "update".to_string(),
                "匹配记录 ID，将更新且保留原附件".to_string(),
            )
        } else {
            ("new".to_string(), "记录 ID 尚不存在，将新增".to_string())
        });
    }

    let score = parse_score_cents(&row.score)?;
    let duplicate: bool = connection.query_row(
        "SELECT EXISTS(
           SELECT 1 FROM records
           WHERE deleted_at IS NULL AND name = ?1 COLLATE NOCASE
             AND activity_date = ?2 AND score_cents = ?3
         )",
        params![row.title, row.date, score],
        |result| result.get(0),
    )?;
    Ok(if duplicate {
        (
            "duplicate".to_string(),
            "名称、日期和分数与现有记录相同，默认跳过".to_string(),
        )
    } else {
        ("new".to_string(), "将新增".to_string())
    })
}

#[cfg(test)]
pub fn commit_excel(state: &AppState, token: &str) -> AppResult<OperationResult> {
    commit_excel_with_options(state, token, false)
}

pub fn commit_excel_with_options(
    state: &AppState,
    token: &str,
    include_duplicates: bool,
) -> AppResult<OperationResult> {
    let connection = state.connection()?;
    if let Some(result) = connection
        .query_row(
            "SELECT result_json FROM import_commits WHERE token = ?1",
            [token],
            |row| row.get::<_, String>(0),
        )
        .optional()?
    {
        return serde_json::from_str(&result).map_err(AppError::from);
    }
    drop(connection);

    let pending = state.imports.lock().get(token).cloned().ok_or_else(|| {
        AppError::new(
            "IMPORT_TOKEN_EXPIRED",
            "导入预览已失效，请重新选择 Excel 文件",
        )
    })?;
    ensure_unique_import_ids(&pending.rows, &pending.row_numbers)?;
    create_daily_recovery(state)?;
    let mut connection = state.connection()?;
    let transaction = connection.transaction()?;
    let mut affected = 0_usize;
    let mut added = 0_usize;
    let mut updated = 0_usize;

    for (index, row) in pending.rows.iter().enumerate() {
        let source_row = pending
            .row_numbers
            .get(index)
            .copied()
            .unwrap_or(index as u32 + 2);
        let status = pending
            .statuses
            .get(&source_row)
            .map(String::as_str)
            .unwrap_or("skip");
        if status != "new" && status != "update" && !(include_duplicates && status == "duplicate") {
            continue;
        }
        let category_id: String = transaction
            .query_row(
                "SELECT id FROM categories WHERE name = ?1 COLLATE NOCASE",
                [&row.category],
                |result| result.get(0),
            )
            .map_err(|_| {
                AppError::new(
                    "CATEGORY_NOT_FOUND",
                    format!("类别“{}”已不存在，请重新预览", row.category),
                )
            })?;
        let level = AssessmentLevel::parse(&row.level)
            .ok_or_else(|| AppError::validation("导入数据中的综测级别无效"))?;
        let score = parse_score_cents(&row.score)?;
        let id = row
            .id
            .as_deref()
            .filter(|id| !id.trim().is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let now = now_iso();
        let exists: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM records WHERE id = ?1)",
            [&id],
            |result| result.get(0),
        )?;
        if exists {
            transaction.execute(
                "UPDATE records SET revision = revision + 1, name = ?2, category_id = ?3,
                 level = ?4, activity_date = ?5, score_cents = ?6, notes = ?7,
                 deleted_at = NULL, purge_at = NULL, updated_at = ?8 WHERE id = ?1",
                params![
                    id,
                    row.title.trim(),
                    category_id,
                    level.as_str(),
                    row.date,
                    score,
                    row.remark.trim(),
                    now
                ],
            )?;
            updated += 1;
        } else {
            transaction.execute(
                "INSERT INTO records
                 (id, revision, name, category_id, level, activity_date, score_cents, notes,
                  created_at, updated_at)
                 VALUES (?1, 1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
                params![
                    id,
                    row.title.trim(),
                    category_id,
                    level.as_str(),
                    row.date,
                    score,
                    row.remark.trim(),
                    now
                ],
            )?;
            added += 1;
        }
        affected += 1;
    }

    let result = OperationResult::success(format!("导入完成：新增 {added} 条，更新 {updated} 条"))
        .with_affected(affected);
    transaction.execute(
        "INSERT INTO import_commits(token, committed_at, result_json) VALUES (?1, ?2, ?3)",
        params![token, now_iso(), serde_json::to_string(&result)?],
    )?;
    transaction.commit()?;
    state.imports.lock().remove(token);
    Ok(result)
}

fn preflight_excel(path: &Path) -> AppResult<()> {
    let metadata = fs::metadata(path)
        .map_err(|error| AppError::io(&format!("无法读取 {}", path.display()), error))?;
    if !metadata.is_file() {
        return Err(AppError::validation("请选择 Excel 文件"));
    }
    if metadata.len() > MAX_EXCEL_BYTES {
        return Err(AppError::new("EXCEL_TOO_LARGE", "Excel 文件不能超过 50 MB"));
    }
    if path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("xlsx"))
    {
        let file = File::open(path)?;
        let mut archive = ZipArchive::new(file).map_err(|error| {
            AppError::new("INVALID_EXCEL", format!("Excel 压缩结构损坏：{error}"))
        })?;
        validate_archive_limits(&mut archive, 5_000, 512 * 1024 * 1024)?;
    }
    Ok(())
}

pub fn export_material_package(
    state: &AppState,
    record_ids: &[String],
    destination: &Path,
) -> AppResult<OperationResult> {
    ensure_extension(destination, "zip")?;
    if record_ids.is_empty() {
        return Err(AppError::validation("请至少选择一条记录"));
    }
    let connection = state.connection()?;
    let files = material_files_for_records(&connection, Some(record_ids))?;
    let mut files_by_record: HashMap<String, Vec<_>> = HashMap::new();
    for (record_id, material) in files {
        files_by_record.entry(record_id).or_default().push(material);
    }

    let temp = TempDir::new_in(state.root().join("staging"))?;
    let excel_path = temp.path().join("综测记录.xlsx");
    let mut excel_rows = Vec::new();
    let mut records = Vec::new();
    for id in record_ids {
        let record = db::get_record(state, id)?;
        excel_rows.push(ExcelRecordRow {
            id: Some(record.id.clone()),
            title: record.name.clone(),
            category: record.category_name.clone(),
            level: record.level.as_str().to_string(),
            date: record.date.clone(),
            score: record.score.clone(),
            remark: record.notes.clone(),
            material_count: record.materials.len() as u32,
            material_names: record
                .materials
                .iter()
                .map(|item| item.name.clone())
                .collect(),
        });
        records.push(record);
    }
    let total_cents = excel_rows.iter().try_fold(0_i64, |total, row| {
        parse_score_cents(&row.score).and_then(|value| {
            total
                .checked_add(value)
                .ok_or_else(|| AppError::new("SCORE_OVERFLOW", "导出统计总分过大"))
        })
    })?;
    excel::write_records(
        &excel_path,
        &excel_rows,
        &[
            ("记录数".to_string(), excel_rows.len().to_string()),
            ("总分".to_string(), format_score(total_cents)),
        ],
    )
    .map_err(|error| AppError::new("EXCEL_EXPORT_FAILED", error))?;

    let temporary = temporary_output_path(destination);
    ensure_parent(destination)?;
    let file = File::create(&temporary)?;
    let mut writer = ZipWriter::new(file);
    add_file_to_zip(&mut writer, "综测记录.xlsx", &excel_path, None)?;

    let mut package_records = Vec::new();
    let mut used_folders = HashSet::new();
    for record in &records {
        let base_folder = format!(
            "{}_{}_{}",
            record.date,
            sanitize_component(&record.name, 80),
            record.level.label()
        );
        let folder = unique_name(&base_folder, &mut used_folders);
        let mut used_names = HashSet::new();
        let mut package_materials = Vec::new();
        for material in files_by_record.get(&record.id).into_iter().flatten() {
            let name = unique_name(&sanitize_component(&material.name, 120), &mut used_names);
            let archive_path = format!("记录/{folder}/{name}");
            let source = safe_material_path(&state.root(), &material.relative_path)?;
            if material.size != fs::metadata(&source)?.len() {
                return Err(AppError::new(
                    "MATERIAL_SIZE_MISMATCH",
                    format!("材料大小校验失败：{}", material.name),
                ));
            }
            add_file_to_zip(&mut writer, &archive_path, &source, Some(&material.sha256))?;
            package_materials.push(PackageMaterial {
                id: material.id.clone(),
                name: material.name.clone(),
                path: archive_path,
                sha256: material.sha256.clone(),
                size: material.size,
                mime_type: material.mime_type.clone(),
            });
        }
        package_records.push(PackageRecord {
            id: record.id.clone(),
            name: record.name.clone(),
            date: record.date.clone(),
            level: record.level.as_str().to_string(),
            score: record.score.clone(),
            folder: format!("记录/{folder}"),
            materials: package_materials,
        });
    }
    let manifest = serde_json::to_vec_pretty(&PackageManifest {
        format: "zongce-records-material-package",
        version: 1,
        created_at: now_iso(),
        records: package_records,
    })?;
    add_bytes_to_zip(&mut writer, "manifest.json", &manifest)?;
    finish_zip(writer, &temporary, destination)?;

    Ok(
        OperationResult::success(format!("已导出 {} 条记录的材料包", records.len()))
            .with_affected(records.len())
            .with_path(destination.to_string_lossy()),
    )
}

pub fn export_backup(
    state: &AppState,
    destination: &Path,
    app_version: &str,
) -> AppResult<OperationResult> {
    ensure_extension(destination, "zcbak")?;
    let temp = TempDir::new_in(state.root().join("staging"))?;
    let snapshot = temp.path().join("综测记录.sqlite3");
    let connection = state.connection()?;
    db::create_database_snapshot(&connection, &snapshot)?;
    let record_count: i64 =
        connection.query_row("SELECT COUNT(*) FROM records", [], |row| row.get(0))?;
    let material_count: i64 = connection.query_row(
        "SELECT COUNT(*) FROM materials WHERE EXISTS (
           SELECT 1 FROM record_materials rm WHERE rm.material_id = materials.id
         )",
        [],
        |row| row.get(0),
    )?;
    let material_rows = material_files_for_records(&connection, None)?;
    drop(connection);

    let database = ManifestFile {
        path: "data/综测记录.sqlite3".to_string(),
        sha256: hash_file(&snapshot)?,
        size: fs::metadata(&snapshot)?.len(),
    };
    let mut materials = Vec::new();
    let mut seen = HashSet::new();
    for (_, material) in material_rows {
        let relative = normalize_relative_path(&material.relative_path)?;
        if !seen.insert(relative.clone()) {
            continue;
        }
        let source = safe_material_path(&state.root(), &relative)?;
        let actual_hash = hash_file(&source)?;
        if actual_hash != material.sha256 {
            return Err(AppError::new(
                "HASH_MISMATCH",
                format!("材料哈希校验失败：{}", material.name),
            ));
        }
        let size = fs::metadata(&source)?.len();
        materials.push(ManifestFile {
            path: format!("data/materials/{relative}"),
            sha256: actual_hash,
            size,
        });
    }
    materials.sort_by(|left, right| left.path.cmp(&right.path));
    let manifest = BackupManifest {
        format: BACKUP_FORMAT.to_string(),
        schema_version: BACKUP_SCHEMA_VERSION,
        app_version: app_version.to_string(),
        created_at: now_iso(),
        record_count,
        material_count,
        database: database.clone(),
        materials: materials.clone(),
    };

    let temporary = temporary_output_path(destination);
    ensure_parent(destination)?;
    let file = File::create(&temporary)?;
    let mut writer = ZipWriter::new(file);
    add_bytes_to_zip(
        &mut writer,
        "manifest.json",
        &serde_json::to_vec_pretty(&manifest)?,
    )?;
    add_file_to_zip(
        &mut writer,
        &database.path,
        &snapshot,
        Some(&database.sha256),
    )?;
    for entry in &materials {
        let relative = entry
            .path
            .strip_prefix("data/materials/")
            .ok_or_else(|| AppError::new("INVALID_BACKUP", "备份材料路径无效"))?;
        let source = safe_material_path(&state.root(), relative)?;
        add_file_to_zip(&mut writer, &entry.path, &source, Some(&entry.sha256))?;
    }
    finish_zip(writer, &temporary, destination)?;

    Ok(OperationResult::success(format!(
        "完整备份已创建：{record_count} 条记录，{material_count} 份材料"
    ))
    .with_path(destination.to_string_lossy()))
}

pub fn inspect_backup(state: &AppState, path: &Path) -> AppResult<BackupInspection> {
    let source_hash_before = hash_file(path)?;
    // 自定义数据目录失联时仍允许检查备份；暂存区不应依赖业务数据目录。
    let temp = TempDir::new_in(state.root().join("staging")).or_else(|_| TempDir::new())?;
    let manifest = validate_and_extract_backup(path, temp.path())?;
    let source_hash_after = hash_file(path)?;
    if source_hash_before != source_hash_after {
        return Err(AppError::new(
            "BACKUP_CHANGED",
            "备份文件在检查过程中发生变化，请重新选择",
        ));
    }
    let token = format!("{}.{}", Uuid::new_v4(), source_hash_after);
    state
        .backups
        .lock()
        .insert(token.clone(), path.to_path_buf());
    Ok(BackupInspection {
        token,
        file_name: path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("综测记录完整备份.zcbak")
            .to_string(),
        created_at: manifest.created_at,
        app_version: manifest.app_version,
        record_count: manifest.record_count,
        material_count: manifest.material_count,
        total_bytes: fs::metadata(path)?.len(),
        integrity_valid: true,
    })
}

pub fn restore_backup(state: &AppState, token: &str, mode: &str) -> AppResult<OperationResult> {
    let source = state.backups.lock().get(token).cloned().ok_or_else(|| {
        AppError::new(
            "BACKUP_TOKEN_EXPIRED",
            "备份检查结果已失效，请重新选择备份文件",
        )
    })?;
    verify_backup_token(token, &source)?;
    let result = match mode {
        "merge" => restore_backup_merge(state, &source, MergeConflict::KeepLocal),
        "merge_import" => restore_backup_merge(state, &source, MergeConflict::Import),
        "merge_copy" => restore_backup_merge(state, &source, MergeConflict::Copy),
        "replace" => restore_backup_replace(state, &source),
        _ => Err(AppError::validation("备份恢复模式无效")),
    }?;
    state.backups.lock().remove(token);
    Ok(result)
}

fn verify_backup_token(token: &str, source: &Path) -> AppResult<()> {
    let expected_hash = token
        .rsplit_once('.')
        .map(|(_, hash)| hash)
        .ok_or_else(|| {
            AppError::new(
                "BACKUP_TOKEN_INVALID",
                "备份检查令牌无效，请重新选择备份文件",
            )
        })?;
    if !valid_sha256(expected_hash) || hash_file(source)? != expected_hash {
        return Err(AppError::new(
            "BACKUP_CHANGED",
            "备份文件在检查后发生变化，请重新检查并确认",
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
enum MergeConflict {
    KeepLocal,
    Import,
    Copy,
}

fn restore_backup_merge(
    state: &AppState,
    source: &Path,
    conflict: MergeConflict,
) -> AppResult<OperationResult> {
    create_daily_recovery(state)?;
    let temp = TempDir::new_in(state.root().join("staging"))?;
    validate_and_extract_backup(source, temp.path())?;
    let imported = Connection::open(database_path(temp.path()))?;
    let mut local = state.connection()?;
    let transaction = local.transaction()?;

    let mut category_map = HashMap::new();
    {
        let mut statement = imported.prepare(
            "SELECT id, name, is_active, is_builtin, sort_order, created_at, updated_at
             FROM categories ORDER BY sort_order, created_at",
        )?;
        let categories = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
            ))
        })?;
        for category in categories {
            let (id, name, is_active, is_builtin, sort_order, created_at, updated_at) = category?;
            let local_by_id: Option<String> = transaction
                .query_row("SELECT id FROM categories WHERE id = ?1", [&id], |row| {
                    row.get(0)
                })
                .optional()?;
            let local_id = if let Some(local_id) = local_by_id {
                local_id
            } else if let Some(local_id) = transaction
                .query_row(
                    "SELECT id FROM categories WHERE name = ?1 COLLATE NOCASE",
                    [&name],
                    |row| row.get(0),
                )
                .optional()?
            {
                local_id
            } else {
                transaction.execute(
                    "INSERT INTO categories
                     (id, name, is_active, is_builtin, sort_order, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![id, name, is_active, is_builtin, sort_order, created_at, updated_at],
                )?;
                id.clone()
            };
            category_map.insert(id, local_id);
        }
    }

    // 源记录 ID 到本地记录 ID 的映射同时覆盖新增、导入覆盖和副本模式。
    let mut record_id_map: HashMap<String, String> = HashMap::new();
    {
        let mut statement = imported.prepare(
            "SELECT id, revision, name, category_id, level, activity_date, score_cents, notes,
                    deleted_at, purge_at, created_at, updated_at
             FROM records ORDER BY created_at",
        )?;
        let records = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, i64>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, Option<String>>(8)?,
                row.get::<_, Option<String>>(9)?,
                row.get::<_, String>(10)?,
                row.get::<_, String>(11)?,
            ))
        })?;
        for record in records {
            let (
                id,
                revision,
                name,
                category_id,
                level,
                activity_date,
                score_cents,
                notes,
                deleted_at,
                purge_at,
                created_at,
                updated_at,
            ) = record?;
            let exists: bool = transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM records WHERE id = ?1)",
                [&id],
                |row| row.get(0),
            )?;
            let mapped_category = category_map
                .get(&category_id)
                .ok_or_else(|| AppError::new("INVALID_BACKUP", "备份记录引用了不存在的类别"))?;
            let target_id = if exists {
                match conflict {
                    MergeConflict::KeepLocal => continue,
                    MergeConflict::Import => {
                        transaction.execute(
                            "UPDATE records SET revision = revision + 1, name = ?2,
                             category_id = ?3, level = ?4, activity_date = ?5,
                             score_cents = ?6, notes = ?7, deleted_at = ?8,
                             purge_at = ?9, updated_at = ?10 WHERE id = ?1",
                            params![
                                id,
                                name,
                                mapped_category,
                                level,
                                activity_date,
                                score_cents,
                                notes,
                                deleted_at,
                                purge_at,
                                updated_at
                            ],
                        )?;
                        transaction
                            .execute("DELETE FROM record_materials WHERE record_id = ?1", [&id])?;
                        id.clone()
                    }
                    MergeConflict::Copy => Uuid::new_v4().to_string(),
                }
            } else {
                id.clone()
            };
            if !exists || matches!(conflict, MergeConflict::Copy) {
                transaction.execute(
                    "INSERT INTO records
                     (id, revision, name, category_id, level, activity_date, score_cents, notes,
                      deleted_at, purge_at, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                    params![
                        &target_id,
                        revision.max(1),
                        name,
                        mapped_category,
                        level,
                        activity_date,
                        score_cents,
                        notes,
                        deleted_at,
                        purge_at,
                        created_at,
                        updated_at
                    ],
                )?;
            }
            record_id_map.insert(id, target_id);
        }
    }

    let mut material_id_map: HashMap<String, String> = HashMap::new();
    let mut relation_statement = imported.prepare(
        "SELECT rm.record_id, rm.material_id, rm.sort_order, m.sha256, m.original_name,
                m.mime_type, m.size_bytes, m.stored_rel_path, m.created_at
         FROM record_materials rm JOIN materials m ON m.id = rm.material_id
         ORDER BY rm.record_id, rm.sort_order",
    )?;
    let relations = relation_statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, i64>(6)?,
            row.get::<_, String>(7)?,
            row.get::<_, String>(8)?,
        ))
    })?;
    for relation in relations {
        let (
            record_id,
            source_material_id,
            sort_order,
            sha256,
            original_name,
            mime_type,
            size_bytes,
            relative_path,
            created_at,
        ) = relation?;
        let Some(target_record_id) = record_id_map.get(&record_id) else {
            continue;
        };
        let local_material_id = if let Some(id) = material_id_map.get(&source_material_id) {
            id.clone()
        } else {
            let normalized = normalize_relative_path(&relative_path)?;
            let source_file = safe_material_path(temp.path(), &normalized)?;
            let local_material: Option<(String, String, i64)> = transaction
                .query_row(
                    "SELECT sha256, stored_rel_path, size_bytes FROM materials WHERE id = ?1",
                    [&source_material_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .optional()?;
            let target_id = match &local_material {
                Some((existing_hash, _, _)) if existing_hash == &sha256 => {
                    let canonical_relative = format!("{}/{}", &sha256[..2], sha256);
                    let conflicting_path: bool = transaction.query_row(
                        "SELECT EXISTS(
                           SELECT 1 FROM materials
                           WHERE stored_rel_path = ?1 AND sha256 <> ?2
                         )",
                        params![canonical_relative, sha256],
                        |row| row.get(0),
                    )?;
                    if conflicting_path {
                        return Err(AppError::new(
                            "HASH_CONFLICT",
                            "本地资料库存在同内容路径但不同哈希的材料元数据",
                        ));
                    }
                    repair_material_from_backup(
                        &source_file,
                        &state.root(),
                        &canonical_relative,
                        &sha256,
                        size_bytes.max(0) as u64,
                    )?;
                    transaction.execute(
                        "UPDATE materials SET stored_rel_path = ?2, size_bytes = ?3
                         WHERE id = ?1",
                        params![source_material_id, canonical_relative, size_bytes],
                    )?;
                    source_material_id.clone()
                }
                Some(_) => Uuid::new_v4().to_string(),
                None => source_material_id.clone(),
            };
            if local_material
                .as_ref()
                .is_none_or(|(existing_hash, _, _)| existing_hash != &sha256)
            {
                copy_material_if_missing(&source_file, &state.root(), &normalized, &sha256)?;
                transaction.execute(
                    "INSERT INTO materials
                     (id, sha256, original_name, mime_type, size_bytes, stored_rel_path, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        target_id,
                        sha256,
                        original_name,
                        mime_type,
                        size_bytes,
                        normalized,
                        created_at
                    ],
                )?;
            }
            material_id_map.insert(source_material_id, target_id.clone());
            target_id
        };
        transaction.execute(
            "INSERT INTO record_materials(record_id, material_id, sort_order)
             VALUES (?1, ?2, ?3)",
            params![target_record_id, local_material_id, sort_order],
        )?;
    }
    transaction.commit()?;

    Ok(OperationResult::success(format!(
        "备份合并完成，新增 {} 条记录；同 ID 记录保留本地版本",
        record_id_map.len()
    ))
    .with_affected(record_id_map.len()))
}

fn restore_backup_replace(state: &AppState, source: &Path) -> AppResult<OperationResult> {
    let root = state.root();
    let parent = root
        .parent()
        .ok_or_else(|| AppError::new("INVALID_PATH", "当前数据目录没有可用的父目录"))?;
    let root_name = root
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| AppError::new("INVALID_PATH", "当前数据目录名称无效"))?;
    let staging = parent.join(format!(".{root_name}.恢复暂存_{}", Uuid::new_v4()));
    fs::create_dir(&staging)?;
    if let Err(error) = validate_and_extract_backup(source, &staging) {
        let _ = fs::remove_dir_all(&staging);
        return Err(error);
    }
    fs::create_dir_all(staging.join("recovery"))?;
    fs::create_dir_all(staging.join("staging"))?;

    let rollback = parent.join(format!(
        "{root_name}_恢复前_{}",
        Utc::now().format("%Y%m%d_%H%M%S")
    ));
    fs::rename(&root, &rollback).map_err(|error| AppError::io("无法暂存当前数据目录", error))?;
    if let Err(error) = fs::rename(&staging, &root) {
        let rollback_error = fs::rename(&rollback, &root).err();
        let message = match rollback_error {
            Some(rollback_error) => format!(
                "无法启用恢复数据：{error}；自动回滚也失败：{rollback_error}。原数据位于 {}",
                rollback.display()
            ),
            None => format!("无法启用恢复数据，已恢复原数据：{error}"),
        };
        return Err(AppError::new("RESTORE_SWITCH_FAILED", message));
    }

    Ok(
        OperationResult::success("备份已替换恢复；恢复前数据已完整保留")
            .with_path(rollback.to_string_lossy()),
    )
}

fn validate_and_extract_backup(path: &Path, destination: &Path) -> AppResult<BackupManifest> {
    let metadata = fs::metadata(path)
        .map_err(|error| AppError::io(&format!("无法读取备份 {}", path.display()), error))?;
    if !metadata.is_file() {
        return Err(AppError::validation("请选择有效的 .zcbak 备份文件"));
    }
    let file = File::open(path)?;
    let mut archive = ZipArchive::new(file)
        .map_err(|error| AppError::new("INVALID_BACKUP", format!("备份压缩结构损坏：{error}")))?;
    validate_archive_limits(&mut archive, MAX_ARCHIVE_ENTRIES, MAX_ARCHIVE_UNCOMPRESSED)?;
    validate_archive_names(&mut archive)?;

    let manifest = read_backup_manifest(&mut archive)?;
    validate_manifest(&manifest)?;
    let mut expected = HashMap::new();
    expected.insert(manifest.database.path.clone(), manifest.database.clone());
    for entry in &manifest.materials {
        if expected.insert(entry.path.clone(), entry.clone()).is_some() {
            return Err(AppError::new("INVALID_BACKUP", "备份清单包含重复文件路径"));
        }
    }
    let present: HashSet<String> = (0..archive.len())
        .filter_map(|index| {
            archive
                .by_index(index)
                .ok()
                .map(|entry| entry.name().to_string())
        })
        .collect();
    for name in expected.keys() {
        if !present.contains(name) {
            return Err(AppError::new(
                "BACKUP_FILE_MISSING",
                format!("备份缺少清单文件：{name}"),
            ));
        }
    }
    for name in &present {
        if name != "manifest.json" && !expected.contains_key(name) && !name.ends_with('/') {
            return Err(AppError::new(
                "INVALID_BACKUP",
                format!("备份包含清单外文件：{name}"),
            ));
        }
    }

    fs::create_dir_all(destination)?;
    let expected_total = expected.values().try_fold(0_u64, |total, entry| {
        total
            .checked_add(entry.size)
            .ok_or_else(|| AppError::new("ARCHIVE_TOO_LARGE", "备份清单总大小溢出"))
    })?;
    ensure_extraction_capacity(destination, expected_total)?;
    let mut extracted_total = 0_u64;
    for entry in expected.values() {
        let output_relative = entry
            .path
            .strip_prefix("data/")
            .ok_or_else(|| AppError::new("INVALID_BACKUP", "备份文件路径必须位于 data 目录"))?;
        let output = safe_join(destination, output_relative)?;
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut zipped = archive.by_name(&entry.path).map_err(|error| {
            AppError::new(
                "BACKUP_FILE_MISSING",
                format!("无法读取 {}：{error}", entry.path),
            )
        })?;
        if zipped.size() != entry.size {
            return Err(AppError::new(
                "BACKUP_SIZE_MISMATCH",
                format!("备份文件大小不匹配：{}", entry.path),
            ));
        }
        let mut output_file = File::create(&output)?;
        let (copied, actual_hash) = copy_stream_with_limits(
            &mut zipped,
            &mut output_file,
            entry.size,
            &mut extracted_total,
            MAX_ARCHIVE_UNCOMPRESSED,
        )?;
        output_file.sync_all()?;
        if copied != entry.size || actual_hash != entry.sha256 {
            return Err(AppError::new(
                "BACKUP_HASH_MISMATCH",
                format!("备份文件哈希校验失败：{}", entry.path),
            ));
        }
    }

    verify_extracted_files(destination, expected.values())?;
    validate_extracted_database(destination, &manifest)?;
    Ok(manifest)
}

fn ensure_extraction_capacity(destination: &Path, expected_bytes: u64) -> AppResult<()> {
    let available = fs2::available_space(destination)
        .map_err(|error| AppError::io("无法检查备份暂存空间", error))?;
    validate_extraction_space(expected_bytes, available)
}

fn validate_extraction_space(expected_bytes: u64, available: u64) -> AppResult<()> {
    if expected_bytes > MAX_ARCHIVE_UNCOMPRESSED {
        return Err(AppError::new(
            "ARCHIVE_TOO_LARGE",
            "备份解压后大小超过 8 GiB 安全限制",
        ));
    }
    let required = expected_bytes
        .checked_add(MIN_EXTRACTION_FREE_SPACE)
        .ok_or_else(|| AppError::new("ARCHIVE_TOO_LARGE", "备份空间预算溢出"))?;
    if available < required {
        return Err(AppError::new(
            "INSUFFICIENT_SPACE",
            format!("备份解压至少需要 {required} 字节可用空间，当前仅有 {available} 字节"),
        ));
    }
    Ok(())
}

fn copy_stream_with_limits<R: Read, W: Write>(
    reader: &mut R,
    writer: &mut W,
    expected_size: u64,
    extracted_total: &mut u64,
    max_total: u64,
) -> AppResult<(u64, String)> {
    let mut hasher = Sha256::new();
    let mut copied = 0_u64;
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        copied = copied
            .checked_add(read as u64)
            .ok_or_else(|| AppError::new("ARCHIVE_TOO_LARGE", "解压字节计数溢出"))?;
        *extracted_total = extracted_total
            .checked_add(read as u64)
            .ok_or_else(|| AppError::new("ARCHIVE_TOO_LARGE", "解压总量计数溢出"))?;
        if copied > expected_size {
            return Err(AppError::new(
                "BACKUP_SIZE_MISMATCH",
                "备份文件实际解压量超出清单声明",
            ));
        }
        if *extracted_total > max_total {
            return Err(AppError::new(
                "ARCHIVE_TOO_LARGE",
                "备份实际解压量超过安全限制",
            ));
        }
        hasher.update(&buffer[..read]);
        writer.write_all(&buffer[..read])?;
    }
    Ok((copied, hex::encode(hasher.finalize())))
}

fn verify_extracted_files<'a>(
    destination: &Path,
    entries: impl IntoIterator<Item = &'a ManifestFile>,
) -> AppResult<()> {
    for entry in entries {
        let relative = entry
            .path
            .strip_prefix("data/")
            .ok_or_else(|| AppError::new("INVALID_BACKUP", "备份文件路径必须位于 data 目录"))?;
        let path = safe_join(destination, relative)?;
        let metadata = fs::metadata(&path)?;
        if !metadata.is_file() || metadata.len() != entry.size || hash_file(&path)? != entry.sha256
        {
            return Err(AppError::new(
                "BACKUP_HASH_MISMATCH",
                format!("备份最终落盘校验失败：{}", entry.path),
            ));
        }
    }
    Ok(())
}

fn read_backup_manifest<R: Read + Seek>(archive: &mut ZipArchive<R>) -> AppResult<BackupManifest> {
    let mut entry = archive
        .by_name("manifest.json")
        .map_err(|_| AppError::new("INVALID_BACKUP", "备份缺少 manifest.json 清单"))?;
    if entry.size() > MAX_MANIFEST_BYTES {
        return Err(AppError::new("INVALID_BACKUP", "备份清单文件过大"));
    }
    let mut bytes = Vec::with_capacity(entry.size() as usize);
    entry
        .by_ref()
        .take(MAX_MANIFEST_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_MANIFEST_BYTES {
        return Err(AppError::new("INVALID_BACKUP", "备份清单文件过大"));
    }
    serde_json::from_slice(&bytes)
        .map_err(|error| AppError::new("INVALID_BACKUP", format!("备份清单无效：{error}")))
}

fn validate_manifest(manifest: &BackupManifest) -> AppResult<()> {
    if manifest.format != BACKUP_FORMAT {
        return Err(AppError::new("INVALID_BACKUP", "不是综测记录完整备份"));
    }
    if manifest.schema_version > BACKUP_SCHEMA_VERSION {
        return Err(AppError::new(
            "BACKUP_TOO_NEW",
            format!(
                "备份格式版本 {} 高于当前支持的版本 {}",
                manifest.schema_version, BACKUP_SCHEMA_VERSION
            ),
        ));
    }
    if manifest.schema_version == 0 {
        return Err(AppError::new("INVALID_BACKUP", "备份格式版本无效"));
    }
    if manifest.database.path != "data/综测记录.sqlite3"
        || manifest.database.size > MAX_DATABASE_BYTES
        || !valid_sha256(&manifest.database.sha256)
        || manifest.database.sha256 != manifest.database.sha256.to_ascii_lowercase()
    {
        return Err(AppError::new("INVALID_BACKUP", "备份数据库清单无效"));
    }
    if manifest.materials.len() > MAX_ARCHIVE_ENTRIES.saturating_sub(2) {
        return Err(AppError::new("INVALID_BACKUP", "备份材料条目过多"));
    }
    let mut path_keys = HashSet::new();
    path_keys.insert(windows_path_key(&manifest.database.path)?);
    for material in &manifest.materials {
        if material.size > 200 * 1024 * 1024
            || !valid_sha256(&material.sha256)
            || material.sha256 != material.sha256.to_ascii_lowercase()
        {
            return Err(AppError::new("INVALID_BACKUP", "备份材料清单无效"));
        }
        let relative = material
            .path
            .strip_prefix("data/materials/")
            .ok_or_else(|| AppError::new("INVALID_BACKUP", "备份材料路径无效"))?;
        let normalized = normalize_relative_path(relative)?;
        let expected_relative = format!("{}/{}", &material.sha256[..2], material.sha256);
        if normalized != relative || normalized != expected_relative {
            return Err(AppError::new(
                "INVALID_BACKUP",
                "备份材料未使用标准内容寻址路径",
            ));
        }
        if !path_keys.insert(windows_path_key(&material.path)?) {
            return Err(AppError::new(
                "INVALID_BACKUP",
                format!("备份清单包含 Windows 路径别名冲突：{}", material.path),
            ));
        }
    }
    Ok(())
}

fn validate_archive_limits<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    max_entries: usize,
    max_uncompressed: u64,
) -> AppResult<()> {
    if archive.len() > max_entries {
        return Err(AppError::new(
            "ARCHIVE_TOO_LARGE",
            format!("压缩包条目不能超过 {max_entries} 个"),
        ));
    }
    let mut total = 0_u64;
    for index in 0..archive.len() {
        let entry = archive.by_index(index).map_err(|error| {
            AppError::new("INVALID_ARCHIVE", format!("无法读取压缩包目录：{error}"))
        })?;
        if entry.encrypted() {
            return Err(AppError::new("ENCRYPTED_ARCHIVE", "不支持加密压缩包"));
        }
        total = total
            .checked_add(entry.size())
            .ok_or_else(|| AppError::new("ARCHIVE_TOO_LARGE", "压缩包解压后大小溢出"))?;
        if total > max_uncompressed {
            return Err(AppError::new(
                "ARCHIVE_TOO_LARGE",
                "压缩包解压后大小超过安全限制",
            ));
        }
        if entry.size() > 1024 * 1024 {
            let compressed = entry.compressed_size().max(1);
            if entry.size() > compressed.saturating_mul(MAX_COMPRESSION_RATIO) {
                return Err(AppError::new(
                    "SUSPICIOUS_COMPRESSION",
                    format!("压缩包条目压缩比超过 {MAX_COMPRESSION_RATIO}:1，已拒绝读取"),
                ));
            }
        }
    }
    Ok(())
}

fn validate_archive_names<R: Read + Seek>(archive: &mut ZipArchive<R>) -> AppResult<()> {
    let mut names = HashSet::new();
    for index in 0..archive.len() {
        let entry = archive.by_index(index).map_err(|error| {
            AppError::new("INVALID_BACKUP", format!("无法读取备份目录：{error}"))
        })?;
        let name = entry.name();
        let unix_type = entry.unix_mode().map(|mode| mode & 0o170000);
        if name.contains('\\')
            || entry.enclosed_name().is_none()
            || Path::new(name).is_absolute()
            || unix_type.is_some_and(|kind| kind != 0 && kind != 0o040000 && kind != 0o100000)
        {
            return Err(AppError::new(
                "UNSAFE_ARCHIVE_PATH",
                format!("备份包含不安全路径：{name}"),
            ));
        }
        let comparison_key = windows_archive_path_key(name)?;
        if !names.insert(comparison_key) {
            return Err(AppError::new(
                "INVALID_BACKUP",
                format!("备份包含重复或 Windows 别名路径：{name}"),
            ));
        }
    }
    Ok(())
}

fn validate_extracted_database(root: &Path, manifest: &BackupManifest) -> AppResult<()> {
    let path = database_path(root);
    let connection = Connection::open(&path)?;
    if !integrity_check(&connection)? {
        return Err(AppError::new("BACKUP_INVALID", "备份数据库完整性检查失败"));
    }
    let user_version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if user_version > i64::from(BACKUP_SCHEMA_VERSION) {
        return Err(AppError::new(
            "BACKUP_TOO_NEW",
            format!("备份数据库版本 {user_version} 高于当前软件支持的版本"),
        ));
    }
    if user_version <= 0 {
        return Err(AppError::new("BACKUP_INVALID", "备份数据库版本无效"));
    }
    let foreign_key_error: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_foreign_key_check)",
        [],
        |row| row.get(0),
    )?;
    if foreign_key_error {
        return Err(AppError::new("BACKUP_INVALID", "备份数据库外键校验失败"));
    }
    let record_count: i64 =
        connection.query_row("SELECT COUNT(*) FROM records", [], |row| row.get(0))?;
    let material_count: i64 = connection.query_row(
        "SELECT COUNT(*) FROM materials WHERE EXISTS (
           SELECT 1 FROM record_materials rm WHERE rm.material_id = materials.id
         )",
        [],
        |row| row.get(0),
    )?;
    if record_count != manifest.record_count || material_count != manifest.material_count {
        return Err(AppError::new(
            "BACKUP_COUNT_MISMATCH",
            "备份清单与数据库记录数量不一致",
        ));
    }
    let mut expected_materials: HashMap<_, _> = manifest
        .materials
        .iter()
        .map(|entry| (entry.path.clone(), (entry.sha256.clone(), entry.size)))
        .collect();
    let mut statement = connection.prepare(
        "SELECT DISTINCT m.stored_rel_path, m.sha256, m.size_bytes FROM materials m
         JOIN record_materials rm ON rm.material_id = m.id",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?,
        ))
    })?;
    for row in rows {
        let (stored_relative, database_hash, database_size) = row?;
        let relative = normalize_relative_path(&stored_relative)?;
        if relative != stored_relative
            || !valid_sha256(&database_hash)
            || database_hash != database_hash.to_ascii_lowercase()
            || database_size < 0
            || relative != format!("{}/{}", &database_hash[..2], database_hash)
        {
            return Err(AppError::new(
                "BACKUP_METADATA_MISMATCH",
                "备份数据库包含非标准材料元数据",
            ));
        }
        let archive_path = format!("data/materials/{relative}");
        let Some((manifest_hash, manifest_size)) = expected_materials.remove(&archive_path) else {
            return Err(AppError::new(
                "BACKUP_FILE_MISSING",
                format!("备份清单缺少或重复声明数据库引用的材料：{relative}"),
            ));
        };
        if manifest_hash != database_hash || manifest_size != database_size as u64 {
            return Err(AppError::new(
                "BACKUP_METADATA_MISMATCH",
                format!("备份清单与数据库材料元数据不一致：{relative}"),
            ));
        }
    }
    if !expected_materials.is_empty() {
        return Err(AppError::new(
            "BACKUP_ORPHAN_MATERIAL",
            "备份清单包含数据库未引用的孤立材料",
        ));
    }
    let database_orphan: bool = connection.query_row(
        "SELECT EXISTS(
           SELECT 1 FROM materials m
           WHERE NOT EXISTS (
             SELECT 1 FROM record_materials rm WHERE rm.material_id = m.id
           )
         )",
        [],
        |row| row.get(0),
    )?;
    if database_orphan {
        return Err(AppError::new(
            "BACKUP_ORPHAN_MATERIAL",
            "备份数据库包含未被记录引用的材料元数据",
        ));
    }
    Ok(())
}

fn add_file_to_zip<W: Write + Seek>(
    writer: &mut ZipWriter<W>,
    archive_path: &str,
    source: &Path,
    expected_hash: Option<&str>,
) -> AppResult<()> {
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o600);
    writer
        .start_file(archive_path, options)
        .map_err(zip_error)?;
    let mut file = File::open(source)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        writer.write_all(&buffer[..read])?;
    }
    if let Some(expected) = expected_hash {
        let actual = hex::encode(hasher.finalize());
        if actual != expected {
            return Err(AppError::new(
                "HASH_MISMATCH",
                format!("导出时文件哈希发生变化：{}", source.display()),
            ));
        }
    }
    Ok(())
}

fn add_bytes_to_zip<W: Write + Seek>(
    writer: &mut ZipWriter<W>,
    archive_path: &str,
    bytes: &[u8],
) -> AppResult<()> {
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o600);
    writer
        .start_file(archive_path, options)
        .map_err(zip_error)?;
    writer.write_all(bytes)?;
    Ok(())
}

fn finish_zip(writer: ZipWriter<File>, temporary: &Path, destination: &Path) -> AppResult<()> {
    let file = writer.finish().map_err(zip_error)?;
    file.sync_all()?;
    if destination.exists() {
        fs::remove_file(destination)?;
    }
    fs::rename(temporary, destination)?;
    Ok(())
}

fn copy_material_if_missing(
    source: &Path,
    local_root: &Path,
    relative: &str,
    expected_hash: &str,
) -> AppResult<()> {
    let destination = safe_material_path(local_root, relative)?;
    if destination.exists() {
        if hash_file(&destination)? != expected_hash {
            return Err(AppError::new(
                "HASH_CONFLICT",
                format!("本地资料库存在同路径不同内容文件：{relative}"),
            ));
        }
        return Ok(());
    }
    let parent = destination
        .parent()
        .ok_or_else(|| AppError::new("INVALID_PATH", "材料目标路径无效"))?;
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(".{}.part", Uuid::new_v4()));
    fs::copy(source, &temporary)?;
    if hash_file(&temporary)? != expected_hash {
        let _ = fs::remove_file(&temporary);
        return Err(AppError::new("HASH_MISMATCH", "恢复材料哈希校验失败"));
    }
    fs::rename(&temporary, destination)?;
    Ok(())
}

fn repair_material_from_backup(
    source: &Path,
    local_root: &Path,
    relative: &str,
    expected_hash: &str,
    expected_size: u64,
) -> AppResult<()> {
    let source_metadata = fs::metadata(source)?;
    if !source_metadata.is_file()
        || source_metadata.len() != expected_size
        || hash_file(source)? != expected_hash
    {
        return Err(AppError::new(
            "BACKUP_HASH_MISMATCH",
            "用于修复的备份材料未通过完整性校验",
        ));
    }

    let destination = safe_material_path(local_root, relative)?;
    if destination.is_file()
        && fs::metadata(&destination)?.len() == expected_size
        && hash_file(&destination)? == expected_hash
    {
        return Ok(());
    }
    let parent = destination
        .parent()
        .ok_or_else(|| AppError::new("INVALID_PATH", "材料修复目标路径无效"))?;
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(".{}.repair", Uuid::new_v4()));
    fs::copy(source, &temporary)?;
    // Windows 上只读句柄调用 sync_all 可能返回 ERROR_ACCESS_DENIED。
    let copied = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&temporary)?;
    copied.sync_all()?;
    if fs::metadata(&temporary)?.len() != expected_size || hash_file(&temporary)? != expected_hash {
        let _ = fs::remove_file(&temporary);
        return Err(AppError::new("HASH_MISMATCH", "恢复材料落盘校验失败"));
    }
    if destination.exists() {
        fs::remove_file(&destination)?;
    }
    fs::rename(&temporary, destination)?;
    Ok(())
}

fn ensure_extension(path: &Path, expected: &str) -> AppResult<()> {
    if !path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case(expected))
    {
        return Err(AppError::validation(format!(
            "文件扩展名必须为 .{expected}"
        )));
    }
    Ok(())
}

fn ensure_parent(path: &Path) -> AppResult<()> {
    let parent = path
        .parent()
        .ok_or_else(|| AppError::new("INVALID_PATH", "导出路径无效"))?;
    fs::create_dir_all(parent)?;
    Ok(())
}

fn temporary_output_path(destination: &Path) -> PathBuf {
    let file_name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("综测记录导出");
    destination.with_file_name(format!(".{file_name}.{}.part", Uuid::new_v4()))
}

fn normalize_relative_path(value: &str) -> AppResult<String> {
    let normalized = value.replace('\\', "/");
    windows_path_key(&normalized)?;
    Ok(normalized)
}

fn windows_archive_path_key(value: &str) -> AppResult<String> {
    let without_directory_marker = value.strip_suffix('/').unwrap_or(value);
    if without_directory_marker.is_empty() || without_directory_marker.ends_with('/') {
        return Err(AppError::new("UNSAFE_ARCHIVE_PATH", "压缩包路径无效"));
    }
    windows_path_key(without_directory_marker)
}

fn windows_path_key(value: &str) -> AppResult<String> {
    if value.is_empty() || value.starts_with('/') || value.contains('\\') {
        return Err(AppError::new("UNSAFE_ARCHIVE_PATH", "相对路径无效"));
    }
    let mut result = Vec::new();
    for component in value.split('/') {
        if component.is_empty()
            || component == "."
            || component == ".."
            || component.ends_with(['.', ' '])
            || component.chars().any(|character| {
                character.is_control()
                    || matches!(character, '<' | '>' | ':' | '"' | '|' | '?' | '*')
            })
            || is_windows_reserved_name(component)
        {
            return Err(AppError::new(
                "UNSAFE_ARCHIVE_PATH",
                format!("路径包含 Windows 不安全名称：{component}"),
            ));
        }
        result.push(component.to_lowercase());
    }
    Ok(result.join("/"))
}

fn is_windows_reserved_name(value: &str) -> bool {
    let base = value
        .trim_end_matches(['.', ' '])
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    matches!(
        base.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$" | "CONIN$" | "CONOUT$"
    ) || matches!(
        base.as_str(),
        "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
            | "COM¹"
            | "COM²"
            | "COM³"
            | "LPT¹"
            | "LPT²"
            | "LPT³"
    )
}

fn safe_join(root: &Path, relative: &str) -> AppResult<PathBuf> {
    let normalized = normalize_relative_path(relative)?;
    let joined = normalized
        .split('/')
        .fold(root.to_path_buf(), |path, part| path.join(part));
    if !joined.starts_with(root) {
        return Err(AppError::new("UNSAFE_ARCHIVE_PATH", "压缩包路径越界"));
    }
    Ok(joined)
}

fn sanitize_component(value: &str, max_chars: usize) -> String {
    let mut result: String = value
        .chars()
        .map(|character| {
            if character.is_control()
                || matches!(
                    character,
                    '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
                )
            {
                '_'
            } else {
                character
            }
        })
        .take(max_chars)
        .collect();
    result = result.trim().trim_matches(['.', ' ']).to_string();
    if result.is_empty() {
        result = "未命名".to_string();
    }
    if is_windows_reserved_name(&result) {
        let shortened: String = result.chars().take(max_chars.saturating_sub(1)).collect();
        result = format!("_{shortened}");
    }
    result
}

fn unique_name(base: &str, used: &mut HashSet<String>) -> String {
    if used.insert(base.to_lowercase()) {
        return base.to_string();
    }
    let path = Path::new(base);
    let stem = path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or(base);
    let extension = path.extension().and_then(|name| name.to_str());
    for index in 2..=10_000 {
        let candidate = match extension {
            Some(extension) => format!("{stem} ({index}).{extension}"),
            None => format!("{stem} ({index})"),
        };
        if used.insert(candidate.to_lowercase()) {
            return candidate;
        }
    }
    format!("{}_{}", stem, Uuid::new_v4())
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn zip_error(error: zip::result::ZipError) -> AppError {
    AppError::new("ZIP_ERROR", format!("压缩包操作失败：{error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{PendingMaterial, RecordDraft};
    use std::io::Cursor;

    fn test_state(root: PathBuf) -> AppState {
        AppState::new_for_test(root).expect("创建测试数据目录")
    }

    fn add_record(state: &AppState, source: &Path, name: &str) -> String {
        let id = Uuid::new_v4().to_string();
        let size = fs::metadata(source).expect("材料元数据").len();
        db::save_record(
            state,
            RecordDraft {
                id: id.clone(),
                revision: 0,
                name: name.to_string(),
                category_id: "00000000-0000-4000-8000-000000000001".to_string(),
                level: AssessmentLevel::Provincial,
                date: "2026-08-31".to_string(),
                score: "8.25".to_string(),
                notes: "测试记录".to_string(),
                attachment_ids: Vec::new(),
                new_attachments: vec![PendingMaterial {
                    _client_id: Some(Uuid::new_v4().to_string()),
                    name: "获奖证书.pdf".to_string(),
                    size,
                    mime_type: "application/pdf".to_string(),
                    path: Some(source.to_string_lossy().into_owned()),
                }],
            },
        )
        .expect("保存测试记录");
        id
    }

    #[test]
    fn archive_paths_reject_traversal() {
        assert!(normalize_relative_path("aa/file.pdf").is_ok());
        assert!(normalize_relative_path("../file.pdf").is_err());
        assert!(normalize_relative_path("aa/../../file.pdf").is_err());
        assert!(normalize_relative_path("C:/file.pdf").is_err());
        assert!(normalize_relative_path("aa\\..\\file.pdf").is_err());
    }

    #[test]
    fn windows_file_names_are_sanitized_and_deduplicated() {
        assert_eq!(sanitize_component("竞赛:证书?.pdf", 120), "竞赛_证书_.pdf");
        assert_eq!(sanitize_component("CON.txt", 120), "_CON.txt");
        let mut used = HashSet::new();
        assert_eq!(unique_name("证明.pdf", &mut used), "证明.pdf");
        assert_eq!(unique_name("证明.PDF", &mut used), "证明 (2).PDF");
        assert_eq!(
            windows_path_key("AA/FILE").unwrap(),
            windows_path_key("aa/file").unwrap()
        );
        assert!(windows_path_key("CON.txt").is_err());
        assert!(windows_path_key("folder/name. ").is_err());
    }

    #[test]
    fn archive_budget_and_compression_ratio_are_bounded() {
        assert_eq!(
            validate_extraction_space(MAX_ARCHIVE_UNCOMPRESSED + 1, u64::MAX)
                .expect_err("应拒绝超过总解压上限")
                .code,
            "ARCHIVE_TOO_LARGE"
        );
        assert_eq!(
            validate_extraction_space(1024, 1024)
                .expect_err("应拒绝没有安全余量的空间")
                .code,
            "INSUFFICIENT_SPACE"
        );

        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .start_file(
                "data/materials/aa/file",
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
            )
            .expect("创建高压缩条目");
        writer.write_all(&vec![0_u8; 2 * 1024 * 1024]).unwrap();
        let bytes = writer.finish().unwrap().into_inner();
        let mut archive = ZipArchive::new(Cursor::new(bytes)).unwrap();
        assert_eq!(
            validate_archive_limits(&mut archive, MAX_ARCHIVE_ENTRIES, MAX_ARCHIVE_UNCOMPRESSED)
                .expect_err("应拒绝危险压缩比")
                .code,
            "SUSPICIOUS_COMPRESSION"
        );
    }

    #[test]
    fn archive_names_reject_windows_aliases_and_device_names() {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .start_file("data/materials/AA/FILE", SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"a").unwrap();
        writer
            .start_file("data/materials/aa/file", SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"b").unwrap();
        let bytes = writer.finish().unwrap().into_inner();
        let mut archive = ZipArchive::new(Cursor::new(bytes)).unwrap();
        assert_eq!(
            validate_archive_names(&mut archive)
                .expect_err("应拒绝 Windows 路径别名")
                .code,
            "INVALID_BACKUP"
        );
        assert!(windows_archive_path_key("data/CON/").is_err());
    }

    #[test]
    fn excel_import_is_transactional_and_idempotent() {
        let temp = tempfile::tempdir().expect("临时目录");
        let state = test_state(temp.path().join("数据"));
        let xlsx = temp.path().join("导入.xlsx");
        let id = Uuid::new_v4().to_string();
        excel::write_records(
            &xlsx,
            &[ExcelRecordRow {
                id: Some(id),
                title: "程序设计竞赛".to_string(),
                category: "学科竞赛".to_string(),
                level: "省级".to_string(),
                date: "2026-08-31".to_string(),
                score: "8.25".to_string(),
                remark: "Excel 导入".to_string(),
                material_count: 0,
                material_names: Vec::new(),
            }],
            &[],
        )
        .expect("生成 Excel");

        let preview = preview_excel(&state, &xlsx).expect("预览 Excel");
        assert_eq!(preview.new_count, 1);
        assert_eq!(preview.error_count, 0);
        let first = commit_excel(&state, &preview.token).expect("提交导入");
        let second = commit_excel(&state, &preview.token).expect("重复提交");
        assert_eq!(first.affected, Some(1));
        assert_eq!(second.affected, Some(1));
        assert_eq!(
            db::list_records(&state, RecordFilter::default())
                .expect("查询记录")
                .total,
            1
        );
    }

    #[test]
    fn excel_duplicate_rows_can_be_imported_with_new_ids() {
        let temp = tempfile::tempdir().expect("临时目录");
        let state = test_state(temp.path().join("数据"));
        let original = temp.path().join("原记录.xlsx");
        let duplicate = temp.path().join("疑似重复.xlsx");
        let make_row = |id: Option<String>| ExcelRecordRow {
            id,
            title: "同名竞赛".to_string(),
            category: "学科竞赛".to_string(),
            level: "省级".to_string(),
            date: "2026-08-31".to_string(),
            score: "8.25".to_string(),
            remark: String::new(),
            material_count: 0,
            material_names: Vec::new(),
        };
        excel::write_records(
            &original,
            &[make_row(Some(Uuid::new_v4().to_string()))],
            &[],
        )
        .unwrap();
        let original_preview = preview_excel(&state, &original).unwrap();
        commit_excel(&state, &original_preview.token).unwrap();

        excel::write_records(&duplicate, &[make_row(None)], &[]).unwrap();
        let duplicate_preview = preview_excel(&state, &duplicate).unwrap();
        assert_eq!(duplicate_preview.duplicate_count, 1);
        let result = commit_excel_with_options(&state, &duplicate_preview.token, true).unwrap();
        assert_eq!(result.affected, Some(1));
        assert_eq!(
            db::list_records(&state, RecordFilter::default())
                .unwrap()
                .total,
            2
        );
    }

    #[test]
    fn excel_preview_and_commit_reject_duplicate_ids() {
        let temp = tempfile::tempdir().expect("临时目录");
        let state = test_state(temp.path().join("数据"));
        let xlsx = temp.path().join("重复 ID.xlsx");
        let id = Uuid::new_v4().to_string();
        let rows = (0..2)
            .map(|index| ExcelRecordRow {
                id: Some(id.clone()),
                title: format!("重复活动 {index}"),
                category: "学科竞赛".to_string(),
                level: "省级".to_string(),
                date: "2026-08-31".to_string(),
                score: "1.00".to_string(),
                remark: String::new(),
                material_count: 0,
                material_names: Vec::new(),
            })
            .collect::<Vec<_>>();
        excel::write_records(&xlsx, &rows, &[]).unwrap();
        let preview = preview_excel(&state, &xlsx).unwrap();
        assert_eq!(preview.error_count, 2);
        assert!(preview.rows.iter().all(|row| row.status == "error"));
        assert_eq!(
            commit_excel(&state, &preview.token)
                .expect_err("提交时应再次拒绝重复 ID")
                .code,
            "DUPLICATE_IMPORT_ID"
        );
        assert_eq!(
            db::list_records(&state, RecordFilter::default())
                .unwrap()
                .total,
            0
        );
    }

    #[test]
    fn backup_round_trip_and_material_package_preserve_files() {
        let temp = tempfile::tempdir().expect("临时目录");
        let source_file = temp.path().join("原始证书.pdf");
        fs::write(&source_file, b"local assessment evidence").expect("写材料");
        let source_state = test_state(temp.path().join("源数据"));
        let record_id = add_record(&source_state, &source_file, "省级程序设计竞赛");

        let package = temp.path().join("材料包.zip");
        export_material_package(&source_state, std::slice::from_ref(&record_id), &package)
            .expect("导出材料包");
        let mut package_zip =
            ZipArchive::new(File::open(&package).expect("打开材料包")).expect("读取材料包");
        assert!(package_zip.by_name("manifest.json").is_ok());
        assert!(package_zip.by_name("综测记录.xlsx").is_ok());

        let backup = temp.path().join("完整备份.zcbak");
        export_backup(&source_state, &backup, "0.1.0").expect("导出备份");
        let target_state = test_state(temp.path().join("目标数据"));
        let inspection = inspect_backup(&target_state, &backup).expect("检查备份");
        assert!(inspection.integrity_valid);
        assert_eq!(inspection.record_count, 1);
        restore_backup(&target_state, &inspection.token, "merge").expect("合并备份");
        let restored = db::get_record(&target_state, &record_id).expect("读取恢复记录");
        assert_eq!(restored.materials.len(), 1);
        let connection = target_state.connection().expect("打开目标数据库");
        let files =
            material_files_for_records(&connection, Some(&[record_id])).expect("读取恢复材料");
        let restored_path = safe_material_path(&target_state.root(), &files[0].1.relative_path)
            .expect("恢复材料路径");
        assert_eq!(
            fs::read(restored_path).expect("读取恢复材料"),
            b"local assessment evidence"
        );
    }

    #[test]
    fn merge_repairs_a_missing_local_material_with_the_verified_backup_copy() {
        let temp = tempfile::tempdir().expect("临时目录");
        let source_file = temp.path().join("来源.pdf");
        fs::write(&source_file, b"repairable evidence").unwrap();
        let source_state = test_state(temp.path().join("源数据"));
        let source_record_id = add_record(&source_state, &source_file, "待修复记录");
        let source_connection = source_state.connection().unwrap();
        let source_material = material_files_for_records(
            &source_connection,
            Some(std::slice::from_ref(&source_record_id)),
        )
        .unwrap()
        .pop()
        .unwrap()
        .1;
        drop(source_connection);

        let backup = temp.path().join("修复来源.zcbak");
        export_backup(&source_state, &backup, "0.1.0").unwrap();

        // 预先放入同一个材料 ID，但故意删除其物理文件，模拟本地资料库损坏。
        let target_state = test_state(temp.path().join("目标数据"));
        let target_connection = target_state.connection().unwrap();
        target_connection
            .execute(
                "INSERT INTO materials
                 (id, sha256, original_name, mime_type, size_bytes, stored_rel_path, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    source_material.id,
                    source_material.sha256,
                    source_material.name,
                    source_material.mime_type,
                    source_material.size as i64,
                    source_material.relative_path,
                    now_iso(),
                ],
            )
            .unwrap();
        drop(target_connection);
        let target_material_path = target_state
            .root()
            .join("materials")
            .join(&source_material.relative_path);
        assert!(!target_material_path.exists());

        let inspection = inspect_backup(&target_state, &backup).unwrap();
        restore_backup(&target_state, &inspection.token, "merge").unwrap();
        assert_eq!(
            fs::read(target_material_path).unwrap(),
            b"repairable evidence"
        );
    }

    #[test]
    fn replace_restore_keeps_a_rollback_directory() {
        let temp = tempfile::tempdir().expect("临时目录");
        let source_file = temp.path().join("证书.pdf");
        fs::write(&source_file, b"evidence").expect("写材料");
        let state = test_state(temp.path().join("数据"));
        add_record(&state, &source_file, "备份内记录");
        let backup = temp.path().join("替换恢复.zcbak");
        export_backup(&state, &backup, "0.1.0").expect("导出备份");
        add_record(&state, &source_file, "备份后记录");
        assert_eq!(
            db::list_records(&state, RecordFilter::default())
                .expect("恢复前查询")
                .total,
            2
        );

        let inspection = inspect_backup(&state, &backup).expect("检查备份");
        let result = restore_backup(&state, &inspection.token, "replace").expect("替换恢复");
        assert!(Path::new(result.path.as_deref().expect("回滚路径")).is_dir());
        assert_eq!(
            db::list_records(&state, RecordFilter::default())
                .expect("恢复后查询")
                .total,
            1
        );
    }

    #[test]
    fn backup_rejects_zip_path_traversal() {
        let temp = tempfile::tempdir().expect("临时目录");
        let state = test_state(temp.path().join("数据"));
        let backup = temp.path().join("恶意备份.zcbak");
        let mut writer = ZipWriter::new(File::create(&backup).expect("创建压缩包"));
        writer
            .start_file("../越界.txt", SimpleFileOptions::default())
            .expect("写路径");
        writer.write_all(b"unsafe").expect("写内容");
        writer.finish().expect("完成压缩包");
        assert_eq!(
            inspect_backup(&state, &backup)
                .expect_err("应拒绝越界路径")
                .code,
            "UNSAFE_ARCHIVE_PATH"
        );
    }

    #[test]
    fn backup_token_is_bound_to_checked_content() {
        let temp = tempfile::tempdir().expect("临时目录");
        let state = test_state(temp.path().join("数据"));
        let backup = temp.path().join("内容绑定.zcbak");
        export_backup(&state, &backup, "0.1.0").unwrap();
        let inspection = inspect_backup(&state, &backup).unwrap();
        fs::OpenOptions::new()
            .append(true)
            .open(&backup)
            .unwrap()
            .write_all(b"changed")
            .unwrap();
        assert_eq!(
            restore_backup(&state, &inspection.token, "merge")
                .expect_err("备份内容改变后应拒绝恢复")
                .code,
            "BACKUP_CHANGED"
        );
    }

    #[test]
    fn inspect_backup_falls_back_to_system_temp_when_data_root_is_missing() {
        let temp = tempfile::tempdir().expect("临时目录");
        let root = temp.path().join("数据");
        let state = test_state(root.clone());
        let backup = temp.path().join("脱离数据目录.zcbak");
        export_backup(&state, &backup, "0.1.0").expect("导出备份");
        fs::remove_dir_all(&root).expect("模拟数据目录失联");
        let inspection = inspect_backup(&state, &backup).expect("失联时仍可检查备份");
        assert!(inspection.integrity_valid);
    }

    #[test]
    fn extracted_database_requires_exact_material_manifest_binding() {
        let temp = tempfile::tempdir().expect("临时目录");
        let source_file = temp.path().join("证书.pdf");
        fs::write(&source_file, b"metadata binding").unwrap();
        let state = test_state(temp.path().join("数据"));
        let record_id = add_record(&state, &source_file, "元数据绑定");
        let connection = state.connection().unwrap();
        let material = material_files_for_records(&connection, Some(&[record_id]))
            .unwrap()
            .pop()
            .unwrap()
            .1;
        let snapshot = temp.path().join("快照.sqlite3");
        db::create_database_snapshot(&connection, &snapshot).unwrap();
        drop(connection);

        let extracted = temp.path().join("提取");
        fs::create_dir_all(
            extracted
                .join("materials")
                .join(&material.relative_path[..2]),
        )
        .unwrap();
        fs::copy(&snapshot, extracted.join("综测记录.sqlite3")).unwrap();
        fs::copy(
            state.root().join("materials").join(&material.relative_path),
            extracted.join("materials").join(&material.relative_path),
        )
        .unwrap();
        let manifest = BackupManifest {
            format: BACKUP_FORMAT.to_string(),
            schema_version: BACKUP_SCHEMA_VERSION,
            app_version: "0.1.0".to_string(),
            created_at: now_iso(),
            record_count: 1,
            material_count: 1,
            database: ManifestFile {
                path: "data/综测记录.sqlite3".to_string(),
                sha256: hash_file(&extracted.join("综测记录.sqlite3")).unwrap(),
                size: fs::metadata(extracted.join("综测记录.sqlite3"))
                    .unwrap()
                    .len(),
            },
            materials: vec![ManifestFile {
                path: format!("data/materials/{}", material.relative_path),
                sha256: material.sha256.clone(),
                size: material.size,
            }],
        };
        validate_extracted_database(&extracted, &manifest).unwrap();

        let orphan_path =
            "data/materials/bb/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        let orphan_relative = orphan_path.strip_prefix("data/materials/").unwrap();
        fs::create_dir_all(extracted.join("materials").join("bb")).unwrap();
        fs::write(extracted.join("materials").join(orphan_relative), b"orphan").unwrap();
        let mut orphan_manifest = manifest.clone();
        orphan_manifest.materials.push(ManifestFile {
            path: orphan_path.to_string(),
            sha256: hex::encode(Sha256::digest(b"orphan")),
            size: 6,
        });
        assert_eq!(
            validate_extracted_database(&extracted, &orphan_manifest)
                .expect_err("数据库未引用的孤立材料应拒绝")
                .code,
            "BACKUP_ORPHAN_MATERIAL"
        );

        let tamper = Connection::open(extracted.join("综测记录.sqlite3")).unwrap();
        tamper
            .execute(
                "UPDATE materials SET sha256 = ?1 WHERE id = (SELECT material_id FROM record_materials LIMIT 1)",
                ["aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"],
            )
            .unwrap();
        drop(tamper);
        assert_eq!(
            validate_extracted_database(&extracted, &manifest)
                .expect_err("数据库材料哈希改变后应拒绝")
                .code,
            "BACKUP_METADATA_MISMATCH"
        );
    }
}
