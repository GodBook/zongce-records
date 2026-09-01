use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AssessmentLevel {
    College,
    School,
    Provincial,
    National,
}

impl AssessmentLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::College => "college",
            Self::School => "school",
            Self::Provincial => "provincial",
            Self::National => "national",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "college" | "院级" => Some(Self::College),
            "school" | "university" | "校级" => Some(Self::School),
            "provincial" | "省级" => Some(Self::Provincial),
            "national" | "国家级" => Some(Self::National),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::College => "院级",
            Self::School => "校级",
            Self::Provincial => "省级",
            Self::National => "国家级",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Category {
    pub id: String,
    pub name: String,
    pub is_active: bool,
    pub is_builtin: bool,
    pub record_count: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Material {
    pub id: String,
    pub name: String,
    pub size: u64,
    pub mime_type: String,
    pub sha256: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MaterialPreview {
    pub name: String,
    pub mime_type: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssessmentRecord {
    pub id: String,
    pub revision: i64,
    pub name: String,
    pub category_id: String,
    pub category_name: String,
    pub level: AssessmentLevel,
    pub date: String,
    pub score: String,
    pub notes: String,
    pub materials: Vec<Material>,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
    pub purge_at: Option<String>,
}

fn default_all() -> String {
    "all".to_string()
}

fn default_sort() -> String {
    "dateDesc".to_string()
}

fn default_page() -> u32 {
    1
}

fn default_page_size() -> u32 {
    100
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RecordFilter {
    pub query: String,
    pub academic_year: String,
    pub date_from: String,
    pub date_to: String,
    pub category_id: String,
    pub level: String,
    pub material_status: String,
    pub sort: String,
    pub page: u32,
    pub page_size: u32,
    pub trashed_only: bool,
}

impl Default for RecordFilter {
    fn default() -> Self {
        Self {
            query: String::new(),
            academic_year: default_all(),
            date_from: String::new(),
            date_to: String::new(),
            category_id: default_all(),
            level: default_all(),
            material_status: default_all(),
            sort: default_sort(),
            page: default_page(),
            page_size: default_page_size(),
            trashed_only: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordListResult {
    pub items: Vec<AssessmentRecord>,
    pub total: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordDraft {
    pub id: String,
    pub revision: i64,
    pub name: String,
    pub category_id: String,
    pub level: AssessmentLevel,
    pub date: String,
    pub score: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub attachment_ids: Vec<String>,
    #[serde(default)]
    pub new_attachments: Vec<PendingMaterial>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingMaterial {
    #[serde(rename = "clientId")]
    pub _client_id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub mime_type: String,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryDraft {
    pub id: Option<String>,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricSummary {
    pub record_count: i64,
    pub total_score: String,
    pub material_count: i64,
    pub missing_material_count: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartDatum {
    pub key: String,
    pub label: String,
    pub count: i64,
    pub score: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonthlyDatum {
    pub month: String,
    pub count: i64,
    pub score: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatisticsResult {
    pub summary: MetricSummary,
    pub by_level: Vec<ChartDatum>,
    pub by_category: Vec<ChartDatum>,
    pub monthly: Vec<MonthlyDatum>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInitialization {
    pub app_version: String,
    pub storage_root: String,
    pub database_healthy: bool,
    pub recovery_required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationResult {
    pub success: bool,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub affected: Option<usize>,
}

impl OperationResult {
    pub fn success(message: impl Into<String>) -> Self {
        Self {
            success: true,
            message: message.into(),
            path: None,
            affected: None,
        }
    }

    pub fn with_path(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }

    pub fn with_affected(mut self, affected: usize) -> Self {
        self.affected = Some(affected);
        self
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageStatus {
    pub root: String,
    pub database_bytes: u64,
    pub material_bytes: u64,
    pub recovery_point_count: usize,
    pub writable: bool,
    pub available_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportRowPreview {
    pub row: u32,
    pub status: String,
    pub name: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreview {
    pub token: String,
    pub file_name: String,
    pub total: usize,
    pub new_count: usize,
    pub update_count: usize,
    pub skip_count: usize,
    pub duplicate_count: usize,
    pub error_count: usize,
    pub rows: Vec<ImportRowPreview>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupInspection {
    pub token: String,
    pub file_name: String,
    pub created_at: String,
    pub app_version: String,
    pub record_count: i64,
    pub material_count: i64,
    pub total_bytes: u64,
    pub integrity_valid: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub available: bool,
    pub current_version: String,
    pub version: String,
    pub published_at: String,
    pub notes: String,
}
