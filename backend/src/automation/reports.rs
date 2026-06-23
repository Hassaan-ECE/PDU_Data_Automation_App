use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use regex::Regex;
use serde::Serialize;
use thiserror::Error;
use walkdir::WalkDir;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

pub const TEMPLATE_DIR: &str = "C:/PDU500/00_Template";
pub const MAIN_TEMPLATE_NAME: &str = "PDUD500442AM088_Test Report_0.2CT_Rev02_SN##.xlsx";
pub const PRINT_TEMPLATE_NAME: &str = "PDUD500442AA088_0.2CT Test Report Print.xlsx";
pub const MAIN_REPORT_PREFIX: &str = "PDUD500442AM088_Test Report_0.2CT_Rev02_";
pub const MAIN_REPORT_SN_PREFIX: &str = "PDUD500442AM088_Test Report_0.2CT_Rev02_SN";
pub const PRINT_REPORT_PREFIX: &str = "PDUD500442AA088";
pub const FINAL_OPERATOR_SHEET: &str = "Test Report #2";
pub const FINAL_OPERATOR_CELL: &str = "E39";

#[derive(Debug, Error)]
pub enum ReportError {
    #[error("report I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("xlsx zip operation failed: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("unit folder does not exist: {0}")]
    UnitFolderMissing(String),
    #[error("no main report workbook found under {0}")]
    MainReportMissing(String),
    #[error("no print report workbook found under {0}")]
    PrintReportMissing(String),
    #[error("template folder does not exist: {0}")]
    TemplateFolderMissing(String),
    #[error("template file does not exist: {0}")]
    TemplateMissing(String),
    #[error("workbook is missing required internal file: {0}")]
    WorkbookPartMissing(String),
    #[error("workbook sheet not found: {0}")]
    SheetMissing(String),
    #[error("invalid cell reference: {0}")]
    InvalidCell(String),
    #[error("worksheet XML is missing sheetData for sheet: {0}")]
    SheetDataMissing(String),
    #[error("workbook XML could not be decoded as UTF-8: {0}")]
    InvalidUtf8(String),
}

pub type ReportResult<T> = Result<T, ReportError>;

#[derive(Debug, Clone, Serialize)]
pub struct ReportSetup {
    pub serial_number: Option<String>,
    pub report_path: Option<String>,
    pub print_report_path: Option<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default)]
struct ReportDiscovery {
    main_report: Option<PathBuf>,
    print_report: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub enum CellValue {
    Number(f64),
    Text(String),
}

#[derive(Debug, Clone)]
pub struct CellUpdate {
    pub sheet: String,
    pub cell: String,
    pub value: CellValue,
}

impl CellUpdate {
    pub fn number(sheet: impl Into<String>, cell: impl Into<String>, value: f64) -> Self {
        Self {
            sheet: sheet.into(),
            cell: cell.into(),
            value: CellValue::Number(value),
        }
    }

    pub fn text(
        sheet: impl Into<String>,
        cell: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        Self {
            sheet: sheet.into(),
            cell: cell.into(),
            value: CellValue::Text(value.into()),
        }
    }
}

pub fn setup_reports(unit_folder: &Path) -> ReportResult<ReportSetup> {
    setup_reports_with_serial_number(unit_folder, None)
}

pub fn setup_reports_with_serial_number(
    unit_folder: &Path,
    serial_number_override: Option<&str>,
) -> ReportResult<ReportSetup> {
    if !unit_folder.is_dir() {
        return Err(ReportError::UnitFolderMissing(
            unit_folder.display().to_string(),
        ));
    }

    let mut warnings = Vec::new();
    let serial_number = serial_number_override
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| extract_serial_number(unit_folder));
    let template_root = Path::new(TEMPLATE_DIR);
    let mut reports = discover_reports(unit_folder);
    let mut touched_reports = Vec::<PathBuf>::new();

    if !template_root.is_dir() {
        warnings.push(format!("template folder not found: {TEMPLATE_DIR}"));
    } else {
        touched_reports.extend(copy_templates(
            unit_folder,
            template_root,
            &reports,
            serial_number.as_deref(),
            &mut warnings,
        )?);
        reports = discover_reports(unit_folder);
    }

    if let Some(touched_report) = ensure_report_named_correctly(
        unit_folder,
        &reports,
        serial_number.as_deref(),
        &mut warnings,
    )? {
        touched_reports.push(touched_report);
    }

    let mut repaired = HashSet::<PathBuf>::new();
    for report_path in touched_reports {
        if repaired.insert(report_path.clone()) {
            repair_workbook(&report_path)?;
        }
    }

    reports = discover_reports(unit_folder);

    Ok(ReportSetup {
        serial_number,
        report_path: reports.main_report.map(|path| path.display().to_string()),
        print_report_path: reports.print_report.map(|path| path.display().to_string()),
        warnings,
    })
}

pub fn write_transformer_serial_number(
    unit_folder: &Path,
    transformer_serial_number: &str,
) -> ReportResult<PathBuf> {
    let report_path = require_main_report(unit_folder)?;

    patch_workbook(
        &report_path,
        &[CellUpdate::text(
            "Test Summary",
            "D1",
            transformer_serial_number.to_string(),
        )],
    )?;

    Ok(report_path)
}

pub fn write_final_operator_name(unit_folder: &Path, operator_name: &str) -> ReportResult<PathBuf> {
    if !unit_folder.is_dir() {
        return Err(ReportError::UnitFolderMissing(
            unit_folder.display().to_string(),
        ));
    }

    let report_path = require_print_report(unit_folder)?;

    patch_workbook(
        &report_path,
        &[CellUpdate::text(
            FINAL_OPERATOR_SHEET,
            FINAL_OPERATOR_CELL,
            operator_name.to_string(),
        )],
    )?;

    Ok(report_path)
}

pub fn inspect_reports(unit_folder: &Path) -> ReportResult<ReportSetup> {
    if !unit_folder.is_dir() {
        return Err(ReportError::UnitFolderMissing(
            unit_folder.display().to_string(),
        ));
    }

    let reports = discover_reports(unit_folder);

    Ok(ReportSetup {
        serial_number: extract_serial_number(unit_folder),
        report_path: reports.main_report.map(|path| path.display().to_string()),
        print_report_path: reports.print_report.map(|path| path.display().to_string()),
        warnings: Vec::new(),
    })
}

pub fn require_main_report(unit_folder: &Path) -> ReportResult<PathBuf> {
    find_main_report(unit_folder)
        .ok_or_else(|| ReportError::MainReportMissing(unit_folder.display().to_string()))
}

pub fn require_print_report(unit_folder: &Path) -> ReportResult<PathBuf> {
    find_print_report(unit_folder)
        .ok_or_else(|| ReportError::PrintReportMissing(unit_folder.display().to_string()))
}

pub fn extract_serial_number(unit_folder: &Path) -> Option<String> {
    let folder_name = unit_folder.file_name()?.to_string_lossy();
    let folder_date = Regex::new(r"^(\d{6,})_(\d{8})$").expect("serial/date regex is valid");

    if let Some(captures) = folder_date.captures(&folder_name) {
        return captures.get(1).map(|match_| match_.as_str().to_string());
    }

    if folder_name.chars().all(|ch| ch.is_ascii_digit()) && folder_name.len() >= 6 {
        return Some(folder_name.to_string());
    }

    let metadata_re =
        Regex::new(r"(?i)(?:sn|serial\s*number)[:\s=]*(\d{6,})").expect("metadata regex is valid");

    for file_name in ["SN.txt", "serial_number.txt", "info.txt", "metadata.txt"] {
        let path = unit_folder.join(file_name);

        if !path.is_file() {
            continue;
        }

        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };

        if let Some(captures) = metadata_re.captures(&content) {
            return captures.get(1).map(|match_| match_.as_str().to_string());
        }
    }

    let path_re = Regex::new(r"SN(\d{6,})").expect("path serial regex is valid");
    let path_text = unit_folder.display().to_string();

    if let Some(captures) = path_re.captures(&path_text) {
        return captures.get(1).map(|match_| match_.as_str().to_string());
    }

    let digits_re = Regex::new(r"(\d{8,})").expect("digits regex is valid");

    for entry in WalkDir::new(unit_folder)
        .max_depth(2)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
    {
        let name = entry.file_name().to_string_lossy();

        if let Some(captures) = path_re.captures(&name) {
            return captures.get(1).map(|match_| match_.as_str().to_string());
        }

        if let Some(captures) = digits_re.captures(&name) {
            return captures.get(1).map(|match_| match_.as_str().to_string());
        }
    }

    None
}

pub fn patch_workbook(path: &Path, updates: &[CellUpdate]) -> ReportResult<()> {
    if updates.is_empty() {
        return Ok(());
    }

    rewrite_workbook(path, updates, false)
}

pub fn repair_workbook(path: &Path) -> ReportResult<()> {
    rewrite_workbook(path, &[], true)
}

fn rewrite_workbook(
    path: &Path,
    updates: &[CellUpdate],
    repair_all_sheets: bool,
) -> ReportResult<()> {
    let mut archive = ZipArchive::new(File::open(path)?)?;
    let mut entries = Vec::new();

    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let name = entry.name().to_string();
        let is_dir = entry.is_dir();
        let mut data = Vec::new();

        if !is_dir {
            entry.read_to_end(&mut data)?;
        }

        entries.push(ZipEntry { name, is_dir, data });
    }

    let workbook_xml = entry_text(&entries, "xl/workbook.xml")?;
    let rels_xml = entry_text(&entries, "xl/_rels/workbook.xml.rels")?;
    let sheet_paths = sheet_paths_by_name(&workbook_xml, &rels_xml);
    let mut replacements = HashMap::<String, Vec<u8>>::new();
    let mut removed_entries = HashSet::<String>::new();

    let mut grouped = HashMap::<String, Vec<&CellUpdate>>::new();

    for update in updates {
        grouped
            .entry(update.sheet.clone())
            .or_default()
            .push(update);
    }

    let mut grouped_by_path = HashMap::<String, (String, Vec<&CellUpdate>)>::new();
    let mut sheet_paths_to_rewrite = HashSet::<String>::new();

    if repair_all_sheets {
        sheet_paths_to_rewrite.extend(sheet_paths.values().cloned());
    }

    for (sheet_name, sheet_updates) in grouped {
        let sheet_path = sheet_paths
            .get(&sheet_name)
            .ok_or_else(|| ReportError::SheetMissing(sheet_name.clone()))?;
        grouped_by_path.insert(sheet_path.clone(), (sheet_name, sheet_updates));
        sheet_paths_to_rewrite.insert(sheet_path.clone());
    }

    for sheet_path in sheet_paths_to_rewrite {
        let mut xml = expand_shared_formulas(&entry_text(&entries, &sheet_path)?);

        if let Some((sheet_name, sheet_updates)) = grouped_by_path.get(&sheet_path) {
            for update in sheet_updates {
                xml = patch_cell_xml(&xml, sheet_name, &update.cell, &update.value)?;
            }
        }

        replacements.insert(sheet_path, xml.into_bytes());
    }

    replacements.insert(
        "xl/workbook.xml".to_string(),
        force_full_recalculation(&workbook_xml).into_bytes(),
    );
    replacements.insert(
        "xl/_rels/workbook.xml.rels".to_string(),
        remove_calc_chain_relationship(&rels_xml).into_bytes(),
    );

    if let Ok(content_types_xml) = entry_text(&entries, "[Content_Types].xml") {
        replacements.insert(
            "[Content_Types].xml".to_string(),
            remove_calc_chain_content_type(&content_types_xml).into_bytes(),
        );
    }

    removed_entries.insert("xl/calcChain.xml".to_string());

    let temp_path = path.with_extension("xlsx.tmp");
    write_repacked_zip(&entries, &replacements, &removed_entries, &temp_path)?;
    fs::remove_file(path)?;
    fs::rename(temp_path, path)?;

    Ok(())
}

fn copy_templates(
    unit_folder: &Path,
    template_root: &Path,
    reports: &ReportDiscovery,
    serial_number: Option<&str>,
    warnings: &mut Vec<String>,
) -> ReportResult<Vec<PathBuf>> {
    let mut touched_reports = Vec::new();
    let main_template = template_root.join(MAIN_TEMPLATE_NAME);
    let print_template = template_root.join(PRINT_TEMPLATE_NAME);

    if !main_template.is_file() {
        warnings.push(format!(
            "main report template missing: {}",
            main_template.display()
        ));
    } else if reports.main_report.is_none() {
        let target_name = serial_number
            .map(|serial| format!("{MAIN_REPORT_PREFIX}SN{serial}.xlsx"))
            .unwrap_or_else(|| MAIN_TEMPLATE_NAME.to_string());
        let target_path = unit_folder.join(target_name);

        fs::copy(&main_template, &target_path)?;

        if let Some(serial) = serial_number {
            patch_workbook(
                &target_path,
                &[CellUpdate::text("Test Summary", "B2", serial.to_string())],
            )?;
        }

        touched_reports.push(target_path);
    }

    if !print_template.is_file() {
        warnings.push(format!(
            "print report template missing: {}",
            print_template.display()
        ));
    } else if reports.print_report.is_none() {
        let target_path = unit_folder.join(PRINT_TEMPLATE_NAME);
        fs::copy(&print_template, &target_path)?;
        touched_reports.push(target_path);
    }

    Ok(touched_reports)
}

fn ensure_report_named_correctly(
    unit_folder: &Path,
    reports: &ReportDiscovery,
    serial_number: Option<&str>,
    warnings: &mut Vec<String>,
) -> ReportResult<Option<PathBuf>> {
    let Some(serial_number) = serial_number else {
        return Ok(None);
    };

    let target = unit_folder.join(format!("{MAIN_REPORT_PREFIX}SN{serial_number}.xlsx"));

    if target.exists() {
        return Ok(None);
    }

    let Some(source) = reports.main_report.clone().or_else(|| {
        let template_copy = unit_folder.join(MAIN_TEMPLATE_NAME);
        template_copy.exists().then_some(template_copy)
    }) else {
        warnings.push(format!(
            "main report was not found and could not be named for serial {serial_number}"
        ));
        return Ok(None);
    };

    if source == target {
        return Ok(None);
    }

    fs::copy(source, &target)?;
    patch_workbook(
        &target,
        &[CellUpdate::text(
            "Test Summary",
            "B2",
            serial_number.to_string(),
        )],
    )?;

    Ok(Some(target))
}

fn find_main_report(unit_folder: &Path) -> Option<PathBuf> {
    discover_reports(unit_folder).main_report
}

fn find_print_report(unit_folder: &Path) -> Option<PathBuf> {
    discover_reports(unit_folder).print_report
}

fn discover_reports(root: &Path) -> ReportDiscovery {
    let mut main_sn = None;
    let mut main_prefixed = None;
    let mut main_any = None;
    let mut print_report = None;

    for entry in WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
    {
        let path = entry.into_path();
        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };

        if !file_name.ends_with(".xlsx") {
            continue;
        }

        let Ok(modified) = path.metadata().and_then(|metadata| metadata.modified()) else {
            continue;
        };

        if file_name.starts_with(MAIN_REPORT_SN_PREFIX) {
            retain_latest(&mut main_sn, path.clone(), modified);
        } else if file_name.starts_with(MAIN_REPORT_PREFIX) {
            retain_latest(&mut main_prefixed, path.clone(), modified);
        } else if file_name.starts_with("PDUD500442AM088") {
            retain_latest(&mut main_any, path.clone(), modified);
        }

        if file_name.starts_with(PRINT_REPORT_PREFIX) {
            retain_latest(&mut print_report, path, modified);
        }
    }

    ReportDiscovery {
        main_report: main_sn.or(main_prefixed).or(main_any).map(|(path, _)| path),
        print_report: print_report.map(|(path, _)| path),
    }
}

fn retain_latest(current: &mut Option<(PathBuf, SystemTime)>, path: PathBuf, modified: SystemTime) {
    if current
        .as_ref()
        .map_or(true, |(_, current_modified)| modified > *current_modified)
    {
        *current = Some((path, modified));
    }
}

#[derive(Debug)]
struct ZipEntry {
    name: String,
    is_dir: bool,
    data: Vec<u8>,
}

fn entry_text(entries: &[ZipEntry], name: &str) -> ReportResult<String> {
    let entry = entries
        .iter()
        .find(|entry| entry.name == name)
        .ok_or_else(|| ReportError::WorkbookPartMissing(name.to_string()))?;

    String::from_utf8(entry.data.clone()).map_err(|_| ReportError::InvalidUtf8(name.to_string()))
}

fn write_repacked_zip(
    entries: &[ZipEntry],
    replacements: &HashMap<String, Vec<u8>>,
    removed_entries: &HashSet<String>,
    target: &Path,
) -> ReportResult<()> {
    let file = File::create(target)?;
    let mut writer = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    for entry in entries {
        if removed_entries.contains(&entry.name) {
            continue;
        }

        if entry.is_dir {
            writer.add_directory(&entry.name, options)?;
            continue;
        }

        writer.start_file(&entry.name, options)?;
        let data = replacements.get(&entry.name).unwrap_or(&entry.data);
        writer.write_all(data)?;
    }

    writer.finish()?;
    Ok(())
}

fn force_full_recalculation(workbook_xml: &str) -> String {
    let calc_pr_re = Regex::new(r#"<calcPr\b[^>]*/?>"#).expect("calcPr regex is valid");

    if let Some(match_) = calc_pr_re.find(workbook_xml) {
        let tag = match_.as_str();
        let updated = set_xml_attr(tag, "calcMode", "auto");
        let updated = set_xml_attr(&updated, "fullCalcOnLoad", "1");
        let updated = set_xml_attr(&updated, "forceFullCalc", "1");
        let mut out = String::with_capacity(workbook_xml.len() + updated.len());

        out.push_str(&workbook_xml[..match_.start()]);
        out.push_str(&updated);
        out.push_str(&workbook_xml[match_.end()..]);
        return out;
    }

    let calc_pr = r#"<calcPr calcMode="auto" fullCalcOnLoad="1" forceFullCalc="1"/>"#;

    if let Some(insert_at) = workbook_xml.find("</workbook>") {
        let mut out = String::with_capacity(workbook_xml.len() + calc_pr.len());

        out.push_str(&workbook_xml[..insert_at]);
        out.push_str(calc_pr);
        out.push_str(&workbook_xml[insert_at..]);
        out
    } else {
        workbook_xml.to_string()
    }
}

fn remove_calc_chain_relationship(rels_xml: &str) -> String {
    let rel_re = Regex::new(
        r#"<Relationship\b[^>]*(?:Type="[^"]*/calcChain"|Target="calcChain\.xml")[^>]*/>"#,
    )
    .expect("calc chain relationship regex is valid");

    rel_re.replace_all(rels_xml, "").into_owned()
}

fn remove_calc_chain_content_type(content_types_xml: &str) -> String {
    let override_re = Regex::new(r#"<Override\b[^>]*PartName="/xl/calcChain\.xml"[^>]*/>"#)
        .expect("calc chain content type regex is valid");

    override_re.replace_all(content_types_xml, "").into_owned()
}

#[derive(Debug, Clone)]
struct SharedFormulaMaster {
    cell: String,
    formula: String,
}

#[derive(Debug, Clone)]
struct FormulaSegment<'a> {
    start: usize,
    end: usize,
    start_tag: &'a str,
    body: Option<&'a str>,
}

fn expand_shared_formulas(xml: &str) -> String {
    if !xml.contains(r#"t="shared""#) {
        return xml.to_string();
    }

    let cell_re = Regex::new(r#"(?s)<c\b[^>]*\br="([A-Z]+[0-9]+)"[^>]*>.*?</c>"#)
        .expect("cell regex is valid");
    let mut masters = HashMap::<String, SharedFormulaMaster>::new();

    for captures in cell_re.captures_iter(xml) {
        let Some(cell_match) = captures.get(0) else {
            continue;
        };
        let Some(cell_ref) = captures.get(1).map(|match_| match_.as_str()) else {
            continue;
        };
        let cell_xml = cell_match.as_str();
        let Some(formula) = find_formula_segment(cell_xml) else {
            continue;
        };

        if extract_attr(formula.start_tag, "t").as_deref() != Some("shared") {
            continue;
        }

        let Some(si) = extract_attr(formula.start_tag, "si") else {
            continue;
        };
        let Some(body) = formula.body.filter(|body| !body.trim().is_empty()) else {
            continue;
        };

        masters.insert(
            si,
            SharedFormulaMaster {
                cell: cell_ref.to_string(),
                formula: body.to_string(),
            },
        );
    }

    let mut out = String::with_capacity(xml.len());
    let mut cursor = 0usize;

    for captures in cell_re.captures_iter(xml) {
        let Some(cell_match) = captures.get(0) else {
            continue;
        };
        let Some(cell_ref) = captures.get(1).map(|match_| match_.as_str()) else {
            continue;
        };

        out.push_str(&xml[cursor..cell_match.start()]);
        out.push_str(&expand_shared_formula_in_cell(
            cell_match.as_str(),
            cell_ref,
            &masters,
        ));
        cursor = cell_match.end();
    }

    out.push_str(&xml[cursor..]);
    out
}

fn expand_shared_formula_in_cell(
    cell_xml: &str,
    cell_ref: &str,
    masters: &HashMap<String, SharedFormulaMaster>,
) -> String {
    let Some(formula) = find_formula_segment(cell_xml) else {
        return cell_xml.to_string();
    };

    if extract_attr(formula.start_tag, "t").as_deref() != Some("shared") {
        return cell_xml.to_string();
    }

    let Some(si) = extract_attr(formula.start_tag, "si") else {
        return cell_xml.to_string();
    };
    let Some(master) = masters.get(&si) else {
        let mut out = String::with_capacity(cell_xml.len());

        out.push_str(&cell_xml[..formula.start]);
        out.push_str(&cell_xml[formula.end..]);
        return out;
    };

    let explicit_formula = if let Some(body) = formula.body.filter(|body| !body.trim().is_empty()) {
        body.to_string()
    } else {
        translate_shared_formula(&master.formula, &master.cell, cell_ref)
    };

    let replacement = format!("<f>{explicit_formula}</f>");
    let mut out = String::with_capacity(cell_xml.len() + replacement.len());

    out.push_str(&cell_xml[..formula.start]);
    out.push_str(&replacement);
    out.push_str(&cell_xml[formula.end..]);
    out
}

fn find_formula_segment(xml: &str) -> Option<FormulaSegment<'_>> {
    let start = xml.find("<f")?;
    let open_end = start + xml[start..].find('>')? + 1;
    let start_tag = &xml[start..open_end];

    if start_tag.trim_end().ends_with("/>") {
        return Some(FormulaSegment {
            start,
            end: open_end,
            start_tag,
            body: None,
        });
    }

    let close_start = open_end + xml[open_end..].find("</f>")?;
    let end = close_start + "</f>".len();

    Some(FormulaSegment {
        start,
        end,
        start_tag,
        body: Some(&xml[open_end..close_start]),
    })
}

fn translate_shared_formula(formula: &str, master_cell: &str, target_cell: &str) -> String {
    let Ok((master_column, master_row)) = split_cell_reference(master_cell) else {
        return formula.to_string();
    };
    let Ok((target_column, target_row)) = split_cell_reference(target_cell) else {
        return formula.to_string();
    };
    let Some(master_column_number) = column_name_to_number(&master_column) else {
        return formula.to_string();
    };
    let Some(target_column_number) = column_name_to_number(&target_column) else {
        return formula.to_string();
    };

    let column_delta = i64::from(target_column_number) - i64::from(master_column_number);
    let row_delta = i64::from(target_row) - i64::from(master_row);
    let cell_ref_re =
        Regex::new(r#"\$?[A-Z]{1,3}\$?\d+"#).expect("formula cell reference regex is valid");
    let mut out = String::with_capacity(formula.len());
    let mut cursor = 0usize;

    for match_ in cell_ref_re.find_iter(formula) {
        out.push_str(&formula[cursor..match_.start()]);
        let token = match_.as_str();

        if should_skip_formula_reference(formula, match_.start(), match_.end()) {
            out.push_str(token);
        } else if let Some(shifted) = shift_formula_reference(token, column_delta, row_delta) {
            out.push_str(&shifted);
        } else {
            out.push_str(token);
        }

        cursor = match_.end();
    }

    out.push_str(&formula[cursor..]);
    out
}

fn should_skip_formula_reference(formula: &str, start: usize, end: usize) -> bool {
    let previous = formula[..start].chars().next_back();
    let next = formula[end..].chars().next();

    if previous.is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '.') {
        return true;
    }

    next.is_some_and(|ch| ch == '(' || ch.is_ascii_alphanumeric() || ch == '_' || ch == '.')
}

fn shift_formula_reference(token: &str, column_delta: i64, row_delta: i64) -> Option<String> {
    let bytes = token.as_bytes();
    let mut index = 0usize;
    let absolute_column = bytes.get(index) == Some(&b'$');

    if absolute_column {
        index += 1;
    }

    let column_start = index;

    while bytes.get(index).is_some_and(u8::is_ascii_uppercase) {
        index += 1;
    }

    if column_start == index {
        return None;
    }

    let column = &token[column_start..index];
    let absolute_row = bytes.get(index) == Some(&b'$');

    if absolute_row {
        index += 1;
    }

    let row_start = index;

    while bytes.get(index).is_some_and(u8::is_ascii_digit) {
        index += 1;
    }

    if row_start == index || index != token.len() {
        return None;
    }

    let column_number = column_name_to_number(column)?;
    let row_number = token[row_start..].parse::<i64>().ok()?;
    let shifted_column = if absolute_column {
        i64::from(column_number)
    } else {
        i64::from(column_number) + column_delta
    };
    let shifted_row = if absolute_row {
        row_number
    } else {
        row_number + row_delta
    };

    if shifted_column <= 0 || shifted_row <= 0 {
        return None;
    }

    let mut out = String::with_capacity(token.len() + 2);

    if absolute_column {
        out.push('$');
    }

    out.push_str(&column_number_to_name(shifted_column as u32)?);

    if absolute_row {
        out.push('$');
    }

    out.push_str(&shifted_row.to_string());
    Some(out)
}

fn set_xml_attr(tag: &str, attr: &str, value: &str) -> String {
    let attr_re = Regex::new(&format!(r#"\b{}="[^"]*""#, regex::escape(attr)))
        .expect("attribute regex is valid");

    if attr_re.is_match(tag) {
        return attr_re
            .replace(tag, format!(r#"{attr}="{}""#, escape_xml_attr(value)))
            .into_owned();
    }

    if let Some(insert_at) = tag.rfind("/>") {
        let mut out = String::with_capacity(tag.len() + attr.len() + value.len() + 4);

        out.push_str(&tag[..insert_at]);
        out.push(' ');
        out.push_str(attr);
        out.push_str("=\"");
        out.push_str(&escape_xml_attr(value));
        out.push('"');
        out.push_str(&tag[insert_at..]);
        return out;
    }

    if let Some(insert_at) = tag.rfind('>') {
        let mut out = String::with_capacity(tag.len() + attr.len() + value.len() + 4);

        out.push_str(&tag[..insert_at]);
        out.push(' ');
        out.push_str(attr);
        out.push_str("=\"");
        out.push_str(&escape_xml_attr(value));
        out.push('"');
        out.push_str(&tag[insert_at..]);
        return out;
    }

    tag.to_string()
}

fn sheet_paths_by_name(workbook_xml: &str, rels_xml: &str) -> HashMap<String, String> {
    let sheet_re = Regex::new(r#"<sheet\b[^>]*\bname="([^"]+)"[^>]*\br:id="([^"]+)""#)
        .expect("sheet regex is valid");
    let rel_re = Regex::new(r#"<Relationship\b[^>]*\bId="([^"]+)"[^>]*\bTarget="([^"]+)""#)
        .expect("relationship regex is valid");
    let rel_targets = rel_re
        .captures_iter(rels_xml)
        .filter_map(|captures| {
            Some((
                captures.get(1)?.as_str().to_string(),
                captures.get(2)?.as_str().to_string(),
            ))
        })
        .collect::<HashMap<_, _>>();
    let mut paths = HashMap::new();

    for captures in sheet_re.captures_iter(workbook_xml) {
        let Some(sheet_name) = captures
            .get(1)
            .map(|match_| decode_xml_attr(match_.as_str()))
        else {
            continue;
        };
        let Some(rel_id) = captures.get(2).map(|match_| match_.as_str()) else {
            continue;
        };
        let Some(target) = rel_targets.get(rel_id) else {
            continue;
        };

        let normalized = if target.starts_with('/') {
            target.trim_start_matches('/').to_string()
        } else if target.starts_with("xl/") {
            target.to_string()
        } else {
            format!("xl/{target}")
        };

        paths.insert(sheet_name, normalized);
    }

    paths
}

fn patch_cell_xml(
    xml: &str,
    sheet_name: &str,
    cell: &str,
    value: &CellValue,
) -> ReportResult<String> {
    let (column, row_number) = split_cell_reference(cell)?;

    if let Some(row_bounds) = find_row_bounds(xml, row_number) {
        if row_bounds.self_closing {
            let replacement = format!(
                "<row r=\"{row_number}\">{}</row>",
                render_cell(cell, None, value)
            );
            let mut out = String::with_capacity(xml.len() + replacement.len());
            out.push_str(&xml[..row_bounds.start]);
            out.push_str(&replacement);
            out.push_str(&xml[row_bounds.end..]);
            return Ok(out);
        }

        if let Some(cell_bounds) =
            find_cell_bounds(xml, row_bounds.open_end, row_bounds.close_start, cell)
        {
            let style = extract_attr(&xml[cell_bounds.start..cell_bounds.open_end], "s");
            let replacement = render_cell(cell, style.as_deref(), value);
            let mut out = String::with_capacity(xml.len() + replacement.len());
            out.push_str(&xml[..cell_bounds.start]);
            out.push_str(&replacement);
            out.push_str(&xml[cell_bounds.end..]);
            return Ok(out);
        }

        let replacement = render_cell(cell, None, value);
        let insert_at =
            cell_insert_position(xml, row_bounds.open_end, row_bounds.close_start, cell)
                .unwrap_or(row_bounds.close_start);
        let mut out = String::with_capacity(xml.len() + replacement.len());
        out.push_str(&xml[..insert_at]);
        out.push_str(&replacement);
        out.push_str(&xml[insert_at..]);
        return Ok(out);
    }

    let Some(sheet_data_end) = xml.find("</sheetData>") else {
        return Err(ReportError::SheetDataMissing(sheet_name.to_string()));
    };

    let row_xml = format!(
        "<row r=\"{row_number}\" spans=\"{column}:{column}\">{}</row>",
        render_cell(cell, None, value)
    );
    let insert_at = row_insert_position(xml, row_number).unwrap_or(sheet_data_end);
    let mut out = String::with_capacity(xml.len() + row_xml.len());
    out.push_str(&xml[..insert_at]);
    out.push_str(&row_xml);
    out.push_str(&xml[insert_at..]);
    Ok(out)
}

#[derive(Debug, Clone, Copy)]
struct RowBounds {
    start: usize,
    open_end: usize,
    close_start: usize,
    end: usize,
    self_closing: bool,
}

#[derive(Debug, Clone, Copy)]
struct CellBounds {
    start: usize,
    open_end: usize,
    end: usize,
}

fn find_row_bounds(xml: &str, row_number: u32) -> Option<RowBounds> {
    let attr = format!("r=\"{row_number}\"");
    let mut search_at = 0usize;

    while let Some(relative_start) = xml[search_at..].find("<row") {
        let start = search_at + relative_start;
        let open_end = start + xml[start..].find('>')? + 1;
        let start_tag = &xml[start..open_end];
        search_at = open_end;

        if !start_tag.contains(&attr) {
            continue;
        }

        let self_closing = start_tag.trim_end().ends_with("/>");

        if self_closing {
            return Some(RowBounds {
                start,
                open_end,
                close_start: open_end,
                end: open_end,
                self_closing,
            });
        }

        let close_relative = xml[open_end..].find("</row>")?;
        let close_start = open_end + close_relative;
        let end = close_start + "</row>".len();

        return Some(RowBounds {
            start,
            open_end,
            close_start,
            end,
            self_closing,
        });
    }

    None
}

fn find_cell_bounds(xml: &str, start_at: usize, end_at: usize, cell: &str) -> Option<CellBounds> {
    let attr = format!("r=\"{cell}\"");
    let mut search_at = start_at;

    while search_at < end_at {
        let relative_start = xml[search_at..end_at].find("<c")?;
        let start = search_at + relative_start;
        let open_end = start + xml[start..end_at].find('>')? + 1;
        let start_tag = &xml[start..open_end];
        search_at = open_end;

        if !start_tag.contains(&attr) {
            continue;
        }

        let end = if start_tag.trim_end().ends_with("/>") {
            open_end
        } else {
            let close_relative = xml[open_end..end_at].find("</c>")?;
            open_end + close_relative + "</c>".len()
        };

        return Some(CellBounds {
            start,
            open_end,
            end,
        });
    }

    None
}

fn row_insert_position(xml: &str, row_number: u32) -> Option<usize> {
    let row_re = Regex::new(r#"<row\b[^>]*\br="(\d+)""#).expect("row insert regex is valid");

    let position = row_re.captures_iter(xml).find_map(|captures| {
        let candidate = captures.get(1)?.as_str().parse::<u32>().ok()?;

        if candidate > row_number {
            captures.get(0).map(|match_| match_.start())
        } else {
            None
        }
    });

    position
}

fn cell_insert_position(xml: &str, start_at: usize, end_at: usize, cell: &str) -> Option<usize> {
    let (column, _) = split_cell_reference(cell).ok()?;
    let target_index = column_name_to_number(&column)?;
    let cell_re = Regex::new(r#"<c\b[^>]*\br="([A-Z]+)\d+""#).expect("cell insert regex is valid");
    let row_xml = &xml[start_at..end_at];

    let position = cell_re.captures_iter(row_xml).find_map(|captures| {
        let column = captures.get(1)?.as_str();
        let candidate_index = column_name_to_number(column)?;

        if candidate_index > target_index {
            captures.get(0).map(|match_| start_at + match_.start())
        } else {
            None
        }
    });

    position
}

fn column_name_to_number(column: &str) -> Option<u32> {
    let trimmed = column.trim();

    if trimmed.is_empty() {
        return None;
    }

    let mut number = 0u32;

    for byte in trimmed.bytes() {
        if !byte.is_ascii_uppercase() {
            return None;
        }

        number = number * 26 + u32::from(byte - b'A' + 1);
    }

    Some(number)
}

fn column_number_to_name(mut number: u32) -> Option<String> {
    if number == 0 {
        return None;
    }

    let mut bytes = Vec::new();

    while number > 0 {
        number -= 1;
        bytes.push(b'A' + (number % 26) as u8);
        number /= 26;
    }

    bytes.reverse();
    String::from_utf8(bytes).ok()
}

fn render_cell(cell: &str, style: Option<&str>, value: &CellValue) -> String {
    let style = style
        .filter(|style| !style.trim().is_empty())
        .map(|style| format!(" s=\"{}\"", escape_xml_attr(style)))
        .unwrap_or_default();

    match value {
        CellValue::Number(number) => {
            format!(
                "<c r=\"{}\"{}><v>{}</v></c>",
                escape_xml_attr(cell),
                style,
                number
            )
        }
        CellValue::Text(text) => format!(
            "<c r=\"{}\"{} t=\"inlineStr\"><is><t>{}</t></is></c>",
            escape_xml_attr(cell),
            style,
            escape_xml_text(text)
        ),
    }
}

fn split_cell_reference(cell: &str) -> ReportResult<(String, u32)> {
    let trimmed = cell.trim();
    let letter_count = trimmed.bytes().take_while(u8::is_ascii_uppercase).count();

    if letter_count == 0 || letter_count >= trimmed.len() {
        return Err(ReportError::InvalidCell(cell.to_string()));
    }

    let column = trimmed[..letter_count].to_string();
    let row = trimmed[letter_count..]
        .parse::<u32>()
        .map_err(|_| ReportError::InvalidCell(cell.to_string()))?;

    if row == 0 {
        return Err(ReportError::InvalidCell(cell.to_string()));
    }

    Ok((column, row))
}

fn extract_attr(tag: &str, attr: &str) -> Option<String> {
    let pattern = format!(r#"{attr}=""#);
    let start = tag.find(&pattern)? + pattern.len();
    let end = tag[start..].find('"')? + start;
    Some(decode_xml_attr(&tag[start..end]))
}

fn decode_xml_attr(value: &str) -> String {
    value
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

fn escape_xml_attr(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn escape_xml_text(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::io::{Read, Write};

    use super::*;
    use tempfile::TempDir;
    use zip::{ZipArchive, ZipWriter};

    #[test]
    fn split_cell_reference_parses_column_and_row() {
        assert_eq!(split_cell_reference("B12").unwrap(), ("B".to_string(), 12));
        assert_eq!(split_cell_reference("AA1").unwrap(), ("AA".to_string(), 1));
        assert!(split_cell_reference("12").is_err());
    }

    #[test]
    fn patch_cell_replaces_existing_cell_and_preserves_style() {
        let xml = r#"<worksheet><sheetData><row r="9"><c r="B9" s="2"><v>1</v></c></row></sheetData></worksheet>"#;
        let patched = patch_cell_xml(xml, "Sheet1", "B9", &CellValue::Number(2.5)).unwrap();

        assert!(patched.contains(r#"<c r="B9" s="2"><v>2.5</v></c>"#));
        assert!(!patched.contains("<v>1</v>"));
    }

    #[test]
    fn patch_cell_inserts_missing_row() {
        let xml = r#"<worksheet><sheetData></sheetData></worksheet>"#;
        let patched =
            patch_cell_xml(xml, "Sheet1", "C3", &CellValue::Text("SN123".to_string())).unwrap();

        assert!(patched.contains(r#"<row r="3""#));
        assert!(patched.contains("SN123"));
    }

    #[test]
    fn patch_cell_inserts_missing_cell_in_column_order() {
        let xml = r#"<worksheet><sheetData><row r="1"><c r="A1"><v>1</v></c><c r="D1"><v>4</v></c></row></sheetData></worksheet>"#;
        let patched = patch_cell_xml(xml, "Sheet1", "B1", &CellValue::Number(2.0)).unwrap();

        let a = patched.find(r#"r="A1""#).unwrap();
        let b = patched.find(r#"r="B1""#).unwrap();
        let d = patched.find(r#"r="D1""#).unwrap();

        assert!(a < b);
        assert!(b < d);
    }

    #[test]
    fn transformer_serial_number_is_written_as_text() {
        let temp = TempDir::new().expect("temp dir");
        let unit_folder = temp.path().join("262343000072");
        fs::create_dir_all(&unit_folder).expect("unit folder");
        let workbook = unit_folder.join(format!("{MAIN_REPORT_SN_PREFIX}262343000072.xlsx"));
        write_minimal_test_summary_workbook(&workbook);

        let report_path =
            write_transformer_serial_number(&unit_folder, "000123").expect("write D1");

        assert_eq!(report_path, workbook);

        let mut archive = ZipArchive::new(File::open(&report_path).expect("open workbook"))
            .expect("workbook zip");
        let mut sheet_xml = String::new();
        archive
            .by_name("xl/worksheets/sheet1.xml")
            .expect("sheet")
            .read_to_string(&mut sheet_xml)
            .expect("sheet xml");

        assert!(sheet_xml.contains(r#"<c r="D1" t="inlineStr"><is><t>000123</t></is></c>"#));
        assert!(!sheet_xml.contains("<v>000123</v>"));
    }

    #[test]
    fn final_operator_name_is_written_as_text_to_print_report() {
        let temp = TempDir::new().expect("temp dir");
        let unit_folder = temp.path().join("262343000072");
        fs::create_dir_all(&unit_folder).expect("unit folder");
        let workbook = unit_folder.join(PRINT_TEMPLATE_NAME);
        write_minimal_sheet_workbook(&workbook, FINAL_OPERATOR_SHEET);

        let report_path = write_final_operator_name(&unit_folder, "Sean").expect("write E39");

        assert_eq!(report_path, workbook);

        let mut archive = ZipArchive::new(File::open(&report_path).expect("open workbook"))
            .expect("workbook zip");
        let mut sheet_xml = String::new();
        archive
            .by_name("xl/worksheets/sheet1.xml")
            .expect("sheet")
            .read_to_string(&mut sheet_xml)
            .expect("sheet xml");

        assert!(sheet_xml.contains(r#"<c r="E39" t="inlineStr"><is><t>Sean</t></is></c>"#));
        assert!(!sheet_xml.contains("<v>Sean</v>"));
    }

    #[test]
    fn workbook_recalculation_flags_are_forced() {
        let xml = r#"<workbook><calcPr calcId="191028"/></workbook>"#;
        let patched = force_full_recalculation(xml);

        assert!(patched.contains(r#"calcMode="auto""#));
        assert!(patched.contains(r#"fullCalcOnLoad="1""#));
        assert!(patched.contains(r#"forceFullCalc="1""#));
    }

    #[test]
    fn calc_chain_package_parts_are_removed() {
        let rels = r#"<Relationships><Relationship Id="rId4" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/calcChain" Target="calcChain.xml"/><Relationship Id="rId1" Type="x" Target="worksheets/sheet1.xml"/></Relationships>"#;
        let content_types = r#"<Types><Override PartName="/xl/calcChain.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.calcChain+xml"/><Override PartName="/xl/workbook.xml" ContentType="x"/></Types>"#;

        let patched_rels = remove_calc_chain_relationship(rels);
        let patched_content_types = remove_calc_chain_content_type(content_types);

        assert!(!patched_rels.contains("calcChain"));
        assert!(patched_rels.contains("sheet1.xml"));
        assert!(!patched_content_types.contains("calcChain"));
        assert!(patched_content_types.contains("workbook.xml"));
    }

    #[test]
    fn shared_formulas_are_expanded_before_cell_replacement() {
        let xml = r#"<worksheet><sheetData><row r="11"><c r="I11"><f t="shared" ref="I11:I12" si="1">(G11-H11)/H11*100</f><v>0</v></c></row><row r="12"><c r="I12"><f t="shared" si="1"/><v>0</v></c></row></sheetData></worksheet>"#;
        let expanded = expand_shared_formulas(xml);

        assert!(expanded.contains(r#"<f>(G11-H11)/H11*100</f>"#));
        assert!(expanded.contains(r#"<f>(G12-H12)/H12*100</f>"#));
        assert!(!expanded.contains(r#"t="shared""#));
        assert!(!expanded.contains(r#"si="1""#));
    }

    #[test]
    fn orphan_shared_formulas_are_removed() {
        let xml = r#"<worksheet><sheetData><row r="12"><c r="I12"><f t="shared" si="1"/><v>4.2</v></c></row></sheetData></worksheet>"#;
        let expanded = expand_shared_formulas(xml);

        assert!(expanded.contains(r#"<c r="I12"><v>4.2</v></c>"#));
        assert!(!expanded.contains("<f"));
        assert!(!expanded.contains(r#"t="shared""#));
    }

    #[test]
    fn shared_formula_translation_respects_absolute_references() {
        let formula = "SUM($G11,H$11,$J$11,LOG10(A1))";
        let translated = translate_shared_formula(formula, "I11", "L13");

        assert_eq!(translated, "SUM($G13,K$11,$J$11,LOG10(D3))");
    }

    fn write_minimal_test_summary_workbook(path: &Path) {
        write_minimal_sheet_workbook(path, "Test Summary");
    }

    fn write_minimal_sheet_workbook(path: &Path, sheet_name: &str) {
        let file = File::create(path).expect("create workbook");
        let mut zip = ZipWriter::new(file);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

        zip.start_file("[Content_Types].xml", options)
            .expect("content types");
        zip.write_all(
            br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#,
        )
        .expect("write content types");

        zip.start_file("xl/workbook.xml", options)
            .expect("workbook xml");
        zip.write_all(
            format!(
                r#"<workbook xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="{sheet_name}" sheetId="1" r:id="rId1"/></sheets></workbook>"#
            )
            .as_bytes(),
        )
        .expect("write workbook");

        zip.start_file("xl/_rels/workbook.xml.rels", options)
            .expect("workbook rels");
        zip.write_all(
            br#"<Relationships><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#,
        )
        .expect("write rels");

        zip.start_file("xl/worksheets/sheet1.xml", options)
            .expect("sheet xml");
        zip.write_all(
            br#"<worksheet><sheetData><row r="1"><c r="A1"><v>1</v></c></row></sheetData></worksheet>"#,
        )
        .expect("write sheet");

        zip.finish().expect("finish workbook");
    }
}
