use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use pdu_data_automation_app_lib::automation::{
    save_final_operator_name, save_transformer_sn, tasks::automation_tasks,
};
use serde_json::json;
use tempfile::TempDir;
use zip::ZipArchive;

const MAIN_REPORT_FIXTURE: &str = "PDUD500442AM088_Test Report_0.2CT_Rev02_SN262343000072.xlsx";
const PRINT_REPORT_WRITE_FIXTURE: &str = "PDUD500442AA088_Write Smoke Test Report Print.xlsx";

#[test]
fn transformer_sn_write_preserves_long_numeric_text_exactly() {
    let temp = TempDir::new().expect("temp dir");
    let unit_folder = temp.path().join("262343000072");
    fs::create_dir_all(&unit_folder).expect("unit folder");
    let workbook = copy_workbook_fixture(&unit_folder, MAIN_REPORT_FIXTURE);
    let transformer_sn = "000123456789012345678901234567890123456789";

    save_transformer_sn(
        unit_folder.display().to_string(),
        transformer_sn.to_string(),
    )
    .expect("save transformer SN");

    assert_workbook_loads(&workbook);
    let sheet_xml = worksheet_xml(&workbook, "xl/worksheets/sheet1.xml");

    assert_inline_text_cell(&sheet_xml, "D1", transformer_sn);
    assert_no_numeric_cell(&sheet_xml, "D1", transformer_sn);
    assert!(
        !sheet_xml.contains("E+"),
        "Transformer SN must not be converted to scientific notation:\n{sheet_xml}"
    );
}

#[test]
fn final_operator_name_write_preserves_text_cell() {
    let temp = TempDir::new().expect("temp dir");
    let unit_folder = temp.path().join("262343000072");
    fs::create_dir_all(&unit_folder).expect("unit folder");
    copy_workbook_fixture(&unit_folder, MAIN_REPORT_FIXTURE);
    save_transformer_sn(unit_folder.display().to_string(), "TX-READY".to_string())
        .expect("save transformer SN");
    write_ready_unit_state(&unit_folder);
    let workbook = copy_workbook_fixture(&unit_folder, PRINT_REPORT_WRITE_FIXTURE);

    let path = save_final_operator_name(unit_folder.display().to_string(), "Long".to_string())
        .expect("save final operator name");

    assert_eq!(Path::new(&path), workbook);
    assert_workbook_loads(&workbook);
    let sheet_xml = worksheet_xml(&workbook, "xl/worksheets/sheet1.xml");

    assert_inline_text_cell(&sheet_xml, "E39", "Long");
    assert_no_numeric_cell(&sheet_xml, "E39", "Long");
}

fn write_ready_unit_state(unit_folder: &Path) {
    let tasks = automation_tasks()
        .into_iter()
        .map(|task| {
            json!({
                "task_id": task.id,
                "state": "pass",
                "code": 0,
                "source_csv_path": null,
                "csv_fingerprint": null,
                "processed_at": "2026-06-24T00:00:00-05:00",
                "result": "test ready state",
                "accepted": {
                    "accepted": false,
                    "accepted_at": null,
                    "accepted_by": null,
                    "reason": null
                },
                "audit_log": []
            })
        })
        .map(|entry| {
            let task_id = entry
                .get("task_id")
                .and_then(|value| value.as_str())
                .expect("task id")
                .to_string();
            (task_id, entry)
        })
        .collect::<serde_json::Map<_, _>>();

    let state = json!({
        "schema_version": 1,
        "tasks": tasks
    });

    fs::write(
        unit_folder.join("unit_state.json"),
        serde_json::to_string_pretty(&state).expect("serialize state"),
    )
    .expect("write unit state");
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("backend should be under repo root")
        .to_path_buf()
}

fn copy_workbook_fixture(unit_folder: &Path, file_name: &str) -> PathBuf {
    let source = repo_root()
        .join("fixtures")
        .join("workbooks")
        .join(file_name);
    let target = unit_folder.join(file_name);

    fs::copy(&source, &target).unwrap_or_else(|error| {
        panic!(
            "copy workbook fixture {} to {}: {error}",
            source.display(),
            target.display()
        )
    });

    target
}

fn assert_workbook_loads(path: &Path) {
    let mut archive = ZipArchive::new(File::open(path).expect("open workbook")).expect("zip");

    archive
        .by_name("[Content_Types].xml")
        .expect("content types");
    archive.by_name("xl/workbook.xml").expect("workbook xml");
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

fn assert_inline_text_cell(xml: &str, cell: &str, value: &str) {
    let expected = format!(r#"<c r="{cell}" t="inlineStr"><is><t>{value}</t></is></c>"#);

    assert!(
        xml.contains(&expected),
        "expected inline text cell {expected} in worksheet XML:\n{xml}"
    );
}

fn assert_no_numeric_cell(xml: &str, cell: &str, value: &str) {
    let exact_numeric = format!(r#"<c r="{cell}"><v>{value}</v></c>"#);
    let any_numeric_start = format!(r#"<c r="{cell}"><v>"#);

    assert!(
        !xml.contains(&exact_numeric) && !xml.contains(&any_numeric_start),
        "cell {cell} must not be written as a numeric value:\n{xml}"
    );
}
