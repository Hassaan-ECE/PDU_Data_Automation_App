import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type {
  TaskBatchProcessResult,
  TaskBatchProgress,
} from "@/integrations/tauri/backend";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((resolvePromise) => {
    resolve = resolvePromise;
  });

  return { promise, resolve };
}

const mocks = vi.hoisted(() => ({
  acceptAutomationTaskFailure: vi.fn(),
  chooseUnitFolder: vi.fn(),
  chooseSharedNotificationsFolder: vi.fn(),
  changeSettingsPassword: vi.fn(),
  getAppNotificationSettings: vi.fn(),
  getBackendStatus: vi.fn(),
  getNotificationStatus: vi.fn(),
  getSuggestedUnitFolder: vi.fn(),
  listenAutomationTaskBatchProgress: vi.fn(),
  loadLayoutProfile: vi.fn(),
  openReportLocation: vi.fn(),
  openReportPath: vi.fn(),
  postShiftSummary: vi.fn(),
  previewShiftSummary: vi.fn(),
  processAutomationTasks: vi.fn(),
  processAutomationTask: vi.fn(),
  saveAppNotificationSettings: vi.fn(),
  sendNotificationTest: vi.fn(),
  saveTransformerSn: vi.fn(),
  scanUnitFolder: vi.fn(),
  setupUnitFolder: vi.fn(),
  validateReadyForPrint: vi.fn(),
  verifySettingsPassword: vi.fn(),
}));

vi.mock("@/integrations/tauri/backend", () => ({
  acceptAutomationTaskFailure: mocks.acceptAutomationTaskFailure,
  chooseUnitFolder: mocks.chooseUnitFolder,
  chooseSharedNotificationsFolder: mocks.chooseSharedNotificationsFolder,
  changeSettingsPassword: mocks.changeSettingsPassword,
  getAppNotificationSettings: mocks.getAppNotificationSettings,
  getBackendStatus: mocks.getBackendStatus,
  getNotificationStatus: mocks.getNotificationStatus,
  getSuggestedUnitFolder: mocks.getSuggestedUnitFolder,
  isTauriRuntime: () => false,
  listenAutomationTaskBatchProgress: mocks.listenAutomationTaskBatchProgress,
  loadLayoutProfile: mocks.loadLayoutProfile,
  openReportLocation: mocks.openReportLocation,
  openReportPath: mocks.openReportPath,
  postShiftSummary: mocks.postShiftSummary,
  previewShiftSummary: mocks.previewShiftSummary,
  processAutomationTasks: mocks.processAutomationTasks,
  processAutomationTask: mocks.processAutomationTask,
  saveAppNotificationSettings: mocks.saveAppNotificationSettings,
  sendNotificationTest: mocks.sendNotificationTest,
  saveTransformerSn: mocks.saveTransformerSn,
  scanUnitFolder: mocks.scanUnitFolder,
  setupUnitFolder: mocks.setupUnitFolder,
  validateReadyForPrint: mocks.validateReadyForPrint,
  verifySettingsPassword: mocks.verifySettingsPassword,
}));

import { App } from "@/app/App";

type MockTask = {
  detected_steps: number[];
  label: string;
  latest_csv: string | null;
  latest_csv_created_ms: number | null;
  latest_csv_readable: boolean | null;
  match_reason: string;
  nominal_duration_seconds: number;
  pending_duration_seconds: number;
  phase_deadline_ms: number | null;
  process_ready: boolean;
  processable: boolean;
  state: "off" | "detected" | "waiting" | "processing" | "pass" | "warning" | "fail";
  step: string;
  task_id: string;
  timer_start_ms: number | null;
  wait_phase: "awaiting_csv" | "timing" | "soaking" | "waiting_step72" | "capturing" | "waiting_unlock" | "ready";
};

function unitSummary(unitFolder: string, serialNumber: string, tasks: MockTask[] = []) {
  return {
    detected_count: tasks.filter((task) => task.state === "detected").length,
    print_report_path: `${unitFolder}\\print.xlsx`,
    report_path: `${unitFolder}\\main.xlsx`,
    serial_number: serialNumber,
    tasks,
    unit_folder: unitFolder,
    warnings: [],
  };
}

function detectedSystemTask(): MockTask {
  return {
    detected_steps: [15],
    label: "100% Load",
    latest_csv: "C:\\PDU500\\262343000072\\STEP15.csv",
    latest_csv_created_ms: Date.now(),
    latest_csv_readable: true,
    match_reason: "matched fixture CSV",
    nominal_duration_seconds: 180,
    pending_duration_seconds: 0,
    phase_deadline_ms: Date.now() + 180_000,
    process_ready: false,
    processable: true,
    state: "detected",
    step: "15",
    task_id: "208v-system-100% Load",
    timer_start_ms: Date.now(),
    wait_phase: "timing",
  };
}

function detectedTransformerTask(): MockTask {
  return {
    detected_steps: [14],
    label: "208V Transformer Check",
    latest_csv: "C:\\PDU500\\262343000072\\STEP14.csv",
    latest_csv_created_ms: Date.now() - 60_000,
    latest_csv_readable: true,
    match_reason: "matched fixture CSV",
    nominal_duration_seconds: 60,
    pending_duration_seconds: 0,
    phase_deadline_ms: Date.now() - 1,
    process_ready: true,
    processable: true,
    state: "detected",
    step: "14",
    task_id: "208v-transformer",
    timer_start_ms: null,
    wait_phase: "ready",
  };
}

describe("OperatorPanel current-step scrolling", () => {
  let scrollIntoView: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    vi.clearAllMocks();
    scrollIntoView = vi.fn();
    Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
      configurable: true,
      value: scrollIntoView,
    });
    mocks.getBackendStatus.mockResolvedValue({
      app_name: "PDU Data Automation",
      backend: "tauri-rust",
      process_uptime_ms: 1,
      version: "0.2.8",
      window_setup_uptime_ms: 1,
    });
    mocks.loadLayoutProfile.mockResolvedValue({
      display_name: "PDU500 0.2CT Rev02",
      profile_id: "pdu500.rev02",
      task_count: 65,
      validation: { errors: [], warnings: [] },
    });
    mocks.getSuggestedUnitFolder.mockResolvedValue({
      serial_number: "262343000072",
      unit_folder: "C:\\PDU500\\262343000072",
    });
    mocks.getNotificationStatus.mockResolvedValue(null);
    mocks.listenAutomationTaskBatchProgress.mockResolvedValue(() => {});
    const summary = unitSummary("C:\\PDU500\\262343000072", "262343000072", [
      detectedTransformerTask(),
      detectedSystemTask(),
    ]);
    mocks.chooseUnitFolder.mockResolvedValue("C:\\PDU500\\262343000072");
    mocks.acceptAutomationTaskFailure.mockResolvedValue(null);
    mocks.processAutomationTasks.mockResolvedValue(null);
    mocks.processAutomationTask.mockResolvedValue(null);
    mocks.saveTransformerSn.mockResolvedValue(undefined);
    mocks.setupUnitFolder.mockResolvedValue(summary);
    mocks.scanUnitFolder.mockResolvedValue(summary);
    mocks.validateReadyForPrint.mockResolvedValue({
      blocking_issues: [],
      message: "Ready to print.",
      ready: true,
    });
  });

  afterEach(() => {
    delete (HTMLElement.prototype as { scrollIntoView?: unknown }).scrollIntoView;
  });

  it("does not auto-scroll for an all-pass previous-test batch", async () => {
    render(<App />);

    expect(screen.getByPlaceholderText("Select Test Unit...")).toHaveValue("");
    fireEvent.click(screen.getByRole("button", { name: "Browse unit folder" }));

    await waitFor(() => {
      expect(screen.getByLabelText("Selected test unit")).toHaveValue("262343000072");
    });
    fireEvent.change(screen.getByLabelText("Transformer SN"), {
      target: { value: "TX-SCROLL" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Start" }));

    expect(await screen.findByText("Previous Tests Detected")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Batch Run Previous Tests" }));

    await waitFor(() => {
      expect(mocks.processAutomationTasks).toHaveBeenCalled();
    });

    expect(screen.getByRole("button", { name: "Pause" })).toBeInTheDocument();
    expect(scrollIntoView).not.toHaveBeenCalled();
  });

  it("pins the first batch failure until the operator passes it", async () => {
    const readySystemTask = {
      ...detectedSystemTask(),
      phase_deadline_ms: Date.now() - 1,
      process_ready: true,
      wait_phase: "ready" as const,
    };
    const summary = unitSummary("C:\\PDU500\\262343000072", "262343000072", [
      detectedTransformerTask(),
      readySystemTask,
    ]);
    mocks.setupUnitFolder.mockResolvedValue(summary);
    mocks.scanUnitFolder.mockResolvedValue(summary);
    const batchResult = {
      committed: true,
      committed_count: 0,
      message: "Batch processed 2 tasks (0 passed, 2 failed).",
      results: [
        {
          code: 1,
          continue_sequence: true,
          csv_fingerprint: "failed-transformer",
          failure: {
            title: "Accuracy Check Failed",
            message: "Voltage is outside the allowed range.",
            location: {
              workbook_path: "C:\\PDU500\\262343000072\\main.xlsx",
              sheet: "System Test - 480_208",
              cell: "G57",
            },
          },
          log: [],
          message: "208V Transformer Check failed verification",
          print_report_path: null,
          report_path: "C:\\PDU500\\262343000072\\main.xlsx",
          source_csv_path: "C:\\PDU500\\262343000072\\STEP14.csv",
          state: "fail",
          task_id: "208v-transformer",
        },
        {
          code: 1,
          continue_sequence: true,
          csv_fingerprint: "failed-system",
          failure: {
            title: "Current Check Failed",
            message: "Current is outside the allowed range.",
            location: {
              workbook_path: "C:\\PDU500\\262343000072\\main.xlsx",
              sheet: "System Test - 480_208",
              cell: "G37",
            },
          },
          log: [],
          message: "100% Load failed verification",
          print_report_path: null,
          report_path: "C:\\PDU500\\262343000072\\main.xlsx",
          source_csv_path: "C:\\PDU500\\262343000072\\STEP15.csv",
          state: "fail",
          task_id: "208v-system-100% Load",
        },
      ],
      stopped_task_id: null,
    } satisfies TaskBatchProcessResult;
    const pendingBatch = deferred<TaskBatchProcessResult>();
    let batchProgressHandler: ((progress: TaskBatchProgress) => void) | null = null;
    mocks.listenAutomationTaskBatchProgress.mockImplementation(
      async (handler: (progress: TaskBatchProgress) => void) => {
        batchProgressHandler = handler;
        return () => {};
      },
    );
    mocks.processAutomationTasks.mockReturnValue(pendingBatch.promise);

    render(<App />);

    fireEvent.click(screen.getByRole("button", { name: "Browse unit folder" }));
    await waitFor(() => {
      expect(screen.getByLabelText("Selected test unit")).toHaveValue("262343000072");
    });
    fireEvent.change(screen.getByLabelText("Transformer SN"), {
      target: { value: "TX-SCROLL" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Start" }));

    expect(await screen.findByText("Previous Tests Detected")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Batch Run Previous Tests" }));

    await waitFor(() => {
      expect(mocks.processAutomationTasks).toHaveBeenCalled();
    });
    act(() => {
      batchProgressHandler?.({
        unit_folder: "C:\\PDU500\\262343000072",
        task_id: "208v-transformer",
        state: "fail",
        message: "208V Transformer Check failed verification",
        index: 1,
        total: 2,
      });
      batchProgressHandler?.({
        unit_folder: "C:\\PDU500\\262343000072",
        task_id: "208v-system-100% Load",
        state: "processing",
        message: "Processing 100% Load",
        index: 2,
        total: 2,
      });
      batchProgressHandler?.({
        unit_folder: "C:\\PDU500\\262343000072",
        task_id: "208v-system-100% Load",
        state: "fail",
        message: "100% Load failed verification",
        index: 2,
        total: 2,
      });
    });

    const failedTask = screen.getByRole("button", { name: "208V Transformer Check" });
    expect(failedTask).not.toHaveAttribute("data-current-task", "true");
    expect(screen.queryByText("Accuracy Check Failed")).not.toBeInTheDocument();
    expect(scrollIntoView).not.toHaveBeenCalled();

    await act(async () => {
      pendingBatch.resolve(batchResult);
      await pendingBatch.promise;
    });

    expect(await screen.findByText("Accuracy Check Failed")).toBeInTheDocument();
    expect(failedTask).toHaveAttribute("data-current-task", "true");
    expect(screen.getByRole("button", { name: "Pause" })).toBeInTheDocument();
    await waitFor(() => {
      expect(scrollIntoView).toHaveBeenCalled();
    });

    await act(async () => {
      await new Promise((resolve) => window.setTimeout(resolve, 3_200));
    });

    expect(failedTask).toHaveAttribute("data-current-task", "true");

    mocks.acceptAutomationTaskFailure.mockResolvedValue({
      ...summary,
      tasks: [
        { ...detectedTransformerTask(), accepted: true, state: "pass" },
        { ...readySystemTask, accepted: false, state: "fail" },
      ],
    });
    const firstFailureRow = failedTask.parentElement;
    expect(firstFailureRow).not.toBeNull();
    fireEvent.click(within(firstFailureRow as HTMLElement).getByRole("button", { name: "Pass" }));

    await waitFor(() => {
      expect(mocks.acceptAutomationTaskFailure).toHaveBeenCalledWith(
        "C:\\PDU500\\262343000072",
        "208v-transformer",
      );
    });
    expect(screen.queryByText("Accuracy Check Failed")).not.toBeInTheDocument();
    expect(screen.getByText("Current Check Failed")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "100% Load" })).toHaveAttribute(
      "data-current-task",
      "true",
    );
    expect(screen.getByRole("button", { name: "Pause" })).toBeInTheDocument();
  });
});
