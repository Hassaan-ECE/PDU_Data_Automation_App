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
export type TaskWaitPhase =
  | "awaiting_csv"
  | "timing"
  | "soaking"
  | "waiting_step72"
  | "capturing"
  | "waiting_unlock"
  | "ready";
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
  process_ready: boolean;
  wait_phase: TaskWaitPhase;
  phase_deadline_ms: number | null;
  pending_duration_seconds: number;
  nominal_duration_seconds: number;
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

export interface NotificationRuntimeStatus {
  state: "idle" | "ready" | "sent" | "skipped" | "failed";
  message: string;
  station_name: string | null;
  destination_name: string | null;
  updated_at: string | null;
  event_kind?: "test_ping" | "problem" | "complete" | "changeover" | "stuck" | "summary" | null;
}

export interface NotificationEventToggles {
  problem: boolean;
  complete: boolean;
  changeover: boolean;
  stuck: boolean;
  summary: boolean;
}

export interface ShiftWindow {
  label: string;
  start_time: string;
  end_time: string;
}

export type StationRole = "floor" | "admin";

export interface StationCatalogEntry {
  id: string;
  name: string;
  role: StationRole;
}

export interface CatalogCreateRequest {
  name: string;
  role: StationRole;
  select_for_this_pc: boolean;
}

export interface FloorSyncStatus {
  configured: boolean;
  source: string;
  updated_at?: string | null;
  updated_by_station_id?: string | null;
  message: string;
}

export interface AppNotificationSettingsView {
  enabled: boolean;
  teams_destination_name: string;
  teams_webhook_url: string;
  webhook_configured: boolean;
  station_id: string;
  station_name: string;
  idle_timeout_minutes: number;
  events: NotificationEventToggles;
  shared_shift_log_path: string;
  shifts: ShiftWindow[];
  summary_poster_station_id: string;
  summary_included_station_ids: string[];
  is_summary_poster: boolean;
  stations: StationCatalogEntry[];
  floor_sync: FloorSyncStatus;
}

/** Matches backend SettingsSaveScope (snake_case). */
export type SettingsSaveScope =
  | "operator"
  | "identity"
  | "teams"
  | "advanced"
  | "connect"
  | "local";

export type SaveAppNotificationSettingsRequest = Omit<
  AppNotificationSettingsView,
  "webhook_configured" | "is_summary_poster" | "floor_sync"
> & {
  scope: SettingsSaveScope;
  /** Required when Connect targets an existing floor file. */
  connect_password?: string;
  catalog_create?: CatalogCreateRequest;
};

export interface ShiftSummaryPreview {
  text: string;
  is_summary_poster: boolean;
  poster_station_id: string;
  poster_station_name: string;
  event_count: number;
  shared_folder_configured: boolean;
  already_posted?: boolean;
  last_summary_at?: string | null;
  last_summary_by?: string | null;
  last_summary_shift?: string | null;
}

export interface ShiftSummaryResult {
  message: string;
  text: string;
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

/** Pick the shared OneDrive/network folder used for multi-station shift logging. */
export async function chooseSharedNotificationsFolder(): Promise<string | null> {
  if (!isTauriRuntime()) {
    return "C:\\Users\\Public\\PDU_Notifications_Shared";
  }

  const selected = await openDialog({
    directory: true,
    multiple: false,
    title: "Select shared OneDrive notifications folder",
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

export async function getNotificationStatus(): Promise<NotificationRuntimeStatus | null> {
  if (!isTauriRuntime()) {
    return null;
  }

  return invoke<NotificationRuntimeStatus>("get_notification_status");
}

export async function getAppNotificationSettings(): Promise<AppNotificationSettingsView> {
  if (!isTauriRuntime()) {
    return { ...mockNotificationSettings, events: { ...mockNotificationSettings.events } };
  }

  return invoke<AppNotificationSettingsView>("get_app_notification_settings");
}

export async function verifySettingsPassword(password: string): Promise<boolean> {
  if (!isTauriRuntime()) {
    return password === mockSettingsPassword;
  }

  return invoke<boolean>("verify_settings_password", { password });
}

export async function saveAppNotificationSettings(
  request: SaveAppNotificationSettingsRequest,
): Promise<AppNotificationSettingsView> {
  if (!isTauriRuntime()) {
    if (request.teams_webhook_url.trim()) {
      mockNotificationWebhookUrl = request.teams_webhook_url.trim();
    }
    const stations = (request.stations?.length
      ? request.stations
      : mockNotificationSettings.stations
    ).map((s) => ({ ...s }));
    let stationId = request.station_id;
    let stationName = request.station_name;
    if (request.catalog_create) {
      const created = {
        id: `identity-mock-${stations.length + 1}`,
        name: request.catalog_create.name.trim(),
        role: request.catalog_create.role,
      };
      stations.push(created);
      if (request.catalog_create.select_for_this_pc) {
        stationId = created.id;
        stationName = created.name;
      }
    }
    mockNotificationSettings = {
      enabled: request.enabled,
      teams_destination_name: request.teams_destination_name,
      teams_webhook_url: "",
      webhook_configured: Boolean(mockNotificationWebhookUrl),
      station_id: stationId,
      station_name: stationName,
      idle_timeout_minutes: request.idle_timeout_minutes,
      events: { ...request.events },
      shared_shift_log_path: request.shared_shift_log_path,
      shifts: request.shifts.map((shift) => ({ ...shift })),
      summary_poster_station_id: request.summary_poster_station_id,
      summary_included_station_ids: [...request.summary_included_station_ids],
      stations,
      is_summary_poster:
        request.station_id ===
        (request.summary_poster_station_id || "pdu-lab"),
      floor_sync: {
        configured: Boolean(request.shared_shift_log_path?.trim()),
        source: request.shared_shift_log_path?.trim() ? "floor" : "local",
        updated_at: null,
        updated_by_station_id: null,
        message: request.shared_shift_log_path?.trim()
          ? "Syncing via shared folder."
          : "Shared folder not set — settings stay on this PC only.",
      },
    };
    return {
      ...mockNotificationSettings,
      events: { ...mockNotificationSettings.events },
      shifts: mockNotificationSettings.shifts.map((shift) => ({ ...shift })),
      stations: mockNotificationSettings.stations.map((s) => ({ ...s })),
      floor_sync: { ...mockNotificationSettings.floor_sync },
    };
  }

  return invoke<AppNotificationSettingsView>("save_app_notification_settings", { request });
}

export async function changeSettingsPassword(
  currentPassword: string,
  newPassword: string,
  confirmPassword: string,
): Promise<void> {
  if (!isTauriRuntime()) {
    if (currentPassword !== mockSettingsPassword) {
      throw new Error("Current password is incorrect");
    }
    if (!newPassword.trim()) {
      throw new Error("New password must not be empty");
    }
    if (newPassword !== confirmPassword) {
      throw new Error("New password and confirmation do not match");
    }
    mockSettingsPassword = newPassword.trim();
    return;
  }

  await invoke("change_settings_password", {
    request: {
      confirm_password: confirmPassword,
      current_password: currentPassword,
      new_password: newPassword,
    },
  });
}

export async function sendNotificationTest(): Promise<void> {
  if (!isTauriRuntime()) {
    return;
  }

  await invoke("send_notification_test");
}

export async function previewShiftSummary(
  shiftLabel?: string,
): Promise<ShiftSummaryPreview | null> {
  if (!isTauriRuntime()) {
    return {
      text: "📊 End of shift — preview (browser mock)\n\nNo live shared log in browser.",
      is_summary_poster: mockNotificationSettings.station_id === "pdu-lab",
      poster_station_id: mockNotificationSettings.summary_poster_station_id,
      poster_station_name: "PDU Lab",
      event_count: 0,
      shared_folder_configured: Boolean(mockNotificationSettings.shared_shift_log_path),
      already_posted: false,
      last_summary_at: null,
      last_summary_by: null,
      last_summary_shift: null,
    };
  }

  return invoke<ShiftSummaryPreview>("preview_shift_summary", {
    shiftLabel: shiftLabel ?? null,
  });
}

export async function postShiftSummary(shiftLabel: string): Promise<ShiftSummaryResult | null> {
  if (!isTauriRuntime()) {
    return {
      message: `End-of-shift summary posted by ${mockNotificationSettings.station_name}. Other stations will see it was already sent.`,
      text: "📊 End of shift — mock",
    };
  }

  return invoke<ShiftSummaryResult>("post_shift_summary", {
    request: {
      shift_label: shiftLabel,
    },
  });
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

let mockSettingsPassword = "0601";
let mockNotificationWebhookUrl = "";
const mockStations: StationCatalogEntry[] = [
  { id: "test-station-1", name: "Test Station 1", role: "floor" },
  { id: "test-station-3", name: "Test Station 3", role: "floor" },
  { id: "test-station-4", name: "Test Station 4", role: "floor" },
  { id: "pdu-lab", name: "PDU Lab", role: "floor" },
];
let mockNotificationSettings: AppNotificationSettingsView = {
  enabled: true,
  events: {
    complete: true,
    changeover: true,
    problem: true,
    stuck: false,
    summary: true,
  },
  idle_timeout_minutes: 30,
  shared_shift_log_path: "",
  station_id: "test-station-1",
  station_name: "Test Station 1",
  teams_destination_name: "PDU Testing",
  teams_webhook_url: "",
  webhook_configured: false,
  shifts: [],
  summary_poster_station_id: "pdu-lab",
  summary_included_station_ids: [
    "test-station-1",
    "test-station-3",
    "test-station-4",
    "pdu-lab",
  ],
  is_summary_poster: false,
  stations: mockStations,
  floor_sync: {
    configured: false,
    source: "local",
    updated_at: null,
    updated_by_station_id: null,
    message: "Shared folder not set — settings stay on this PC only.",
  },
};
