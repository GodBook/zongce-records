use std::collections::{HashMap, HashSet};
use std::path::Path;

use calamine::{open_workbook_auto, Data, Range, Reader};
use chrono::{Duration, NaiveDate};
use rust_xlsxwriter::{Color, Format, FormatAlign, FormatBorder, Workbook, Worksheet};
use serde::{Deserialize, Serialize};

const DETAIL_HEADERS: [&str; 9] = [
    "记录 ID",
    "活动名称",
    "活动类别",
    "综测级别",
    "日期",
    "分数",
    "备注",
    "材料数量",
    "证明材料名称",
];

const REQUIRED_COLUMNS: [(Column, &str); 5] = [
    (Column::Title, "活动名称"),
    (Column::Category, "活动类别"),
    (Column::Level, "综测级别"),
    (Column::Date, "日期"),
    (Column::Score, "分数"),
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExcelRecordRow {
    pub id: Option<String>,
    pub title: String,
    pub category: String,
    pub level: String,
    pub date: String,
    pub score: String,
    pub remark: String,
    pub material_count: u32,
    pub material_names: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExcelImportIssue {
    /// Excel 中从 1 开始的行号。
    pub row: u32,
    /// 用户可识别的字段名，或无法映射字段时的 Excel 列名。
    pub column: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ParsedExcel {
    pub rows: Vec<ExcelRecordRow>,
    pub issues: Vec<ExcelImportIssue>,
    #[serde(default)]
    pub row_numbers: Vec<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Column {
    Id,
    Title,
    Category,
    Level,
    Date,
    Score,
    Remark,
    MaterialCount,
    MaterialNames,
}

pub fn write_template(path: &Path) -> Result<(), String> {
    ensure_parent_exists(path)?;
    let mut workbook = Workbook::new();
    {
        let worksheet = workbook.add_worksheet();
        worksheet
            .set_name("综测记录")
            .map_err(|error| format!("无法创建模板工作表：{error}"))?;
        write_detail_header(worksheet)?;
    }
    workbook
        .save(path)
        .map_err(|error| format!("无法保存 Excel 模板：{error}"))
}

pub fn write_records(
    path: &Path,
    rows: &[ExcelRecordRow],
    stats: &[(String, String)],
) -> Result<(), String> {
    ensure_parent_exists(path)?;
    let mut workbook = Workbook::new();

    {
        let worksheet = workbook.add_worksheet();
        worksheet
            .set_name("记录明细")
            .map_err(|error| format!("无法创建记录明细工作表：{error}"))?;
        write_detail_header(worksheet)?;

        for (index, record) in rows.iter().enumerate() {
            let row =
                u32::try_from(index + 1).map_err(|_| "导出记录数量超过 Excel 上限".to_string())?;
            let values = [
                safe_text(record.id.as_deref().unwrap_or_default()),
                safe_text(&record.title),
                safe_text(&record.category),
                safe_text(level_display_name(&record.level)),
                safe_text(&record.date),
                safe_text(&record.score),
                safe_text(&record.remark),
                record.material_count.to_string(),
                safe_text(&record.material_names.join("；")),
            ];

            for (column, value) in values.iter().enumerate() {
                worksheet
                    .write_string(row, column as u16, value)
                    .map_err(|error| format!("写入第 {} 行失败：{error}", row + 1))?;
            }
        }
    }

    {
        let header_format = header_format();
        let worksheet = workbook.add_worksheet();
        worksheet
            .set_name("统计汇总")
            .map_err(|error| format!("无法创建统计汇总工作表：{error}"))?;
        worksheet
            .write_string_with_format(0, 0, "统计项", &header_format)
            .and_then(|sheet| sheet.write_string_with_format(0, 1, "数值", &header_format))
            .map_err(|error| format!("写入统计汇总表头失败：{error}"))?;
        for (index, (name, value)) in stats.iter().enumerate() {
            let row =
                u32::try_from(index + 1).map_err(|_| "统计项数量超过 Excel 上限".to_string())?;
            worksheet
                .write_string(row, 0, safe_text(name))
                .and_then(|sheet| sheet.write_string(row, 1, safe_text(value)))
                .map_err(|error| format!("写入统计汇总第 {} 行失败：{error}", row + 1))?;
        }
        worksheet
            .set_column_width(0, 24)
            .and_then(|sheet| sheet.set_column_width(1, 18))
            .map_err(|error| format!("设置统计汇总列宽失败：{error}"))?;
        worksheet
            .set_freeze_panes(1, 0)
            .map_err(|error| format!("冻结统计汇总表头失败：{error}"))?;
    }

    workbook
        .save(path)
        .map_err(|error| format!("无法保存 Excel 文件：{error}"))
}

pub fn parse_records(path: &Path) -> Result<ParsedExcel, String> {
    let mut workbook =
        open_workbook_auto(path).map_err(|error| format!("无法打开 Excel 文件：{error}"))?;
    let sheet_names = workbook.sheet_names();
    let sheet_name = ["综测记录", "记录明细"]
        .iter()
        .find(|preferred| sheet_names.iter().any(|name| name == **preferred))
        .map(|name| (*name).to_string())
        .or_else(|| sheet_names.first().cloned())
        .ok_or_else(|| "Excel 文件中没有工作表".to_string())?;

    let formulas = workbook
        .worksheet_formula(&sheet_name)
        .map_err(|error| format!("无法读取工作表公式：{error}"))?;
    let formula_cells = collect_formula_cells(&formulas);
    let range = workbook
        .worksheet_range(&sheet_name)
        .map_err(|error| format!("无法读取工作表“{sheet_name}”：{error}"))?;

    if range.is_empty() {
        return Ok(ParsedExcel {
            rows: Vec::new(),
            issues: vec![ExcelImportIssue {
                row: 1,
                column: "表头".to_string(),
                message: "工作表为空，请使用官方模板填写后再导入".to_string(),
            }],
            row_numbers: Vec::new(),
        });
    }

    let (header_row, columns) = match find_header_row(&range) {
        Some(found) => found,
        None => {
            return Ok(ParsedExcel {
                rows: Vec::new(),
                issues: vec![ExcelImportIssue {
                    row: range.start().map_or(1, |(row, _)| row + 1),
                    column: "表头".to_string(),
                    message: "未找到可识别的综测记录表头".to_string(),
                }],
                row_numbers: Vec::new(),
            });
        }
    };

    let mut parsed = ParsedExcel::default();
    let absolute_start = range.start().unwrap_or((0, 0));
    for (required, display_name) in REQUIRED_COLUMNS {
        if !columns.contains_key(&required) {
            parsed.issues.push(ExcelImportIssue {
                row: absolute_start.0 + header_row as u32 + 1,
                column: display_name.to_string(),
                message: format!("缺少必填列“{display_name}”"),
            });
        }
    }
    if !parsed.issues.is_empty() {
        return Ok(parsed);
    }

    for relative_row in (header_row + 1)..range.height() {
        if row_is_empty(&range, relative_row) {
            continue;
        }

        let excel_row = absolute_start.0 + relative_row as u32 + 1;
        let mut row_issues = Vec::new();
        for relative_column in 0..range.width() {
            let absolute_column = absolute_start.1 + relative_column as u32;
            if formula_cells.contains(&(excel_row - 1, absolute_column)) {
                row_issues.push(ExcelImportIssue {
                    row: excel_row,
                    column: excel_column_name(absolute_column),
                    message: "不允许使用公式，请粘贴为值后重试".to_string(),
                });
            }
        }
        if !row_issues.is_empty() {
            parsed.issues.extend(row_issues);
            continue;
        }

        let title = required_text(
            &range,
            relative_row,
            &columns,
            Column::Title,
            "活动名称",
            excel_row,
            &mut row_issues,
        );
        let category = required_text(
            &range,
            relative_row,
            &columns,
            Column::Category,
            "活动类别",
            excel_row,
            &mut row_issues,
        );
        let level = parse_level(
            cell(&range, relative_row, &columns, Column::Level),
            excel_row,
            &mut row_issues,
        );
        let date = parse_date(
            cell(&range, relative_row, &columns, Column::Date),
            excel_row,
            &mut row_issues,
        );
        let score = parse_score(
            cell(&range, relative_row, &columns, Column::Score),
            excel_row,
            &mut row_issues,
        );

        let id = optional_text(cell(&range, relative_row, &columns, Column::Id));
        let remark =
            optional_text(cell(&range, relative_row, &columns, Column::Remark)).unwrap_or_default();
        let material_names =
            parse_material_names(cell(&range, relative_row, &columns, Column::MaterialNames));
        let material_count = parse_material_count(
            cell(&range, relative_row, &columns, Column::MaterialCount),
            material_names.len(),
            excel_row,
            &mut row_issues,
        );

        if row_issues.is_empty() {
            parsed.row_numbers.push(excel_row);
            parsed.rows.push(ExcelRecordRow {
                id,
                title: title.expect("必填字段已校验"),
                category: category.expect("必填字段已校验"),
                level: level.expect("必填字段已校验"),
                date: date.expect("必填字段已校验"),
                score: score.expect("必填字段已校验"),
                remark,
                material_count,
                material_names,
            });
        } else {
            parsed.issues.extend(row_issues);
        }
    }

    Ok(parsed)
}

fn write_detail_header(worksheet: &mut Worksheet) -> Result<(), String> {
    let format = header_format();
    for (column, header) in DETAIL_HEADERS.iter().enumerate() {
        worksheet
            .write_string_with_format(0, column as u16, *header, &format)
            .map_err(|error| format!("写入表头失败：{error}"))?;
    }
    let widths = [38, 28, 16, 14, 14, 12, 36, 12, 42];
    for (column, width) in widths.iter().enumerate() {
        worksheet
            .set_column_width(column as u16, *width)
            .map_err(|error| format!("设置列宽失败：{error}"))?;
    }
    worksheet
        .set_freeze_panes(1, 0)
        .map_err(|error| format!("冻结表头失败：{error}"))?;
    Ok(())
}

fn header_format() -> Format {
    Format::new()
        .set_bold()
        .set_font_color(Color::White)
        .set_background_color(Color::RGB(0x18794E))
        .set_border(FormatBorder::Thin)
        .set_align(FormatAlign::Center)
}

fn ensure_parent_exists(path: &Path) -> Result<(), String> {
    let parent = path.parent().ok_or_else(|| "导出路径无效".to_string())?;
    if !parent.as_os_str().is_empty() && !parent.exists() {
        return Err(format!("目标文件夹不存在：{}", parent.display()));
    }
    Ok(())
}

fn safe_text(value: &str) -> String {
    let value = value.trim_end_matches('\0');
    if matches!(value.chars().next(), Some('=' | '+' | '-' | '@')) {
        format!("'{value}")
    } else {
        value.to_string()
    }
}

fn unsanitize_text(value: &str) -> &str {
    value
        .strip_prefix('\'')
        .filter(|rest| matches!(rest.chars().next(), Some('=' | '+' | '-' | '@')))
        .unwrap_or(value)
}

fn level_display_name(level: &str) -> &str {
    match level.trim().to_ascii_lowercase().as_str() {
        "college" => "院级",
        "school" => "校级",
        "provincial" => "省级",
        "national" => "国家级",
        _ => level,
    }
}

fn collect_formula_cells(formulas: &Range<String>) -> HashSet<(u32, u32)> {
    let mut cells = HashSet::new();
    let Some((start_row, start_column)) = formulas.start() else {
        return cells;
    };
    for (row_index, row) in formulas.rows().enumerate() {
        for (column_index, formula) in row.iter().enumerate() {
            if !formula.trim().is_empty() {
                cells.insert((
                    start_row + row_index as u32,
                    start_column + column_index as u32,
                ));
            }
        }
    }
    cells
}

fn find_header_row(range: &Range<Data>) -> Option<(usize, HashMap<Column, usize>)> {
    let limit = range.height().min(20);
    let mut best: Option<(usize, HashMap<Column, usize>)> = None;
    for row_index in 0..limit {
        let mut columns = HashMap::new();
        for column_index in 0..range.width() {
            if let Some(text) = cell_to_text(range.get((row_index, column_index))) {
                if let Some(column) = header_column(&text) {
                    columns.entry(column).or_insert(column_index);
                }
            }
        }
        if columns.contains_key(&Column::Title)
            && best
                .as_ref()
                .is_none_or(|(_, current)| columns.len() > current.len())
        {
            best = Some((row_index, columns));
        }
    }
    best
}

fn header_column(header: &str) -> Option<Column> {
    let normalized: String = header
        .trim_start_matches('\u{feff}')
        .chars()
        .filter(|character| {
            !character.is_whitespace() && !matches!(character, '_' | '-' | '(' | ')' | '（' | '）')
        })
        .flat_map(char::to_lowercase)
        .collect();
    match normalized.as_str() {
        "id" | "记录id" | "recordid" | "记录编号" => Some(Column::Id),
        "活动名称" | "项目名称" | "名称" | "title" | "activity" | "activityname"
        | "recordtitle" => Some(Column::Title),
        "活动类别" | "活动类型" | "项目类别" | "类别" | "category" | "type" => {
            Some(Column::Category)
        }
        "综测级别" | "获奖级别" | "级别" | "等级" | "level" | "assessmentlevel" => {
            Some(Column::Level)
        }
        "日期" | "活动日期" | "获奖日期" | "时间" | "date" | "activitydate" => {
            Some(Column::Date)
        }
        "分数" | "综测分数" | "综测得分" | "得分" | "score" | "points" => {
            Some(Column::Score)
        }
        "备注" | "说明" | "remark" | "remarks" | "note" | "notes" => Some(Column::Remark),
        "材料数量" | "附件数量" | "证明材料数量" | "materialcount" | "attachmentcount" => {
            Some(Column::MaterialCount)
        }
        "材料名称"
        | "附件名称"
        | "证明材料"
        | "证明材料名称"
        | "材料名称分号分隔"
        | "materialnames"
        | "attachments" => Some(Column::MaterialNames),
        _ => None,
    }
}

fn row_is_empty(range: &Range<Data>, row: usize) -> bool {
    (0..range.width()).all(|column| match range.get((row, column)) {
        None | Some(Data::Empty) => true,
        Some(Data::String(value)) => value.trim().is_empty(),
        Some(_) => false,
    })
}

fn cell<'a>(
    range: &'a Range<Data>,
    row: usize,
    columns: &HashMap<Column, usize>,
    column: Column,
) -> Option<&'a Data> {
    columns
        .get(&column)
        .and_then(|column_index| range.get((row, *column_index)))
}

fn required_text(
    range: &Range<Data>,
    row: usize,
    columns: &HashMap<Column, usize>,
    column: Column,
    display_name: &str,
    excel_row: u32,
    issues: &mut Vec<ExcelImportIssue>,
) -> Option<String> {
    match optional_text(cell(range, row, columns, column)) {
        Some(value) => Some(value),
        None => {
            issues.push(ExcelImportIssue {
                row: excel_row,
                column: display_name.to_string(),
                message: format!("“{display_name}”不能为空"),
            });
            None
        }
    }
}

fn optional_text(cell: Option<&Data>) -> Option<String> {
    let value = cell_to_text(cell)?;
    let value = unsanitize_text(value.trim()).trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn cell_to_text(cell: Option<&Data>) -> Option<String> {
    match cell? {
        Data::Empty => None,
        Data::String(value) => Some(value.clone()),
        Data::Int(value) => Some(value.to_string()),
        Data::Float(value) => Some(format_number(*value)),
        Data::Bool(value) => Some(value.to_string()),
        Data::DateTime(value) => Some(format_number(value.as_f64())),
        Data::DateTimeIso(value) | Data::DurationIso(value) => Some(value.clone()),
        Data::Error(error) => Some(error.to_string()),
    }
}

fn parse_level(
    cell: Option<&Data>,
    row: u32,
    issues: &mut Vec<ExcelImportIssue>,
) -> Option<String> {
    let Some(value) = optional_text(cell) else {
        issues.push(ExcelImportIssue {
            row,
            column: "综测级别".to_string(),
            message: "“综测级别”不能为空".to_string(),
        });
        return None;
    };
    let normalized = value.to_ascii_lowercase();
    let level = match normalized.as_str() {
        "院级" | "学院级" | "college" => "college",
        "校级" | "学校级" | "school" | "university" => "school",
        "省级" | "provincial" | "province" => "provincial",
        "国家级" | "national" | "country" => "national",
        _ => {
            issues.push(ExcelImportIssue {
                row,
                column: "综测级别".to_string(),
                message: "级别必须是院级、校级、省级或国家级".to_string(),
            });
            return None;
        }
    };
    Some(level.to_string())
}

fn parse_date(cell: Option<&Data>, row: u32, issues: &mut Vec<ExcelImportIssue>) -> Option<String> {
    let parsed = match cell {
        Some(Data::Int(value)) => excel_serial_date(*value as f64),
        Some(Data::Float(value)) => excel_serial_date(*value),
        Some(Data::DateTime(value)) => excel_serial_date(value.as_f64()),
        Some(Data::DateTimeIso(value)) => parse_date_text(value),
        Some(Data::String(value)) => parse_date_text(unsanitize_text(value.trim())),
        _ => None,
    };
    match parsed {
        Some(date) => Some(date.format("%Y-%m-%d").to_string()),
        None => {
            issues.push(ExcelImportIssue {
                row,
                column: "日期".to_string(),
                message: "日期必须是有效的 YYYY-MM-DD 格式".to_string(),
            });
            None
        }
    }
}

fn parse_date_text(value: &str) -> Option<NaiveDate> {
    let date_part = value.split(['T', ' ']).next().unwrap_or(value);
    ["%Y-%m-%d", "%Y/%m/%d", "%Y.%m.%d"]
        .iter()
        .find_map(|format| NaiveDate::parse_from_str(date_part, format).ok())
}

fn excel_serial_date(serial: f64) -> Option<NaiveDate> {
    if !serial.is_finite() || !(1.0..=2_958_465.999_999).contains(&serial) {
        return None;
    }
    let whole_days = serial.floor() as i64;
    let adjusted_days = if whole_days < 60 {
        whole_days + 1
    } else {
        whole_days
    };
    NaiveDate::from_ymd_opt(1899, 12, 30)?.checked_add_signed(Duration::days(adjusted_days))
}

fn parse_score(
    cell: Option<&Data>,
    row: u32,
    issues: &mut Vec<ExcelImportIssue>,
) -> Option<String> {
    let result = match cell {
        Some(Data::Int(value)) if *value >= 0 => Some(value.to_string()),
        Some(Data::Float(value)) => normalize_numeric_score(*value),
        Some(Data::String(value)) => normalize_score_text(unsanitize_text(value.trim())),
        _ => None,
    };
    match result {
        Some(score) => Some(score),
        None => {
            issues.push(ExcelImportIssue {
                row,
                column: "分数".to_string(),
                message: "分数必须是非负数，且最多保留两位小数".to_string(),
            });
            None
        }
    }
}

fn normalize_numeric_score(value: f64) -> Option<String> {
    if !value.is_finite() || value < 0.0 {
        return None;
    }
    let cents = (value * 100.0).round();
    if (value * 100.0 - cents).abs() > 1e-7 || cents > i64::MAX as f64 {
        return None;
    }
    Some(format_cents(cents as i64))
}

fn normalize_score_text(value: &str) -> Option<String> {
    let value = value.strip_prefix('+').unwrap_or(value);
    if value.is_empty() || value.starts_with('-') {
        return None;
    }
    let mut parts = value.split('.');
    let integer = parts.next()?;
    let fraction = parts.next();
    if parts.next().is_some()
        || integer.is_empty()
        || !integer.chars().all(|character| character.is_ascii_digit())
    {
        return None;
    }
    let fraction = fraction.unwrap_or_default();
    if fraction.len() > 2 || !fraction.chars().all(|character| character.is_ascii_digit()) {
        return None;
    }
    let integer = integer.trim_start_matches('0');
    let integer = if integer.is_empty() { "0" } else { integer };
    let fraction = fraction.trim_end_matches('0');
    if fraction.is_empty() {
        Some(integer.to_string())
    } else {
        Some(format!("{integer}.{fraction}"))
    }
}

fn format_cents(cents: i64) -> String {
    let integer = cents / 100;
    let fraction = cents % 100;
    match fraction {
        0 => integer.to_string(),
        value if value % 10 == 0 => format!("{integer}.{}", value / 10),
        value => format!("{integer}.{value:02}"),
    }
}

fn format_number(value: f64) -> String {
    if value.fract().abs() < f64::EPSILON {
        format!("{value:.0}")
    } else {
        let value = format!("{value:.10}");
        value
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    }
}

fn parse_material_names(cell: Option<&Data>) -> Vec<String> {
    optional_text(cell)
        .map(|value| {
            value
                .split(['\n', '\r', ';', '；', '、', '|'])
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn parse_material_count(
    cell: Option<&Data>,
    fallback: usize,
    row: u32,
    issues: &mut Vec<ExcelImportIssue>,
) -> u32 {
    let parsed = match cell {
        None | Some(Data::Empty) => u32::try_from(fallback).ok(),
        Some(Data::Int(value)) => u32::try_from(*value).ok(),
        Some(Data::Float(value)) if value.is_finite() && *value >= 0.0 && value.fract() == 0.0 => {
            u32::try_from(*value as u64).ok()
        }
        Some(Data::String(value)) => unsanitize_text(value.trim()).parse::<u32>().ok(),
        _ => None,
    };
    match parsed {
        Some(value) => value,
        None => {
            issues.push(ExcelImportIssue {
                row,
                column: "材料数量".to_string(),
                message: "材料数量必须是非负整数".to_string(),
            });
            0
        }
    }
}

fn excel_column_name(mut column: u32) -> String {
    let mut name = String::new();
    loop {
        let remainder = (column % 26) as u8;
        name.insert(0, (b'A' + remainder) as char);
        if column < 26 {
            break;
        }
        column = column / 26 - 1;
    }
    name
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_xlsxwriter::Formula;
    use std::sync::atomic::{AtomicU64, Ordering};

    static FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temporary_xlsx(name: &str) -> std::path::PathBuf {
        let sequence = FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "zongce_excel_{}_{}_{}_{}.xlsx",
            name,
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default(),
            sequence
        ))
    }

    #[test]
    fn score_normalization_is_exact_to_two_decimals() {
        assert_eq!(normalize_score_text("0012.30"), Some("12.3".to_string()));
        assert_eq!(normalize_score_text("0.05"), Some("0.05".to_string()));
        assert_eq!(normalize_numeric_score(8.25), Some("8.25".to_string()));
        assert_eq!(normalize_score_text("1.234"), None);
        assert_eq!(normalize_score_text("-1"), None);
    }

    #[test]
    fn records_round_trip_and_formula_like_text_is_preserved() {
        let path = temporary_xlsx("round_trip");
        let rows = vec![ExcelRecordRow {
            id: Some("record-1".to_string()),
            title: "=SUM(A1:A2)".to_string(),
            category: "学科竞赛".to_string(),
            level: "provincial".to_string(),
            date: "2026-08-31".to_string(),
            score: "12.5".to_string(),
            remark: "+不会执行".to_string(),
            material_count: 2,
            material_names: vec!["证书.pdf".to_string(), "现场照片.png".to_string()],
        }];

        write_records(&path, &rows, &[("总分".to_string(), "12.5".to_string())]).unwrap();
        let parsed = parse_records(&path).unwrap();
        std::fs::remove_file(&path).ok();

        assert!(parsed.issues.is_empty(), "{:?}", parsed.issues);
        assert_eq!(parsed.rows, rows);
    }

    #[test]
    fn aliases_are_recognized_and_invalid_rows_report_issues() {
        let path = temporary_xlsx("aliases");
        let mut workbook = Workbook::new();
        {
            let worksheet = workbook.add_worksheet();
            worksheet.set_name("Sheet1").unwrap();
            for (column, header) in ["title", "type", "level", "date", "score"]
                .iter()
                .enumerate()
            {
                worksheet.write_string(0, column as u16, *header).unwrap();
            }
            for (column, value) in ["志愿活动", "志愿服务", "校级", "2025/09/01", "3.50"]
                .iter()
                .enumerate()
            {
                worksheet.write_string(1, column as u16, *value).unwrap();
            }
            worksheet.write_string(2, 0, "错误记录").unwrap();
            worksheet.write_string(2, 1, "其他").unwrap();
            worksheet.write_string(2, 2, "国际级").unwrap();
            worksheet.write_string(2, 3, "2025-02-30").unwrap();
            worksheet.write_string(2, 4, "1.234").unwrap();
        }
        workbook.save(&path).unwrap();

        let parsed = parse_records(&path).unwrap();
        std::fs::remove_file(&path).ok();

        assert_eq!(parsed.rows.len(), 1);
        assert_eq!(parsed.rows[0].level, "school");
        assert_eq!(parsed.rows[0].date, "2025-09-01");
        assert_eq!(parsed.rows[0].score, "3.5");
        assert_eq!(parsed.issues.len(), 3);
    }

    #[test]
    fn formula_cells_are_rejected() {
        let path = temporary_xlsx("formula");
        let mut workbook = Workbook::new();
        {
            let worksheet = workbook.add_worksheet();
            for (column, header) in ["活动名称", "活动类别", "综测级别", "日期", "分数"]
                .iter()
                .enumerate()
            {
                worksheet.write_string(0, column as u16, *header).unwrap();
            }
            for (column, value) in ["测试", "其他", "院级", "2026-08-31"].iter().enumerate() {
                worksheet.write_string(1, column as u16, *value).unwrap();
            }
            worksheet.write_formula(1, 4, Formula::new("=1+1")).unwrap();
        }
        workbook.save(&path).unwrap();

        let parsed = parse_records(&path).unwrap();
        std::fs::remove_file(&path).ok();

        assert!(parsed.rows.is_empty());
        assert_eq!(parsed.issues.len(), 1);
        assert!(parsed.issues[0].message.contains("公式"));
    }

    #[test]
    fn template_contains_a_parseable_empty_sheet() {
        let path = temporary_xlsx("template");
        write_template(&path).unwrap();
        let parsed = parse_records(&path).unwrap();
        std::fs::remove_file(&path).ok();

        assert!(parsed.rows.is_empty());
        assert!(parsed.issues.is_empty());
    }
}
