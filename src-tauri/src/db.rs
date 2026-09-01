use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration as StdDuration;

use chrono::{Duration, Local, NaiveDate, SecondsFormat, Utc};
use directories::ProjectDirs;
use parking_lot::{Mutex, RwLock};
use rusqlite::backup::Backup;
use rusqlite::types::Value;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;
use walkdir::WalkDir;

use crate::error::{AppError, AppResult};
use crate::excel::ExcelRecordRow;
use crate::models::{
    AppInitialization, AssessmentLevel, AssessmentRecord, Category, CategoryDraft, ChartDatum,
    Material, MaterialPreview, MetricSummary, MonthlyDatum, OperationResult, PendingMaterial,
    RecordDraft, RecordFilter, RecordListResult, StatisticsResult, StorageStatus,
};

const DATABASE_FILE: &str = "综测记录.sqlite3";
const MAX_MATERIALS_PER_RECORD: usize = 20;
const MAX_MATERIAL_BYTES: u64 = 200 * 1024 * 1024;
const SCHEMA_VERSION: i64 = 1;

const BUILTIN_CATEGORIES: [(&str, &str); 8] = [
    ("00000000-0000-4000-8000-000000000001", "学科竞赛"),
    ("00000000-0000-4000-8000-000000000002", "科研创新"),
    ("00000000-0000-4000-8000-000000000003", "社会实践"),
    ("00000000-0000-4000-8000-000000000004", "志愿服务"),
    ("00000000-0000-4000-8000-000000000005", "文体活动"),
    ("00000000-0000-4000-8000-000000000006", "学生工作"),
    ("00000000-0000-4000-8000-000000000007", "荣誉表彰"),
    ("00000000-0000-4000-8000-000000000008", "其他"),
];

#[derive(Debug, Clone)]
struct DataLocation {
    root: PathBuf,
    available: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoragePointer {
    root: PathBuf,
}

#[derive(Debug, Clone)]
pub(crate) struct PendingImport {
    pub rows: Vec<ExcelRecordRow>,
    pub row_numbers: Vec<u32>,
    pub statuses: HashMap<u32, String>,
}

pub struct AppState {
    location: RwLock<DataLocation>,
    pointer_file: PathBuf,
    pub(crate) imports: Mutex<HashMap<String, PendingImport>>,
    pub(crate) backups: Mutex<HashMap<String, PathBuf>>,
}

#[derive(Debug, Clone)]
pub(crate) struct MaterialFile {
    pub id: String,
    pub name: String,
    pub mime_type: String,
    pub size: u64,
    pub sha256: String,
    pub relative_path: String,
}

#[derive(Debug)]
struct PreparedMaterial {
    id: String,
    name: String,
    mime_type: String,
    size: u64,
    sha256: String,
    relative_path: String,
}

impl AppState {
    pub fn new() -> AppResult<Self> {
        let project = ProjectDirs::from("com", "GodBook", "综测记录")
            .ok_or_else(|| AppError::new("APP_DIR_UNAVAILABLE", "无法确定应用数据目录"))?;
        let default_root = project.data_local_dir().join("数据");
        let config_dir = project.config_dir();
        fs::create_dir_all(config_dir).map_err(|error| AppError::io("无法创建配置目录", error))?;
        let pointer_file = config_dir.join("存储位置.json");

        let (root, available, is_custom) = if pointer_file.exists() {
            let text = fs::read_to_string(&pointer_file)
                .map_err(|error| AppError::io("无法读取数据位置配置", error))?;
            let pointer: StoragePointer = serde_json::from_str(&text)?;
            let available = pointer.root.is_dir();
            (pointer.root, available, true)
        } else {
            (default_root, true, false)
        };

        if available {
            ensure_data_directories(&root)?;
            let connection = open_database_at(&root)?;
            migrate(&connection)?;
            seed_categories(&connection)?;
        } else if !is_custom {
            return Err(AppError::new("STORAGE_UNAVAILABLE", "默认数据目录不可用"));
        }

        Ok(Self {
            location: RwLock::new(DataLocation { root, available }),
            pointer_file,
            imports: Mutex::new(HashMap::new()),
            backups: Mutex::new(HashMap::new()),
        })
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(root: PathBuf) -> AppResult<Self> {
        ensure_data_directories(&root)?;
        let connection = open_database_at(&root)?;
        migrate(&connection)?;
        seed_categories(&connection)?;
        drop(connection);
        Ok(Self {
            pointer_file: root.with_extension("测试存储位置.json"),
            location: RwLock::new(DataLocation {
                root,
                available: true,
            }),
            imports: Mutex::new(HashMap::new()),
            backups: Mutex::new(HashMap::new()),
        })
    }

    pub(crate) fn root(&self) -> PathBuf {
        self.location.read().root.clone()
    }

    pub(crate) fn is_available(&self) -> bool {
        self.location.read().available
    }

    pub(crate) fn connection(&self) -> AppResult<Connection> {
        let location = self.location.read().clone();
        if !location.available || !location.root.is_dir() {
            return Err(AppError::new(
                "STORAGE_UNAVAILABLE",
                format!("数据位置不可用：{}", location.root.display()),
            ));
        }
        open_database_at(&location.root)
    }

    fn write_pointer(&self, root: &Path) -> AppResult<()> {
        let parent = self
            .pointer_file
            .parent()
            .ok_or_else(|| AppError::new("INVALID_PATH", "存储位置配置路径无效"))?;
        fs::create_dir_all(parent)?;
        let temporary = parent.join(format!("存储位置.{}.tmp", Uuid::new_v4()));
        let bytes = serde_json::to_vec_pretty(&StoragePointer {
            root: root.to_path_buf(),
        })?;
        fs::write(&temporary, bytes)?;
        if self.pointer_file.exists() {
            fs::remove_file(&self.pointer_file)?;
        }
        fs::rename(&temporary, &self.pointer_file)?;
        Ok(())
    }

    fn set_root(&self, root: PathBuf) {
        *self.location.write() = DataLocation {
            root,
            available: true,
        };
    }
}

pub fn initialize_app(state: &AppState, app_version: &str) -> AppResult<AppInitialization> {
    if !state.is_available() {
        return Ok(AppInitialization {
            app_version: app_version.to_string(),
            storage_root: state.root().to_string_lossy().into_owned(),
            database_healthy: false,
            recovery_required: true,
        });
    }

    let connection = state.connection()?;
    let healthy = integrity_check(&connection)?;
    drop(connection);
    let _ = cleanup_expired_records(state);
    let _ = cleanup_content_store(&state.root());

    Ok(AppInitialization {
        app_version: app_version.to_string(),
        storage_root: state.root().to_string_lossy().into_owned(),
        database_healthy: healthy,
        recovery_required: !healthy,
    })
}

fn ensure_data_directories(root: &Path) -> AppResult<()> {
    fs::create_dir_all(root)?;
    fs::create_dir_all(root.join("materials"))?;
    fs::create_dir_all(root.join("recovery"))?;
    fs::create_dir_all(root.join("staging"))?;
    Ok(())
}

pub(crate) fn database_path(root: &Path) -> PathBuf {
    root.join(DATABASE_FILE)
}

pub(crate) fn materials_path(root: &Path) -> PathBuf {
    root.join("materials")
}

pub(crate) fn recovery_path(root: &Path) -> PathBuf {
    root.join("recovery")
}

pub(crate) fn open_database_at(root: &Path) -> AppResult<Connection> {
    let connection = Connection::open(database_path(root))?;
    connection.busy_timeout(StdDuration::from_secs(5))?;
    connection.execute_batch(
        "PRAGMA foreign_keys = ON;
         PRAGMA journal_mode = WAL;
         PRAGMA synchronous = FULL;
         PRAGMA temp_store = MEMORY;",
    )?;
    Ok(connection)
}

pub(crate) fn migrate(connection: &Connection) -> AppResult<()> {
    let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version > SCHEMA_VERSION {
        return Err(AppError::new(
            "SCHEMA_TOO_NEW",
            format!("数据版本 {version} 高于当前软件支持的版本 {SCHEMA_VERSION}"),
        ));
    }
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS meta (
           key TEXT PRIMARY KEY,
           value TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS categories (
           id TEXT PRIMARY KEY,
           name TEXT NOT NULL COLLATE NOCASE UNIQUE,
           is_active INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
           is_builtin INTEGER NOT NULL DEFAULT 0 CHECK (is_builtin IN (0, 1)),
           sort_order INTEGER NOT NULL DEFAULT 0,
           created_at TEXT NOT NULL,
           updated_at TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS records (
           id TEXT PRIMARY KEY,
           revision INTEGER NOT NULL DEFAULT 1 CHECK (revision >= 1),
           name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 200),
           category_id TEXT NOT NULL REFERENCES categories(id) ON UPDATE CASCADE ON DELETE RESTRICT,
           level TEXT NOT NULL CHECK (level IN ('college', 'school', 'provincial', 'national')),
           activity_date TEXT NOT NULL,
           score_cents INTEGER NOT NULL CHECK (score_cents >= 0),
           notes TEXT NOT NULL DEFAULT '',
           deleted_at TEXT,
           purge_at TEXT,
           created_at TEXT NOT NULL,
           updated_at TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS materials (
           id TEXT PRIMARY KEY,
           sha256 TEXT NOT NULL,
           original_name TEXT NOT NULL,
           mime_type TEXT NOT NULL,
           size_bytes INTEGER NOT NULL CHECK (size_bytes >= 0),
           stored_rel_path TEXT NOT NULL,
           created_at TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS record_materials (
           record_id TEXT NOT NULL REFERENCES records(id) ON DELETE CASCADE,
           material_id TEXT NOT NULL REFERENCES materials(id) ON DELETE CASCADE,
           sort_order INTEGER NOT NULL DEFAULT 0,
           PRIMARY KEY (record_id, material_id)
         );
         CREATE TABLE IF NOT EXISTS import_commits (
           token TEXT PRIMARY KEY,
           committed_at TEXT NOT NULL,
           result_json TEXT NOT NULL
         );
         CREATE INDEX IF NOT EXISTS idx_records_active_date ON records(deleted_at, activity_date DESC);
         CREATE INDEX IF NOT EXISTS idx_records_category ON records(category_id, deleted_at);
         CREATE INDEX IF NOT EXISTS idx_records_level ON records(level, deleted_at);
         CREATE INDEX IF NOT EXISTS idx_records_purge ON records(purge_at) WHERE deleted_at IS NOT NULL;
         CREATE INDEX IF NOT EXISTS idx_materials_hash ON materials(sha256);
         CREATE INDEX IF NOT EXISTS idx_record_materials_record ON record_materials(record_id, sort_order);",
    )?;
    if version < 1 {
        connection.execute_batch("PRAGMA user_version = 1;")?;
    }
    Ok(())
}

fn seed_categories(connection: &Connection) -> AppResult<()> {
    let now = now_iso();
    for (order, (id, name)) in BUILTIN_CATEGORIES.iter().enumerate() {
        connection.execute(
            "INSERT OR IGNORE INTO categories
             (id, name, is_active, is_builtin, sort_order, created_at, updated_at)
             VALUES (?1, ?2, 1, 1, ?3, ?4, ?4)",
            params![id, name, order as i64, now],
        )?;
    }
    Ok(())
}

pub(crate) fn integrity_check(connection: &Connection) -> AppResult<bool> {
    let result: String = connection.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    Ok(result.eq_ignore_ascii_case("ok"))
}

pub(crate) fn now_iso() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

pub(crate) fn parse_score_cents(value: &str) -> AppResult<i64> {
    let value = value.trim();
    if value.is_empty() || value.starts_with('-') || value.starts_with('+') {
        return Err(AppError::validation("分数必须是非负数，最多保留两位小数"));
    }
    let mut parts = value.split('.');
    let integer = parts.next().unwrap_or_default();
    let decimal = parts.next();
    if parts.next().is_some()
        || integer.is_empty()
        || !integer.bytes().all(|byte| byte.is_ascii_digit())
        || decimal.is_some_and(|part| {
            part.is_empty() || part.len() > 2 || !part.bytes().all(|byte| byte.is_ascii_digit())
        })
    {
        return Err(AppError::validation("分数必须是非负数，最多保留两位小数"));
    }
    let whole: i64 = integer
        .parse()
        .map_err(|_| AppError::validation("分数数值过大"))?;
    let fraction = match decimal {
        None => 0,
        Some(part) if part.len() == 1 => part.parse::<i64>().unwrap_or(0) * 10,
        Some(part) => part.parse::<i64>().unwrap_or(0),
    };
    whole
        .checked_mul(100)
        .and_then(|result| result.checked_add(fraction))
        .ok_or_else(|| AppError::validation("分数数值过大"))
}

pub(crate) fn format_score(cents: i64) -> String {
    format!("{}.{:02}", cents / 100, cents.rem_euclid(100))
}

pub(crate) fn academic_year_bounds(value: &str) -> AppResult<(String, String)> {
    let mut years = value.split('-');
    let start: i32 = years
        .next()
        .and_then(|item| item.parse().ok())
        .ok_or_else(|| AppError::validation("学年格式无效"))?;
    let end: i32 = years
        .next()
        .and_then(|item| item.parse().ok())
        .ok_or_else(|| AppError::validation("学年格式无效"))?;
    if years.next().is_some() || end != start + 1 {
        return Err(AppError::validation("学年格式无效"));
    }
    Ok((format!("{start:04}-09-01"), format!("{end:04}-08-31")))
}

fn validate_record_draft(draft: &RecordDraft) -> AppResult<i64> {
    let name = draft.name.trim();
    if name.is_empty() {
        return Err(AppError::validation("活动名称不能为空"));
    }
    if name.chars().count() > 200 {
        return Err(AppError::validation("活动名称不能超过 200 个字"));
    }
    NaiveDate::parse_from_str(&draft.date, "%Y-%m-%d")
        .map_err(|_| AppError::validation("活动日期格式无效"))?;
    if draft.attachment_ids.len() + draft.new_attachments.len() > MAX_MATERIALS_PER_RECORD {
        return Err(AppError::new(
            "TOO_MANY_MATERIALS",
            format!("每条记录最多添加 {MAX_MATERIALS_PER_RECORD} 份证明材料"),
        ));
    }
    if !Uuid::try_parse(&draft.id).is_ok() {
        return Err(AppError::validation("记录 ID 格式无效"));
    }
    parse_score_cents(&draft.score)
}

fn build_filter(filter: &RecordFilter) -> AppResult<(String, Vec<Value>)> {
    let mut clauses = vec![if filter.trashed_only {
        "r.deleted_at IS NOT NULL".to_string()
    } else {
        "r.deleted_at IS NULL".to_string()
    }];
    let mut values = Vec::new();

    if !filter.query.trim().is_empty() {
        clauses.push(
            "(r.name LIKE ? ESCAPE '\\' OR r.notes LIKE ? ESCAPE '\\' OR EXISTS (
               SELECT 1 FROM record_materials qrm
               JOIN materials qm ON qm.id = qrm.material_id
               WHERE qrm.record_id = r.id AND qm.original_name LIKE ? ESCAPE '\\'
             ))"
            .to_string(),
        );
        let escaped = filter
            .query
            .trim()
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        let pattern = format!("%{escaped}%");
        values.extend([
            Value::Text(pattern.clone()),
            Value::Text(pattern.clone()),
            Value::Text(pattern),
        ]);
    }
    if filter.academic_year != "all" && !filter.academic_year.is_empty() {
        let (start, end) = academic_year_bounds(&filter.academic_year)?;
        clauses.push("r.activity_date BETWEEN ? AND ?".to_string());
        values.push(Value::Text(start));
        values.push(Value::Text(end));
    }
    if !filter.date_from.is_empty() {
        NaiveDate::parse_from_str(&filter.date_from, "%Y-%m-%d")
            .map_err(|_| AppError::validation("起始日期格式无效"))?;
        clauses.push("r.activity_date >= ?".to_string());
        values.push(Value::Text(filter.date_from.clone()));
    }
    if !filter.date_to.is_empty() {
        NaiveDate::parse_from_str(&filter.date_to, "%Y-%m-%d")
            .map_err(|_| AppError::validation("结束日期格式无效"))?;
        clauses.push("r.activity_date <= ?".to_string());
        values.push(Value::Text(filter.date_to.clone()));
    }
    if filter.category_id != "all" && !filter.category_id.is_empty() {
        clauses.push("r.category_id = ?".to_string());
        values.push(Value::Text(filter.category_id.clone()));
    }
    if filter.level != "all" && !filter.level.is_empty() {
        if AssessmentLevel::parse(&filter.level).is_none() {
            return Err(AppError::validation("综测级别筛选无效"));
        }
        clauses.push("r.level = ?".to_string());
        values.push(Value::Text(filter.level.clone()));
    }
    match filter.material_status.as_str() {
        "attached" => clauses.push(
            "EXISTS (SELECT 1 FROM record_materials srm WHERE srm.record_id = r.id)".to_string(),
        ),
        "missing" => clauses.push(
            "NOT EXISTS (SELECT 1 FROM record_materials srm WHERE srm.record_id = r.id)"
                .to_string(),
        ),
        "all" | "" => {}
        _ => return Err(AppError::validation("材料状态筛选无效")),
    }
    Ok((clauses.join(" AND "), values))
}

pub fn list_records(state: &AppState, filter: RecordFilter) -> AppResult<RecordListResult> {
    let connection = state.connection()?;
    list_records_with_connection(&connection, &filter)
}

pub fn list_academic_years(state: &AppState) -> AppResult<Vec<String>> {
    let connection = state.connection()?;
    let mut statement = connection.prepare(
        "WITH academic_years AS (
           SELECT CAST(substr(activity_date, 1, 4) AS INTEGER) -
                  CASE WHEN substr(activity_date, 6, 2) < '09' THEN 1 ELSE 0 END AS start_year
           FROM records
           WHERE deleted_at IS NULL
         )
         SELECT printf('%04d-%04d', start_year, start_year + 1)
         FROM academic_years
         GROUP BY start_year
         ORDER BY start_year DESC",
    )?;
    let rows = statement.query_map([], |row| row.get(0))?;
    Ok(rows.collect::<Result<Vec<String>, _>>()?)
}

pub(crate) fn list_records_with_connection(
    connection: &Connection,
    filter: &RecordFilter,
) -> AppResult<RecordListResult> {
    let (where_sql, values) = build_filter(filter)?;
    let total: i64 = connection.query_row(
        &format!("SELECT COUNT(*) FROM records r WHERE {where_sql}"),
        params_from_iter(values.iter()),
        |row| row.get(0),
    )?;

    let order = match filter.sort.as_str() {
        "dateAsc" => "r.activity_date ASC, r.created_at ASC",
        "scoreDesc" => "r.score_cents DESC, r.activity_date DESC",
        "updatedDesc" => "r.updated_at DESC",
        _ => "r.activity_date DESC, r.created_at DESC",
    };
    let page = filter.page.max(1);
    let page_size = filter.page_size.clamp(1, 100_000);
    let offset = u64::from(page - 1) * u64::from(page_size);
    let mut query_values = values;
    query_values.push(Value::Integer(i64::from(page_size)));
    query_values.push(Value::Integer(i64::try_from(offset).unwrap_or(i64::MAX)));
    let sql = format!(
        "SELECT r.id, r.revision, r.name, r.category_id, c.name, r.level,
                r.activity_date, r.score_cents, r.notes, r.created_at, r.updated_at,
                r.deleted_at, r.purge_at
         FROM records r JOIN categories c ON c.id = r.category_id
         WHERE {where_sql}
         ORDER BY {order} LIMIT ? OFFSET ?"
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(query_values.iter()), |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, String>(6)?,
            row.get::<_, i64>(7)?,
            row.get::<_, String>(8)?,
            row.get::<_, String>(9)?,
            row.get::<_, String>(10)?,
            row.get::<_, Option<String>>(11)?,
            row.get::<_, Option<String>>(12)?,
        ))
    })?;

    let mut items = Vec::new();
    for row in rows {
        let (
            id,
            revision,
            name,
            category_id,
            category_name,
            level,
            date,
            score_cents,
            notes,
            created_at,
            updated_at,
            deleted_at,
            purge_at,
        ) = row?;
        items.push(AssessmentRecord {
            materials: load_materials(connection, &id)?,
            id,
            revision,
            name,
            category_id,
            category_name,
            level: AssessmentLevel::parse(&level)
                .ok_or_else(|| AppError::new("INVALID_DATA", "数据库中存在无效综测级别"))?,
            date,
            score: format_score(score_cents),
            notes,
            created_at,
            updated_at,
            deleted_at,
            purge_at,
        });
    }
    Ok(RecordListResult { items, total })
}

pub fn get_record(state: &AppState, id: &str) -> AppResult<AssessmentRecord> {
    let connection = state.connection()?;
    load_record(&connection, id)
}

pub(crate) fn load_record(connection: &Connection, id: &str) -> AppResult<AssessmentRecord> {
    let record = connection
        .query_row(
            "SELECT r.id, r.revision, r.name, r.category_id, c.name, r.level,
                    r.activity_date, r.score_cents, r.notes, r.created_at, r.updated_at,
                    r.deleted_at, r.purge_at
             FROM records r JOIN categories c ON c.id = r.category_id WHERE r.id = ?1",
            [id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, String>(9)?,
                    row.get::<_, String>(10)?,
                    row.get::<_, Option<String>>(11)?,
                    row.get::<_, Option<String>>(12)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| AppError::not_found("记录不存在"))?;
    Ok(AssessmentRecord {
        id: record.0.clone(),
        revision: record.1,
        name: record.2,
        category_id: record.3,
        category_name: record.4,
        level: AssessmentLevel::parse(&record.5)
            .ok_or_else(|| AppError::new("INVALID_DATA", "数据库中存在无效综测级别"))?,
        date: record.6,
        score: format_score(record.7),
        notes: record.8,
        materials: load_materials(connection, &record.0)?,
        created_at: record.9,
        updated_at: record.10,
        deleted_at: record.11,
        purge_at: record.12,
    })
}

fn load_materials(connection: &Connection, record_id: &str) -> AppResult<Vec<Material>> {
    let mut statement = connection.prepare(
        "SELECT m.id, m.original_name, m.size_bytes, m.mime_type, m.sha256, m.created_at
         FROM record_materials rm JOIN materials m ON m.id = rm.material_id
         WHERE rm.record_id = ?1 ORDER BY rm.sort_order, m.created_at",
    )?;
    let rows = statement.query_map([record_id], |row| {
        Ok(Material {
            id: row.get(0)?,
            name: row.get(1)?,
            size: row.get::<_, i64>(2)?.max(0) as u64,
            mime_type: row.get(3)?,
            sha256: row.get(4)?,
            created_at: row.get(5)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub fn save_record(state: &AppState, draft: RecordDraft) -> AppResult<AssessmentRecord> {
    let score_cents = validate_record_draft(&draft)?;
    create_daily_recovery(state)?;
    let root = state.root();
    let prepared = prepare_materials(&root, &draft.new_attachments)?;
    let mut connection = state.connection()?;
    let transaction = connection.transaction()?;

    let category_exists: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM categories WHERE id = ?1)",
        [&draft.category_id],
        |row| row.get(0),
    )?;
    if !category_exists {
        return Err(AppError::new("CATEGORY_NOT_FOUND", "所选活动类别不存在"));
    }

    let current_revision: Option<i64> = transaction
        .query_row(
            "SELECT revision FROM records WHERE id = ?1",
            [&draft.id],
            |row| row.get(0),
        )
        .optional()?;
    let now = now_iso();
    let next_revision = match current_revision {
        Some(revision) => {
            if revision != draft.revision {
                return Err(
                    AppError::new("REVISION_CONFLICT", "记录已被更新，请重新打开后再保存")
                        .details(serde_json::json!({ "currentRevision": revision })),
                );
            }
            transaction.execute(
                "UPDATE records SET revision = revision + 1, name = ?2, category_id = ?3,
                   level = ?4, activity_date = ?5, score_cents = ?6, notes = ?7,
                   updated_at = ?8 WHERE id = ?1",
                params![
                    draft.id,
                    draft.name.trim(),
                    draft.category_id,
                    draft.level.as_str(),
                    draft.date,
                    score_cents,
                    draft.notes.trim(),
                    now,
                ],
            )?;
            revision + 1
        }
        None => {
            if draft.revision != 0 {
                return Err(AppError::new(
                    "REVISION_CONFLICT",
                    "待编辑的记录不存在，无法按原版本保存",
                ));
            }
            transaction.execute(
                "INSERT INTO records
                 (id, revision, name, category_id, level, activity_date, score_cents, notes,
                  created_at, updated_at)
                 VALUES (?1, 1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
                params![
                    draft.id,
                    draft.name.trim(),
                    draft.category_id,
                    draft.level.as_str(),
                    draft.date,
                    score_cents,
                    draft.notes.trim(),
                    now,
                ],
            )?;
            1
        }
    };

    let mut retained = Vec::new();
    for material_id in &draft.attachment_ids {
        let belongs: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM record_materials WHERE record_id = ?1 AND material_id = ?2)",
            params![draft.id, material_id],
            |row| row.get(0),
        )?;
        if !belongs {
            return Err(AppError::new(
                "MATERIAL_NOT_OWNED",
                "记录包含不属于它的证明材料",
            ));
        }
        retained.push(material_id.clone());
    }
    transaction.execute(
        "DELETE FROM record_materials WHERE record_id = ?1",
        [&draft.id],
    )?;
    for (order, material_id) in retained.iter().enumerate() {
        transaction.execute(
            "INSERT INTO record_materials(record_id, material_id, sort_order) VALUES (?1, ?2, ?3)",
            params![draft.id, material_id, order as i64],
        )?;
    }
    for (offset, material) in prepared.iter().enumerate() {
        transaction.execute(
            "INSERT INTO materials
             (id, sha256, original_name, mime_type, size_bytes, stored_rel_path, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                material.id,
                material.sha256,
                material.name,
                material.mime_type,
                i64::try_from(material.size).unwrap_or(i64::MAX),
                material.relative_path,
                now,
            ],
        )?;
        transaction.execute(
            "INSERT INTO record_materials(record_id, material_id, sort_order) VALUES (?1, ?2, ?3)",
            params![draft.id, material.id, retained.len() as i64 + offset as i64],
        )?;
    }
    transaction.commit()?;
    remove_orphan_material_rows(state)?;
    let saved = load_record(&connection, &draft.id)?;
    debug_assert_eq!(saved.revision, next_revision);
    Ok(saved)
}

fn prepare_materials(
    root: &Path,
    attachments: &[PendingMaterial],
) -> AppResult<Vec<PreparedMaterial>> {
    let mut prepared = Vec::new();
    let staging = root.join("staging");
    fs::create_dir_all(&staging)?;

    for attachment in attachments {
        let source = attachment
            .path
            .as_deref()
            .map(Path::new)
            .ok_or_else(|| AppError::validation("证明材料缺少本地路径"))?;
        let metadata = fs::metadata(source)
            .map_err(|error| AppError::io(&format!("无法读取材料 {}", source.display()), error))?;
        if !metadata.is_file() {
            return Err(AppError::validation(format!(
                "证明材料不是普通文件：{}",
                source.display()
            )));
        }
        if metadata.len() > MAX_MATERIAL_BYTES {
            return Err(AppError::new(
                "MATERIAL_TOO_LARGE",
                format!("{} 超过 200 MB", attachment.name),
            ));
        }
        if attachment.size != 0 && attachment.size != metadata.len() {
            return Err(AppError::new(
                "MATERIAL_CHANGED",
                format!("{} 在选择后发生了变化，请重新选择", attachment.name),
            ));
        }

        let temporary = staging.join(format!("{}.part", Uuid::new_v4()));
        let mut input = File::open(source)?;
        let mut output = File::create(&temporary)?;
        let mut hasher = Sha256::new();
        let mut size = 0_u64;
        let mut buffer = vec![0_u8; 1024 * 1024];
        loop {
            let read = input.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            size = size
                .checked_add(read as u64)
                .ok_or_else(|| AppError::new("MATERIAL_TOO_LARGE", "材料大小溢出"))?;
            if size > MAX_MATERIAL_BYTES {
                let _ = fs::remove_file(&temporary);
                return Err(AppError::new(
                    "MATERIAL_TOO_LARGE",
                    format!("{} 超过 200 MB", attachment.name),
                ));
            }
            hasher.update(&buffer[..read]);
            output.write_all(&buffer[..read])?;
        }
        output.sync_all()?;
        drop(output);

        let sha256 = hex::encode(hasher.finalize());
        let relative_path = format!("{}/{}", &sha256[..2], sha256);
        let destination = materials_path(root).join(Path::new(&relative_path));
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        if destination.exists() {
            fs::remove_file(&temporary)?;
        } else {
            fs::rename(&temporary, &destination)?;
        }
        let name = if attachment.name.trim().is_empty() {
            source
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("未命名材料")
                .to_string()
        } else {
            attachment.name.trim().to_string()
        };
        let mime_type = if attachment.mime_type.trim().is_empty() {
            mime_guess::from_path(&name)
                .first_or_octet_stream()
                .essence_str()
                .to_string()
        } else {
            attachment.mime_type.clone()
        };
        prepared.push(PreparedMaterial {
            id: Uuid::new_v4().to_string(),
            name,
            mime_type,
            size,
            sha256,
            relative_path,
        });
    }
    Ok(prepared)
}

pub fn move_records_to_trash(state: &AppState, ids: &[String]) -> AppResult<OperationResult> {
    mutate_record_deletion(state, ids, "trash")
}

pub fn restore_records(state: &AppState, ids: &[String]) -> AppResult<OperationResult> {
    mutate_record_deletion(state, ids, "restore")
}

fn mutate_record_deletion(
    state: &AppState,
    ids: &[String],
    action: &str,
) -> AppResult<OperationResult> {
    if ids.is_empty() {
        return Ok(OperationResult::success("没有需要处理的记录").with_affected(0));
    }
    create_daily_recovery(state)?;
    let mut connection = state.connection()?;
    let transaction = connection.transaction()?;
    let now = now_iso();
    let purge = (Utc::now() + Duration::days(30)).to_rfc3339_opts(SecondsFormat::Millis, true);
    let mut affected = 0_usize;
    for id in ids {
        let changed = if action == "trash" {
            transaction.execute(
                "UPDATE records SET deleted_at = ?2, purge_at = ?3, updated_at = ?2,
                 revision = revision + 1 WHERE id = ?1 AND deleted_at IS NULL",
                params![id, now, purge],
            )?
        } else {
            transaction.execute(
                "UPDATE records SET deleted_at = NULL, purge_at = NULL, updated_at = ?2,
                 revision = revision + 1 WHERE id = ?1 AND deleted_at IS NOT NULL",
                params![id, now],
            )?
        };
        affected += changed;
    }
    transaction.commit()?;
    let message = if action == "trash" {
        format!("已将 {affected} 条记录移入回收站")
    } else {
        format!("已恢复 {affected} 条记录")
    };
    Ok(OperationResult::success(message).with_affected(affected))
}

pub fn permanently_delete_records(state: &AppState, ids: &[String]) -> AppResult<OperationResult> {
    if ids.is_empty() {
        return Ok(OperationResult::success("没有需要删除的记录").with_affected(0));
    }
    create_daily_recovery(state)?;
    let mut connection = state.connection()?;
    let transaction = connection.transaction()?;
    let mut affected = 0_usize;
    for id in ids {
        affected += transaction.execute(
            "DELETE FROM records WHERE id = ?1 AND deleted_at IS NOT NULL",
            [id],
        )?;
    }
    transaction.commit()?;
    remove_orphan_material_rows(state)?;
    Ok(OperationResult::success(format!("已永久删除 {affected} 条记录")).with_affected(affected))
}

pub fn cleanup_expired_records(state: &AppState) -> AppResult<usize> {
    if !state.is_available() {
        return Ok(0);
    }
    let mut connection = state.connection()?;
    let expired: i64 = connection.query_row(
        "SELECT COUNT(*) FROM records WHERE deleted_at IS NOT NULL AND purge_at <= ?1",
        [now_iso()],
        |row| row.get(0),
    )?;
    if expired == 0 {
        return Ok(0);
    }
    create_daily_recovery(state)?;
    let transaction = connection.transaction()?;
    let affected = transaction.execute(
        "DELETE FROM records WHERE deleted_at IS NOT NULL AND purge_at <= ?1",
        [now_iso()],
    )?;
    transaction.commit()?;
    remove_orphan_material_rows(state)?;
    Ok(affected)
}

pub fn list_categories(state: &AppState) -> AppResult<Vec<Category>> {
    let connection = state.connection()?;
    list_categories_with_connection(&connection)
}

pub(crate) fn list_categories_with_connection(connection: &Connection) -> AppResult<Vec<Category>> {
    let mut statement = connection.prepare(
        "SELECT c.id, c.name, c.is_active, c.is_builtin, c.created_at, c.updated_at,
                COUNT(r.id)
         FROM categories c LEFT JOIN records r ON r.category_id = c.id AND r.deleted_at IS NULL
         GROUP BY c.id ORDER BY c.sort_order, c.name COLLATE NOCASE",
    )?;
    let rows = statement.query_map([], |row| {
        Ok(Category {
            id: row.get(0)?,
            name: row.get(1)?,
            is_active: row.get::<_, i64>(2)? != 0,
            is_builtin: row.get::<_, i64>(3)? != 0,
            created_at: row.get(4)?,
            updated_at: row.get(5)?,
            record_count: row.get(6)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub fn save_category(state: &AppState, draft: CategoryDraft) -> AppResult<Category> {
    let name = draft.name.trim();
    if name.is_empty() || name.chars().count() > 40 {
        return Err(AppError::validation("类别名称应为 1 至 40 个字"));
    }
    create_daily_recovery(state)?;
    let connection = state.connection()?;
    let now = now_iso();
    let id = draft.id.unwrap_or_else(|| Uuid::new_v4().to_string());
    let duplicate: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM categories WHERE name = ?1 COLLATE NOCASE AND id <> ?2)",
        params![name, id],
        |row| row.get(0),
    )?;
    if duplicate {
        return Err(AppError::new("DUPLICATE_CATEGORY", "已存在同名活动类别"));
    }
    let exists: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM categories WHERE id = ?1)",
        [&id],
        |row| row.get(0),
    )?;
    if exists {
        connection.execute(
            "UPDATE categories SET name = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, name, now],
        )?;
    } else {
        let sort_order: i64 = connection.query_row(
            "SELECT COALESCE(MAX(sort_order), 0) + 1 FROM categories",
            [],
            |row| row.get(0),
        )?;
        connection.execute(
            "INSERT INTO categories
             (id, name, is_active, is_builtin, sort_order, created_at, updated_at)
             VALUES (?1, ?2, 1, 0, ?3, ?4, ?4)",
            params![id, name, sort_order, now],
        )?;
    }
    category_by_id(&connection, &id)
}

pub fn set_category_active(state: &AppState, id: &str, is_active: bool) -> AppResult<Category> {
    create_daily_recovery(state)?;
    let connection = state.connection()?;
    let changed = connection.execute(
        "UPDATE categories SET is_active = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, i64::from(is_active), now_iso()],
    )?;
    if changed == 0 {
        return Err(AppError::not_found("活动类别不存在"));
    }
    category_by_id(&connection, id)
}

fn category_by_id(connection: &Connection, id: &str) -> AppResult<Category> {
    connection
        .query_row(
            "SELECT c.id, c.name, c.is_active, c.is_builtin, c.created_at, c.updated_at,
                    COUNT(r.id)
             FROM categories c LEFT JOIN records r ON r.category_id = c.id AND r.deleted_at IS NULL
             WHERE c.id = ?1 GROUP BY c.id",
            [id],
            |row| {
                Ok(Category {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    is_active: row.get::<_, i64>(2)? != 0,
                    is_builtin: row.get::<_, i64>(3)? != 0,
                    created_at: row.get(4)?,
                    updated_at: row.get(5)?,
                    record_count: row.get(6)?,
                })
            },
        )
        .optional()?
        .ok_or_else(|| AppError::not_found("活动类别不存在"))
}

pub fn get_statistics(state: &AppState, mut filter: RecordFilter) -> AppResult<StatisticsResult> {
    filter.page = 1;
    filter.page_size = 100_000;
    filter.trashed_only = false;
    let connection = state.connection()?;
    let result = list_records_with_connection(&connection, &filter)?;
    let mut total_score = 0_i64;
    let mut material_count = 0_i64;
    let mut missing = 0_i64;
    let mut levels: HashMap<String, (i64, i64)> = HashMap::new();
    let mut categories: HashMap<String, (String, i64, i64)> = HashMap::new();
    let mut months: BTreeMap<String, (i64, i64)> = BTreeMap::new();

    for record in &result.items {
        let score = parse_score_cents(&record.score)?;
        total_score += score;
        material_count += record.materials.len() as i64;
        if record.materials.is_empty() {
            missing += 1;
        }
        let level = record.level.as_str().to_string();
        let level_entry = levels.entry(level).or_default();
        level_entry.0 += 1;
        level_entry.1 += score;
        let category_entry = categories
            .entry(record.category_id.clone())
            .or_insert_with(|| (record.category_name.clone(), 0, 0));
        category_entry.1 += 1;
        category_entry.2 += score;
        let month_entry = months.entry(record.date[..7].to_string()).or_default();
        month_entry.0 += 1;
        month_entry.1 += score;
    }

    let by_level = [
        AssessmentLevel::College,
        AssessmentLevel::School,
        AssessmentLevel::Provincial,
        AssessmentLevel::National,
    ]
    .into_iter()
    .map(|level| {
        let (count, score) = levels.get(level.as_str()).copied().unwrap_or_default();
        ChartDatum {
            key: level.as_str().to_string(),
            label: level.label().to_string(),
            count,
            score: format_score(score),
        }
    })
    .collect();
    let mut by_category: Vec<_> = categories
        .into_iter()
        .map(|(key, (label, count, score))| ChartDatum {
            key,
            label,
            count,
            score: format_score(score),
        })
        .collect();
    by_category.sort_by(|left, right| {
        parse_score_cents(&right.score)
            .unwrap_or(0)
            .cmp(&parse_score_cents(&left.score).unwrap_or(0))
    });
    let monthly = months
        .into_iter()
        .map(|(month, (count, score))| MonthlyDatum {
            month,
            count,
            score: format_score(score),
        })
        .collect();

    Ok(StatisticsResult {
        summary: MetricSummary {
            record_count: result.total,
            total_score: format_score(total_score),
            material_count,
            missing_material_count: missing,
        },
        by_level,
        by_category,
        monthly,
    })
}

pub fn open_material(state: &AppState, material_id: &str) -> AppResult<OperationResult> {
    let connection = state.connection()?;
    let relative: String = connection
        .query_row(
            "SELECT stored_rel_path FROM materials WHERE id = ?1",
            [material_id],
            |row| row.get(0),
        )
        .optional()?
        .ok_or_else(|| AppError::not_found("证明材料不存在"))?;
    let path = safe_material_path(&state.root(), &relative)?;
    if !path.is_file() {
        return Err(AppError::new(
            "MATERIAL_MISSING",
            format!("材料文件不存在：{}", path.display()),
        ));
    }
    let canonical_root = materials_path(&state.root())
        .canonicalize()
        .map_err(|error| AppError::io("无法校验证明材料目录", error))?;
    let canonical_path = path
        .canonicalize()
        .map_err(|error| AppError::io("无法校验证明材料路径", error))?;
    if !canonical_path.starts_with(&canonical_root) {
        return Err(AppError::new("INVALID_PATH", "证明材料路径超出托管目录"));
    }
    open::that(&canonical_path).map_err(|error| AppError::io("无法使用系统程序打开材料", error))?;
    Ok(OperationResult::success("已打开证明材料"))
}

pub fn get_material_preview(state: &AppState, material_id: &str) -> AppResult<MaterialPreview> {
    let connection = state.connection()?;
    let material = connection
        .query_row(
            "SELECT original_name, mime_type, stored_rel_path
             FROM materials WHERE id = ?1",
            [material_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| AppError::not_found("证明材料不存在"))?;

    const PREVIEW_MIME_TYPES: &[&str] = &[
        "application/pdf",
        "image/jpeg",
        "image/png",
        "image/gif",
        "image/webp",
        "image/bmp",
    ];
    if !PREVIEW_MIME_TYPES.contains(&material.1.as_str()) {
        return Err(AppError::new(
            "PREVIEW_UNSUPPORTED",
            "该文件格式不支持内置预览，请使用系统程序打开",
        ));
    }

    let path = safe_material_path(&state.root(), &material.2)?;
    if !path.is_file() {
        return Err(AppError::new(
            "MATERIAL_MISSING",
            format!("材料文件不存在：{}", path.display()),
        ));
    }

    // canonicalize also blocks a replaced content-store entry from escaping through a symlink.
    let canonical_root = materials_path(&state.root())
        .canonicalize()
        .map_err(|error| AppError::io("无法校验证明材料目录", error))?;
    let canonical_path = path
        .canonicalize()
        .map_err(|error| AppError::io("无法校验证明材料路径", error))?;
    if !canonical_path.starts_with(&canonical_root) {
        return Err(AppError::new("INVALID_PATH", "证明材料路径超出托管目录"));
    }

    Ok(MaterialPreview {
        name: material.0,
        mime_type: material.1,
        path: canonical_path.to_string_lossy().into_owned(),
    })
}

pub(crate) fn safe_material_path(root: &Path, relative: &str) -> AppResult<PathBuf> {
    let relative_path = Path::new(relative);
    if relative_path.is_absolute()
        || relative_path
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Err(AppError::new("INVALID_PATH", "材料存储路径无效"));
    }
    Ok(materials_path(root).join(relative_path))
}

pub(crate) fn material_files_for_records(
    connection: &Connection,
    record_ids: Option<&[String]>,
) -> AppResult<Vec<(String, MaterialFile)>> {
    let mut sql = String::from(
        "SELECT rm.record_id, m.id, m.original_name, m.mime_type, m.size_bytes,
                m.sha256, m.stored_rel_path
         FROM record_materials rm JOIN materials m ON m.id = rm.material_id",
    );
    let mut values = Vec::new();
    if let Some(ids) = record_ids {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        sql.push_str(" WHERE rm.record_id IN (");
        sql.push_str(&vec!["?"; ids.len()].join(","));
        sql.push(')');
        values.extend(ids.iter().cloned().map(Value::Text));
    }
    sql.push_str(" ORDER BY rm.record_id, rm.sort_order");
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(values.iter()), |row| {
        Ok((
            row.get(0)?,
            MaterialFile {
                id: row.get(1)?,
                name: row.get(2)?,
                mime_type: row.get(3)?,
                size: row.get::<_, i64>(4)?.max(0) as u64,
                sha256: row.get(5)?,
                relative_path: row.get(6)?,
            },
        ))
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub(crate) fn excel_rows_for_filter(
    connection: &Connection,
    mut filter: RecordFilter,
) -> AppResult<Vec<ExcelRecordRow>> {
    filter.page = 1;
    filter.page_size = 100_000;
    let records = list_records_with_connection(connection, &filter)?.items;
    Ok(records
        .into_iter()
        .map(|record| ExcelRecordRow {
            id: Some(record.id),
            title: record.name,
            category: record.category_name,
            level: record.level.as_str().to_string(),
            date: record.date,
            score: record.score,
            remark: record.notes,
            material_count: record.materials.len() as u32,
            material_names: record.materials.into_iter().map(|item| item.name).collect(),
        })
        .collect())
}

pub(crate) fn create_daily_recovery(state: &AppState) -> AppResult<()> {
    if !state.is_available() {
        return Ok(());
    }
    let root = state.root();
    let directory = recovery_path(&root);
    fs::create_dir_all(&directory)?;
    let date = Local::now().format("%Y-%m-%d");
    let destination_path = directory.join(format!("恢复点_{date}.sqlite3"));
    if destination_path.exists() {
        return Ok(());
    }
    let source = state.connection()?;
    let mut destination = Connection::open(&destination_path)?;
    {
        let backup = Backup::new(&source, &mut destination)?;
        backup.run_to_completion(16, StdDuration::from_millis(10), None)?;
    }
    destination.execute_batch("PRAGMA journal_mode = DELETE;")?;
    prune_recovery_points(&directory)?;
    Ok(())
}

fn prune_recovery_points(directory: &Path) -> AppResult<()> {
    let mut points: Vec<_> = fs::read_dir(directory)?
        .filter_map(Result::ok)
        .filter(|entry| {
            entry.file_type().is_ok_and(|kind| kind.is_file())
                && entry.file_name().to_string_lossy().starts_with("恢复点_")
        })
        .collect();
    points.sort_by_key(|entry| entry.file_name());
    let remove_count = points.len().saturating_sub(7);
    for point in points.into_iter().take(remove_count) {
        fs::remove_file(point.path())?;
    }
    Ok(())
}

fn remove_orphan_material_rows(state: &AppState) -> AppResult<()> {
    let connection = state.connection()?;
    connection.execute(
        "DELETE FROM materials WHERE NOT EXISTS (
           SELECT 1 FROM record_materials rm WHERE rm.material_id = materials.id
         )",
        [],
    )?;
    drop(connection);
    cleanup_content_store(&state.root())
}

fn cleanup_content_store(root: &Path) -> AppResult<()> {
    let mut referenced = HashSet::new();
    let current = open_database_at(root)?;
    collect_paths_from_database(&current, &mut referenced)?;
    drop(current);
    let recovery = recovery_path(root);
    if recovery.is_dir() {
        for entry in fs::read_dir(&recovery)?.filter_map(Result::ok) {
            if entry.path().extension().and_then(|item| item.to_str()) != Some("sqlite3") {
                continue;
            }
            if let Ok(connection) = Connection::open(entry.path()) {
                let _ = collect_paths_from_database(&connection, &mut referenced);
            }
        }
    }

    let material_root = materials_path(root);
    if !material_root.is_dir() {
        return Ok(());
    }
    for entry in WalkDir::new(&material_root)
        .min_depth(1)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
    {
        let relative = entry
            .path()
            .strip_prefix(&material_root)
            .unwrap_or(entry.path())
            .to_string_lossy()
            .replace('\\', "/");
        if !referenced.contains(&relative) {
            let _ = fs::remove_file(entry.path());
        }
    }
    Ok(())
}

fn collect_paths_from_database(
    connection: &Connection,
    referenced: &mut HashSet<String>,
) -> AppResult<()> {
    let mut statement = connection.prepare(
        "SELECT DISTINCT m.stored_rel_path FROM materials m
         JOIN record_materials rm ON rm.material_id = m.id",
    )?;
    let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
    for row in rows {
        referenced.insert(row?.replace('\\', "/"));
    }
    Ok(())
}

pub(crate) fn create_database_snapshot(source: &Connection, path: &Path) -> AppResult<()> {
    if path.exists() {
        fs::remove_file(path)?;
    }
    let mut destination = Connection::open(path)?;
    {
        let backup = Backup::new(source, &mut destination)?;
        backup.run_to_completion(16, StdDuration::from_millis(10), None)?;
    }
    destination.execute_batch("PRAGMA journal_mode = DELETE;")?;
    if !integrity_check(&destination)? {
        return Err(AppError::new("BACKUP_INVALID", "数据库快照完整性检查失败"));
    }
    Ok(())
}

pub fn get_storage_status(state: &AppState) -> AppResult<StorageStatus> {
    let root = state.root();
    let database_bytes = fs::metadata(database_path(&root))
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    let material_bytes = directory_size(&materials_path(&root))?;
    let recovery_point_count = fs::read_dir(recovery_path(&root))
        .map(|entries| entries.filter_map(Result::ok).count())
        .unwrap_or(0);
    let writable = state.is_available()
        && fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(root.join("staging").join(".write-test"))
            .and_then(|_| fs::remove_file(root.join("staging").join(".write-test")))
            .is_ok();
    let available_bytes = fs2::available_space(&root).unwrap_or(0);
    Ok(StorageStatus {
        root: root.to_string_lossy().into_owned(),
        database_bytes,
        material_bytes,
        recovery_point_count,
        writable,
        available_bytes,
    })
}

pub fn migrate_data_root(state: &AppState, destination: &str) -> AppResult<OperationResult> {
    let selected = PathBuf::from(destination);
    if !selected.is_absolute() {
        return Err(AppError::validation("请选择绝对路径作为新数据位置"));
    }
    fs::create_dir_all(&selected)?;
    let source = state.root();
    let source_available = state.is_available() && source.is_dir();
    let selected_canonical = selected.canonicalize()?;
    if source_available {
        let source_canonical = source.canonicalize()?;
        if selected_canonical.starts_with(&source_canonical)
            || source_canonical.starts_with(selected_canonical.join("综测记录数据"))
        {
            return Err(AppError::validation("新数据位置不能位于当前数据目录内部"));
        }
    }
    let target = if selected.file_name().and_then(|name| name.to_str()) == Some("综测记录数据")
    {
        selected.clone()
    } else {
        selected.join("综测记录数据")
    };
    if target.exists() && fs::read_dir(&target)?.next().is_some() {
        return Err(AppError::new(
            "DESTINATION_NOT_EMPTY",
            format!("目标目录不为空：{}", target.display()),
        ));
    }

    let required = if source_available {
        directory_size(&source)?
    } else {
        0
    };
    let available = fs2::available_space(&selected).unwrap_or(0);
    if available < required.saturating_add(64 * 1024 * 1024) {
        return Err(AppError::new(
            "INSUFFICIENT_SPACE",
            format!("目标磁盘空间不足，需要至少 {} 字节", required),
        ));
    }

    let staging = selected.join(format!(".综测记录迁移_{}", Uuid::new_v4()));
    if source_available {
        copy_tree_verified(&source, &staging)?;
    } else {
        ensure_data_directories(&staging)?;
        let connection = open_database_at(&staging)?;
        migrate(&connection)?;
        seed_categories(&connection)?;
    }
    if target.exists() {
        fs::remove_dir_all(&target)?;
    }
    fs::rename(&staging, &target)?;
    let connection = open_database_at(&target)?;
    if !integrity_check(&connection)? {
        let _ = fs::remove_dir_all(&target);
        return Err(AppError::new(
            "MIGRATION_INVALID",
            "迁移后的数据库完整性检查失败",
        ));
    }
    drop(connection);
    state.write_pointer(&target)?;
    state.set_root(target.clone());
    Ok(
        OperationResult::success("数据位置已迁移并完成校验；旧副本暂时保留")
            .with_path(target.to_string_lossy()),
    )
}

fn directory_size(path: &Path) -> AppResult<u64> {
    if !path.exists() {
        return Ok(0);
    }
    let mut total = 0_u64;
    for entry in WalkDir::new(path).into_iter().filter_map(Result::ok) {
        if entry.file_type().is_file() {
            total = total.saturating_add(entry.metadata().map(|item| item.len()).unwrap_or(0));
        }
    }
    Ok(total)
}

fn copy_tree_verified(source: &Path, destination: &Path) -> AppResult<()> {
    fs::create_dir_all(destination)?;
    for entry in WalkDir::new(source).into_iter().filter_map(Result::ok) {
        let relative = entry
            .path()
            .strip_prefix(source)
            .map_err(|error| AppError::io("无法计算迁移相对路径", error))?;
        let target = destination.join(relative);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target)?;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(entry.path(), &target)?;
        if hash_file(entry.path())? != hash_file(&target)? {
            return Err(AppError::new(
                "HASH_MISMATCH",
                format!("迁移校验失败：{}", relative.display()),
            ));
        }
    }
    Ok(())
}

pub(crate) fn hash_file(path: &Path) -> AppResult<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration as StdDuration, Instant};

    fn test_state() -> (tempfile::TempDir, AppState) {
        let temp = tempfile::tempdir().expect("临时目录");
        let root = temp.path().join("数据");
        ensure_data_directories(&root).expect("创建目录");
        let connection = open_database_at(&root).expect("打开数据库");
        migrate(&connection).expect("迁移");
        seed_categories(&connection).expect("类别");
        drop(connection);
        let state = AppState {
            location: RwLock::new(DataLocation {
                root,
                available: true,
            }),
            pointer_file: temp.path().join("存储位置.json"),
            imports: Mutex::new(HashMap::new()),
            backups: Mutex::new(HashMap::new()),
        };
        (temp, state)
    }

    fn draft(id: String, score: &str, date: &str) -> RecordDraft {
        RecordDraft {
            id,
            revision: 0,
            name: "测试活动".to_string(),
            category_id: BUILTIN_CATEGORIES[0].0.to_string(),
            level: AssessmentLevel::School,
            date: date.to_string(),
            score: score.to_string(),
            notes: String::new(),
            attachment_ids: Vec::new(),
            new_attachments: Vec::new(),
        }
    }

    #[test]
    fn score_is_exact() {
        assert_eq!(parse_score_cents("0.10").unwrap(), 10);
        assert_eq!(parse_score_cents("123.4").unwrap(), 12_340);
        assert_eq!(format_score(12_340), "123.40");
        assert!(parse_score_cents("1.001").is_err());
    }

    #[test]
    fn academic_year_boundaries_are_september_to_august() {
        assert_eq!(
            academic_year_bounds("2025-2026").unwrap(),
            ("2025-09-01".to_string(), "2026-08-31".to_string())
        );
    }

    #[test]
    fn academic_year_list_is_distinct_descending_and_excludes_trash() {
        let (_temp, state) = test_state();
        let older_id = Uuid::new_v4().to_string();
        let newer_id = Uuid::new_v4().to_string();
        let duplicate_year_id = Uuid::new_v4().to_string();
        save_record(&state, draft(older_id, "1.00", "2026-08-31")).unwrap();
        save_record(&state, draft(newer_id.clone(), "2.00", "2026-09-01")).unwrap();
        save_record(
            &state,
            draft(duplicate_year_id.clone(), "3.00", "2026-10-01"),
        )
        .unwrap();

        assert_eq!(
            list_academic_years(&state).unwrap(),
            vec!["2026-2027".to_string(), "2025-2026".to_string()]
        );

        move_records_to_trash(&state, &[newer_id, duplicate_year_id]).unwrap();
        assert_eq!(
            list_academic_years(&state).unwrap(),
            vec!["2025-2026".to_string()]
        );
    }

    #[test]
    fn crud_revision_statistics_and_trash() {
        let (_temp, state) = test_state();
        let id = Uuid::new_v4().to_string();
        let saved = save_record(&state, draft(id.clone(), "1.25", "2026-08-31")).unwrap();
        assert_eq!(saved.revision, 1);
        assert_eq!(saved.score, "1.25");

        let mut stale = draft(id.clone(), "2.00", "2026-09-01");
        stale.revision = 0;
        assert_eq!(
            save_record(&state, stale).unwrap_err().code,
            "REVISION_CONFLICT"
        );

        let statistics = get_statistics(&state, RecordFilter::default()).unwrap();
        assert_eq!(statistics.summary.record_count, 1);
        assert_eq!(statistics.summary.total_score, "1.25");

        move_records_to_trash(&state, std::slice::from_ref(&id)).unwrap();
        assert_eq!(
            list_records(&state, RecordFilter::default()).unwrap().total,
            0
        );
        let trash_filter = RecordFilter {
            trashed_only: true,
            ..RecordFilter::default()
        };
        assert_eq!(list_records(&state, trash_filter).unwrap().total, 1);
        restore_records(&state, std::slice::from_ref(&id)).unwrap();
        assert_eq!(
            list_records(&state, RecordFilter::default()).unwrap().total,
            1
        );
    }

    #[test]
    fn preview_only_returns_managed_image_or_pdf_paths() {
        let (temp, state) = test_state();
        let source = temp.path().join("证明.png");
        fs::write(&source, b"not-a-real-png-but-managed").unwrap();
        let mut input = draft(Uuid::new_v4().to_string(), "1.00", "2026-08-31");
        input.new_attachments.push(PendingMaterial {
            _client_id: None,
            name: "证明.png".to_string(),
            size: 0,
            mime_type: "image/png".to_string(),
            path: Some(source.to_string_lossy().into_owned()),
        });
        let saved = save_record(&state, input).unwrap();
        let preview = get_material_preview(&state, &saved.materials[0].id).unwrap();
        assert_eq!(preview.name, "证明.png");
        assert!(Path::new(&preview.path)
            .starts_with(materials_path(&state.root()).canonicalize().unwrap()));

        let connection = state.connection().unwrap();
        connection
            .execute(
                "UPDATE materials SET mime_type = 'application/zip' WHERE id = ?1",
                [&saved.materials[0].id],
            )
            .unwrap();
        drop(connection);
        assert_eq!(
            get_material_preview(&state, &saved.materials[0].id)
                .unwrap_err()
                .code,
            "PREVIEW_UNSUPPORTED"
        );
    }

    #[test]
    #[ignore = "本机性能基准；使用 cargo test performance_5000 -- --ignored --nocapture 显式运行"]
    fn performance_5000_records_filter_and_statistics_under_500ms() {
        const RECORD_COUNT: usize = 5_000;
        const LIMIT: StdDuration = StdDuration::from_millis(500);
        let (_temp, state) = test_state();
        let mut connection = state.connection().expect("打开性能测试数据库");
        let transaction = connection.transaction().expect("开始批量插入事务");
        {
            let mut insert = transaction
                .prepare(
                    "INSERT INTO records
                     (id, revision, name, category_id, level, activity_date, score_cents, notes,
                      created_at, updated_at)
                     VALUES (?1, 1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
                )
                .expect("准备批量插入语句");
            let levels = ["college", "school", "provincial", "national"];
            let first_day = NaiveDate::from_ymd_opt(2025, 9, 1).expect("固定起始日期");
            for index in 0..RECORD_COUNT {
                let date = first_day
                    .checked_add_signed(Duration::days((index % 365) as i64))
                    .expect("生成测试日期")
                    .format("%Y-%m-%d")
                    .to_string();
                let notes = if index % 7 == 0 {
                    "专项检索；5000 条性能验证"
                } else {
                    "5000 条性能验证"
                };
                insert
                    .execute(params![
                        format!("performance-record-{index:04}"),
                        format!("性能验证活动 {index:04}"),
                        BUILTIN_CATEGORIES[index % BUILTIN_CATEGORIES.len()].0,
                        levels[(index / BUILTIN_CATEGORIES.len()) % levels.len()],
                        date,
                        (index % 10_000) as i64,
                        notes,
                        "2026-09-01T00:00:00.000Z",
                    ])
                    .expect("插入性能测试记录");
            }
        }
        transaction.commit().expect("提交批量插入事务");
        drop(connection);

        let list_filter = RecordFilter {
            query: "专项检索".to_string(),
            academic_year: "2025-2026".to_string(),
            category_id: BUILTIN_CATEGORIES[0].0.to_string(),
            level: "school".to_string(),
            material_status: "missing".to_string(),
            sort: "scoreDesc".to_string(),
            page: 1,
            page_size: 50,
            ..RecordFilter::default()
        };
        let statistics_filter = RecordFilter {
            academic_year: "2025-2026".to_string(),
            ..RecordFilter::default()
        };

        list_records(&state, list_filter.clone()).expect("预热筛选查询");
        get_statistics(&state, statistics_filter.clone()).expect("预热统计查询");

        let list_started = Instant::now();
        let page = list_records(&state, list_filter).expect("执行 5000 条记录筛选");
        let list_elapsed = list_started.elapsed();
        assert!(page.total > 0, "代表性筛选应命中测试记录");

        let statistics_started = Instant::now();
        let statistics = get_statistics(&state, statistics_filter).expect("执行 5000 条记录统计");
        let statistics_elapsed = statistics_started.elapsed();
        assert_eq!(statistics.summary.record_count, RECORD_COUNT as i64);

        println!(
            "5000 条记录性能：筛选 {:?}（命中 {} 条），统计 {:?}（{} 条）",
            list_elapsed, page.total, statistics_elapsed, statistics.summary.record_count
        );
        assert!(
            list_elapsed < LIMIT,
            "5000 条记录筛选耗时 {list_elapsed:?}，超过 500 ms"
        );
        assert!(
            statistics_elapsed < LIMIT,
            "5000 条记录统计耗时 {statistics_elapsed:?}，超过 500 ms"
        );
    }
}
