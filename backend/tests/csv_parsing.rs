use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use pdu_data_automation_app_lib::automation::{process_task_at, scan_unit_folder};
use tempfile::TempDir;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

const MAIN_REPORT_NAME: &str = "PDUD500442AM088_Test Report_0.2CT_Rev02_SN262343000072.xlsx";
const PRINT_REPORT_NAME: &str = "PDUD500442AA088_0.2CT Test Report Print.xlsx";

#[test]
fn transformer_csv_required_value_errors_fail_clearly_without_zero_fallback() {
    for case in [
        CsvFailureCase {
            fixture: "missing_required_column_STEP14_TRANSFORMER_TEST_DATA_AVG.csv",
            expected: "source column Z is missing",
        },
        CsvFailureCase {
            fixture: "blank_numeric_STEP14_TRANSFORMER_TEST_DATA_AVG.csv",
            expected: "source column Z is blank",
        },
        CsvFailureCase {
            fixture: "malformed_numeric_STEP14_TRANSFORMER_TEST_DATA_AVG.csv",
            expected: "source column Z value 'not-a-number' is not numeric",
        },
    ] {
        let temp = TempDir::new().expect("temp dir");
        let unit_folder = temp.path().join("262343000072");
        fs::create_dir_all(&unit_folder).expect("unit folder");
        copy_csv_fixture(&unit_folder, case.fixture);

        let result = process_task_at(
            unit_folder.display().to_string(),
            "208v-transformer".to_string(),
            u64::MAX,
        )
        .expect("known task should process to a task result");

        assert_eq!(result.state, "fail", "{}", result.message);
        assert_eq!(result.code, 1);
        assert!(
            result.message.contains(case.expected),
            "expected '{}' in '{}'",
            case.expected,
            result.message
        );
        assert!(
            !result.message.contains("processed successfully"),
            "CSV failure must not be treated as a successful zero-value write"
        );
        assert!(result.report_path.is_none());
        assert!(result.print_report_path.is_none());
    }
}

#[test]
fn system_burn_in_step72_fixture_writes_report_capture_values() {
    let temp = TempDir::new().expect("temp dir");
    let unit_folder = temp.path().join("262343000072");
    fs::create_dir_all(&unit_folder).expect("unit folder");
    copy_csv_fixture(&unit_folder, "sample_STEP71_SYSTEM_BURN_IN_SOAK.csv");
    copy_csv_fixture(
        &unit_folder,
        "valid_STEP72_SYSTEM_ACCURACY_TEST_DATA_AVG_report_capture.csv",
    );
    write_burn_in_workbooks(&unit_folder);

    let result = process_task_at(
        unit_folder.display().to_string(),
        "system-burn-in".to_string(),
        u64::MAX,
    )
    .expect("known task should process");

    assert_eq!(result.state, "pass", "{}", result.message);
    assert_eq!(result.code, 0);

    let print_xml = worksheet_xml(
        &unit_folder.join(PRINT_REPORT_NAME),
        "xl/worksheets/sheet1.xml",
    );
    assert_cell_value(&print_xml, "F30", "100");
    assert_cell_value(&print_xml, "F31", "0.1");
    assert_no_cell_value(&print_xml, "F30", "0");
}

#[test]
fn system_burn_in_step71_alone_is_detected_but_not_used_for_report_capture() {
    let temp = TempDir::new().expect("temp dir");
    let unit_folder = temp.path().join("262343000072");
    fs::create_dir_all(&unit_folder).expect("unit folder");
    copy_csv_fixture(&unit_folder, "sample_STEP71_SYSTEM_BURN_IN_SOAK.csv");

    let summary =
        scan_unit_folder(unit_folder.display().to_string()).expect("unit folder should scan");
    let burn_in = summary
        .tasks
        .iter()
        .find(|task| task.task_id == "system-burn-in")
        .expect("system burn-in task should exist");

    assert_eq!(burn_in.detected_steps, vec![71]);

    let result = process_task_at(
        unit_folder.display().to_string(),
        "system-burn-in".to_string(),
        u64::MAX,
    )
    .expect("known task should process to a task result");

    assert_eq!(result.state, "waiting", "{}", result.message);
    assert_eq!(result.code, 2);
    assert!(
        result.message.contains("waiting for matching STEP72"),
        "STEP71 soak data must not satisfy STEP72 report capture: {}",
        result.message
    );
}

#[test]
fn system_burn_in_uses_step72_values_when_step71_and_step72_exist() {
    let temp = TempDir::new().expect("temp dir");
    let unit_folder = temp.path().join("262343000072");
    fs::create_dir_all(&unit_folder).expect("unit folder");
    copy_csv_fixture(&unit_folder, "sample_STEP71_SYSTEM_BURN_IN_SOAK.csv");
    copy_csv_fixture(
        &unit_folder,
        "sample_STEP72_SYSTEM_ACCURACY_TEST_DATA_AVG_report_capture.csv",
    );
    write_burn_in_workbooks(&unit_folder);

    let summary =
        scan_unit_folder(unit_folder.display().to_string()).expect("unit folder should scan");
    let burn_in = summary
        .tasks
        .iter()
        .find(|task| task.task_id == "system-burn-in")
        .expect("system burn-in task should exist");

    assert_eq!(burn_in.detected_steps, vec![71, 72]);

    let result = process_task_at(
        unit_folder.display().to_string(),
        "system-burn-in".to_string(),
        u64::MAX,
    )
    .expect("known task should process");

    assert_eq!(result.state, "pass", "{}", result.message);
    assert!(result.log.iter().any(|line| line.contains("_STEP72_")));

    let print_xml = worksheet_xml(
        &unit_folder.join(PRINT_REPORT_NAME),
        "xl/worksheets/sheet1.xml",
    );
    assert_cell_value(&print_xml, "F30", "200");
    assert_no_cell_value(&print_xml, "F30", "999");
}

struct CsvFailureCase {
    fixture: &'static str,
    expected: &'static str,
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("backend should be under repo root")
        .to_path_buf()
}

fn copy_csv_fixture(unit_folder: &Path, file_name: &str) -> PathBuf {
    let source = repo_root().join("fixtures").join("csv").join(file_name);
    let target = unit_folder.join(file_name);

    fs::copy(&source, &target).unwrap_or_else(|error| {
        panic!(
            "copy CSV fixture {} to {}: {error}",
            source.display(),
            target.display()
        )
    });

    target
}

fn write_burn_in_workbooks(unit_folder: &Path) {
    write_minimal_workbook(
        &unit_folder.join(MAIN_REPORT_NAME),
        &["Burn-in System - 415", "Test Summary"],
    );
    write_minimal_workbook(&unit_folder.join(PRINT_REPORT_NAME), &["Test Report"]);
}

fn write_minimal_workbook(path: &Path, sheet_names: &[&str]) {
    let file = File::create(path).expect("create workbook");
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    zip.start_file("[Content_Types].xml", options)
        .expect("content types");
    zip.write_all(content_types_xml(sheet_names.len()).as_bytes())
        .expect("write content types");

    zip.start_file("xl/workbook.xml", options)
        .expect("workbook xml");
    zip.write_all(workbook_xml(sheet_names).as_bytes())
        .expect("write workbook");

    zip.start_file("xl/_rels/workbook.xml.rels", options)
        .expect("workbook rels");
    zip.write_all(workbook_rels_xml(sheet_names.len()).as_bytes())
        .expect("write rels");

    for index in 1..=sheet_names.len() {
        zip.start_file(format!("xl/worksheets/sheet{index}.xml"), options)
            .expect("sheet xml");
        zip.write_all(br#"<worksheet><sheetData></sheetData></worksheet>"#)
            .expect("write sheet");
    }

    zip.finish().expect("finish workbook");
}

fn content_types_xml(sheet_count: usize) -> String {
    let mut xml = String::from(
        r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>"#,
    );

    for index in 1..=sheet_count {
        xml.push_str(&format!(
            r#"<Override PartName="/xl/worksheets/sheet{index}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>"#
        ));
    }

    xml.push_str("</Types>");
    xml
}

fn workbook_xml(sheet_names: &[&str]) -> String {
    let mut xml = String::from(
        r#"<workbook xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets>"#,
    );

    for (index, sheet_name) in sheet_names.iter().enumerate() {
        let sheet_id = index + 1;
        xml.push_str(&format!(
            r#"<sheet name="{}" sheetId="{sheet_id}" r:id="rId{sheet_id}"/>"#,
            escape_xml_attr(sheet_name)
        ));
    }

    xml.push_str("</sheets></workbook>");
    xml
}

fn workbook_rels_xml(sheet_count: usize) -> String {
    let mut xml = String::from("<Relationships>");

    for index in 1..=sheet_count {
        xml.push_str(&format!(
            r#"<Relationship Id="rId{index}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet{index}.xml"/>"#
        ));
    }

    xml.push_str("</Relationships>");
    xml
}

fn worksheet_xml(path: &Path, sheet_path: &str) -> String {
    let mut archive = ZipArchive::new(File::open(path).expect("open workbook")).expect("zip");
    let mut xml = String::new();

    archive
        .by_name(sheet_path)
        .expect("sheet")
        .read_to_string(&mut xml)
        .expect("read sheet");

    xml
}

fn assert_cell_value(xml: &str, cell: &str, value: &str) {
    let expected = format!(r#"<c r="{cell}"><v>{value}</v></c>"#);

    assert!(
        xml.contains(&expected),
        "expected cell value {expected} in worksheet XML:\n{xml}"
    );
}

fn assert_no_cell_value(xml: &str, cell: &str, value: &str) {
    let unexpected = format!(r#"<c r="{cell}"><v>{value}</v></c>"#);

    assert!(
        !xml.contains(&unexpected),
        "unexpected cell value {unexpected} in worksheet XML:\n{xml}"
    );
}

fn escape_xml_attr(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
