use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static RUNTIME_RESOURCE_DIR: OnceLock<PathBuf> = OnceLock::new();

pub fn set_runtime_resource_dir(path: PathBuf) {
    let _ = RUNTIME_RESOURCE_DIR.set(path);
}

pub(crate) fn report_layout_candidate_paths(file_name: &str) -> Vec<PathBuf> {
    report_layout_candidate_paths_for(file_name, runtime_resource_dir())
}

pub(crate) fn report_layout_candidate_paths_for(
    file_name: &str,
    resource_dir: Option<&Path>,
) -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if let Some(resource_dir) = resource_dir {
        push_resource_report_layout_paths(&mut paths, resource_dir, file_name);
    }

    if let Ok(current_dir) = std::env::current_dir() {
        push_unique(
            &mut paths,
            current_dir
                .join("config")
                .join("report-layouts")
                .join(file_name),
        );
        push_unique(
            &mut paths,
            current_dir
                .join("..")
                .join("config")
                .join("report-layouts")
                .join(file_name),
        );
    }

    push_unique(
        &mut paths,
        PathBuf::from("C:/PDU500")
            .join("config")
            .join("report-layouts")
            .join(file_name),
    );

    paths
}

fn runtime_resource_dir() -> Option<&'static Path> {
    RUNTIME_RESOURCE_DIR.get().map(PathBuf::as_path)
}

fn push_resource_report_layout_paths(
    paths: &mut Vec<PathBuf>,
    resource_dir: &Path,
    file_name: &str,
) {
    // The Tauri config references ../config/... from backend/tauri.conf.json,
    // which is bundled under _up_/config/... in current release output. The
    // extra shapes keep the loader tolerant of dev and bundler layout changes.
    push_unique(
        paths,
        resource_dir
            .join("_up_")
            .join("config")
            .join("report-layouts")
            .join(file_name),
    );
    push_unique(
        paths,
        resource_dir
            .join("config")
            .join("report-layouts")
            .join(file_name),
    );
    push_unique(paths, resource_dir.join("report-layouts").join(file_name));
    push_unique(paths, resource_dir.join(file_name));
}

fn push_unique(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if !paths.iter().any(|existing| existing == &path) {
        paths.push(path);
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn bundled_report_layout_paths_are_first() {
        let resource_dir = PathBuf::from("C:/app/resources");
        let paths =
            report_layout_candidate_paths_for("pdu500.rev02.layout.json", Some(&resource_dir));

        assert_eq!(
            paths.first(),
            Some(
                &resource_dir
                    .join("_up_")
                    .join("config")
                    .join("report-layouts")
                    .join("pdu500.rev02.layout.json")
            )
        );
    }
}
