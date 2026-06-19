import { invoke } from "@tauri-apps/api/core";
import { open as openDialog } from "@tauri-apps/plugin-dialog";

export interface BackendStatus {
  app_name: string;
  version: string;
  backend: string;
  process_uptime_ms?: number;
  window_setup_uptime_ms?: number | null;
}

export interface LayoutValidationResult {
  warnings: string[];
  errors: string[];
}

export interface LayoutLoadResponse {
  profile_id: string;
  display_name: string;
  task_count: number;
  validation: LayoutValidationResult;
}

export type BackendTaskState = "off" | "detected" | "waiting" | "processing" | "pass" | "warning" | "fail";

export interface BackendTaskStatus {
  task_id: string;
  label: string;
  step: string;
  state: BackendTaskState;
  detected_steps: number[];
  latest_csv: string | null;
  latest_csv_created_ms: number | null;
  latest_csv_readable: boolean | null;
  timer_start_ms: number | null;
}

export interface UnitFolderSummary {
  unit_folder: string;
  serial_number: string | null;
  report_path: string | null;
  print_report_path: string | null;
  detected_count: number;
  tasks: BackendTaskStatus[];
  warnings: string[];
}

export interface FailureLocation {
  workbook_path: string;
  sheet: string;
  cell: string;
}

export interface FailureDetail {
  title: string;
  message: string;
  location: FailureLocation | null;
}

export interface TaskProcessResult {
  task_id: string;
  state: BackendTaskState;
  code: number;
  message: string;
  log: string[];
  report_path: string | null;
  print_report_path: string | null;
  failure: FailureDetail | null;
}

export function isTauriRuntime() {
  return Boolean(window.__TAURI_INTERNALS__);
}

export async function getBackendStatus(): Promise<BackendStatus | null> {
  if (!isTauriRuntime()) {
    return null;
  }

  return invoke<BackendStatus>("get_app_status");
}

export async function loadLayoutProfile(): Promise<LayoutLoadResponse | null> {
  if (!isTauriRuntime()) {
    return null;
  }

  return invoke<LayoutLoadResponse>("load_layout_profile");
}

export async function chooseUnitFolder(): Promise<string | null> {
  if (!isTauriRuntime()) {
    return "C:\\PDU500\\DEMO_20260617";
  }

  const selected = await openDialog({
    directory: true,
    multiple: false,
    title: "Select PDU unit folder",
  });

  return typeof selected === "string" ? selected : null;
}

export async function setupUnitFolder(unitFolder: string): Promise<UnitFolderSummary | null> {
  if (!isTauriRuntime()) {
    return mockUnitFolderSummary(unitFolder);
  }

  return invoke<UnitFolderSummary>("setup_unit_folder", { unitFolder });
}

export async function scanUnitFolder(unitFolder: string): Promise<UnitFolderSummary | null> {
  if (!isTauriRuntime()) {
    return mockUnitFolderSummary(unitFolder);
  }

  return invoke<UnitFolderSummary>("scan_unit_folder", { unitFolder });
}

export async function processAutomationTask(
  unitFolder: string,
  taskId: string,
): Promise<TaskProcessResult | null> {
  if (!isTauriRuntime()) {
    await new Promise((resolve) => window.setTimeout(resolve, 250));

    return {
      task_id: taskId,
      state: "pass",
      code: 0,
      message: "Mock task processed",
      log: [],
      report_path: null,
      print_report_path: null,
      failure: null,
    };
  }

  return invoke<TaskProcessResult>("process_automation_task", { unitFolder, taskId });
}

export async function openReportPath(unitFolder: string, path: string) {
  if (!isTauriRuntime()) {
    return;
  }

  await invoke("open_report_path", { unitFolder, path });
}

export async function openReportLocation(
  unitFolder: string,
  path: string,
  sheet: string,
  cell: string,
) {
  if (!isTauriRuntime()) {
    return;
  }

  await invoke("open_report_location", { unitFolder, path, sheet, cell });
}

function mockUnitFolderSummary(unitFolder: string): UnitFolderSummary {
  const folderName = unitFolder.split(/[\\/]/).filter(Boolean).at(-1) ?? "";
  const serialNumber = folderName.match(/\d{6,}/)?.[0] ?? "262343000072";

  return {
    unit_folder: unitFolder,
    serial_number: serialNumber,
    report_path: `${unitFolder}\\PDUD500442AM088_Test Report_0.2CT_Rev02_SN${serialNumber}.xlsx`,
    print_report_path: `${unitFolder}\\PDUD500442AA088_0.2CT Test Report Print.xlsx`,
    detected_count: 0,
    tasks: [],
    warnings: [],
  };
}
