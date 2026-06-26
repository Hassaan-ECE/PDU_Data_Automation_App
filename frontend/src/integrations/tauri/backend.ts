import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
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
const AUTOMATION_TASK_BATCH_PROGRESS_EVENT = "automation-task-batch-progress";

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
  processable: boolean;
  match_reason: string;
  source_csv_path: string | null;
  csv_fingerprint: string | null;
  processed_at: string | null;
  result: string | null;
  accepted: boolean;
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

export interface UnitFolderSuggestion {
  detection_reason?: string | null;
  detection_source?: string | null;
  unit_folder: string;
  serial_label?: string | null;
  serial_number: string;
  detected_count?: number | null;
}

interface LatestUnitCandidateResponse {
  candidate: UnitFolderSuggestion | null;
  searched_roots: string[];
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
  source_csv_path: string | null;
  csv_fingerprint: string | null;
}

export interface TaskBatchProcessResult {
  results: TaskProcessResult[];
  committed: boolean;
  committed_count: number;
  stopped_task_id: string | null;
  message: string;
}

export interface TaskBatchProgress {
  unit_folder: string;
  task_id: string;
  state: BackendTaskState;
  message: string;
  index: number;
  total: number;
}

export interface PrintReadinessBlocker {
  task_id: string | null;
  label: string | null;
  code: string;
  reason: string;
}

export interface PrintReadinessResult {
  ready: boolean;
  message: string;
  blocking_issues: PrintReadinessBlocker[];
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

export async function getSuggestedUnitFolder(): Promise<UnitFolderSuggestion | null> {
  if (!isTauriRuntime()) {
    return mockUnitFolderSuggestion();
  }

  const result = await invoke<LatestUnitCandidateResponse>("find_latest_unit_candidate");

  return result.candidate;
}

export async function setupUnitFolder(
  unitFolder: string,
  transformerSn?: string,
  unitSerialNumber?: string | null,
): Promise<UnitFolderSummary | null> {
  if (!isTauriRuntime()) {
    return mockUnitFolderSummary(unitFolder);
  }

  const trimmedTransformerSn = transformerSn?.trim();

  if (trimmedTransformerSn) {
    return invoke<UnitFolderSummary>("setup_unit_folder_with_transformer_sn", {
      transformerSn: trimmedTransformerSn,
      unitFolder,
      unitSerialNumber: unitSerialNumber?.trim() || null,
    });
  }

  return invoke<UnitFolderSummary>("setup_unit_folder", { unitFolder });
}

export async function saveTransformerSn(unitFolder: string, transformerSn: string): Promise<void> {
  if (!isTauriRuntime()) {
    return;
  }

  return invoke<void>("save_transformer_sn", { unitFolder, transformerSn });
}

export async function saveFinalOperatorName(
  unitFolder: string,
  operatorName: string,
): Promise<string> {
  if (!isTauriRuntime()) {
    return `${unitFolder}\\PDUD500442AA088_0.2CT Test Report Print.xlsx`;
  }

  return invoke<string>("save_final_operator_name", { unitFolder, operatorName });
}

export async function openPrintReportDialog(unitFolder: string): Promise<void> {
  if (!isTauriRuntime()) {
    return;
  }

  return invoke<void>("open_print_report_dialog", { unitFolder });
}

export async function validateReadyForPrint(unitFolder: string): Promise<PrintReadinessResult> {
  if (!isTauriRuntime()) {
    return {
      blocking_issues: [],
      message: "Ready to print.",
      ready: true,
    };
  }

  return invoke<PrintReadinessResult>("validate_ready_for_print", { unitFolder });
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
      source_csv_path: null,
      csv_fingerprint: null,
    };
  }

  return invoke<TaskProcessResult>("process_automation_task", { unitFolder, taskId });
}

export async function processAutomationTasks(
  unitFolder: string,
  taskIds: string[],
): Promise<TaskBatchProcessResult | null> {
  if (!isTauriRuntime()) {
    await new Promise((resolve) => window.setTimeout(resolve, 250));

    return {
      committed: true,
      committed_count: taskIds.length,
      message: `Mock batch processed ${taskIds.length} task${taskIds.length === 1 ? "" : "s"}`,
      results: taskIds.map((taskId) => ({
        task_id: taskId,
        state: "pass",
        code: 0,
        message: "Mock task processed",
        log: [],
        report_path: null,
        print_report_path: null,
        failure: null,
        source_csv_path: null,
        csv_fingerprint: null,
      })),
      stopped_task_id: null,
    };
  }

  return invoke<TaskBatchProcessResult>("process_automation_tasks", { unitFolder, taskIds });
}

export async function listenAutomationTaskBatchProgress(
  handler: (progress: TaskBatchProgress) => void,
): Promise<() => void> {
  if (!isTauriRuntime()) {
    return () => {};
  }

  return listen<TaskBatchProgress>(AUTOMATION_TASK_BATCH_PROGRESS_EVENT, (event) => {
    handler(event.payload);
  });
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

function mockUnitFolderSuggestion(): UnitFolderSuggestion {
  const unitFolder = "C:\\PDU500\\DEMO_20260617";
  const folderName = unitFolder.split(/[\\/]/).filter(Boolean).at(-1) ?? "";
  const serialNumber = folderName.match(/\d{6,}/)?.[0] ?? "262343000072";

  return {
    detected_count: 0,
    detection_reason: "browser mock",
    detection_source: unitFolder,
    serial_label: `SN ${serialNumber}`,
    serial_number: serialNumber,
    unit_folder: unitFolder,
  };
}
