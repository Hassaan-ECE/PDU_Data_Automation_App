use std::fs;
use std::path::{Path, PathBuf};

use pdu_data_automation_app_lib::automation::scan_unit_folder;
use tempfile::TempDir;

#[test]
fn scan_unit_folder_detects_fixture_step14_without_mutating_fixture() {
    let fixture = repo_root()
        .join("fixtures")
        .join("unit-folders")
        .join("basic-detected")
        .join("262343000072");
    let temp = TempDir::new().expect("temp dir");
    let unit_copy = temp.path().join("262343000072");

    copy_dir(&fixture, &unit_copy);

    let summary = scan_unit_folder(unit_copy.display().to_string()).expect("scan fixture");
    let transformer = summary
        .tasks
        .iter()
        .find(|task| task.task_id == "208v-transformer")
        .expect("208v transformer task should exist");

    assert_eq!(summary.serial_number.as_deref(), Some("262343000072"));
    assert_eq!(transformer.state, "detected");
    assert_eq!(transformer.detected_steps, vec![14]);
    assert!(summary.report_path.is_none());
    assert!(summary.print_report_path.is_none());
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("backend should be under repo root")
        .to_path_buf()
}

fn copy_dir(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).expect("create destination");

    for entry in fs::read_dir(source).expect("read source") {
        let entry = entry.expect("read entry");
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());

        if source_path.is_dir() {
            copy_dir(&source_path, &destination_path);
        } else {
            fs::copy(&source_path, &destination_path).expect("copy fixture file");
        }
    }
}
