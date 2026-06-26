import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  chooseUnitFolder: vi.fn(),
  getBackendStatus: vi.fn(),
  getSuggestedUnitFolder: vi.fn(),
  listenAutomationTaskBatchProgress: vi.fn(),
  loadLayoutProfile: vi.fn(),
  openReportLocation: vi.fn(),
  openReportPath: vi.fn(),
  processAutomationTasks: vi.fn(),
  processAutomationTask: vi.fn(),
  saveTransformerSn: vi.fn(),
  scanUnitFolder: vi.fn(),
  setupUnitFolder: vi.fn(),
  validateReadyForPrint: vi.fn(),
}));

vi.mock("@/integrations/tauri/backend", () => ({
  chooseUnitFolder: mocks.chooseUnitFolder,
  getBackendStatus: mocks.getBackendStatus,
  getSuggestedUnitFolder: mocks.getSuggestedUnitFolder,
  isTauriRuntime: () => false,
  listenAutomationTaskBatchProgress: mocks.listenAutomationTaskBatchProgress,
  loadLayoutProfile: mocks.loadLayoutProfile,
  openReportLocation: mocks.openReportLocation,
  openReportPath: mocks.openReportPath,
  processAutomationTasks: mocks.processAutomationTasks,
  processAutomationTask: mocks.processAutomationTask,
  saveTransformerSn: mocks.saveTransformerSn,
  scanUnitFolder: mocks.scanUnitFolder,
  setupUnitFolder: mocks.setupUnitFolder,
  validateReadyForPrint: mocks.validateReadyForPrint,
}));

import { App } from "@/app/App";

type MockTask = {
  detected_steps: number[];
  label: string;
  latest_csv: string | null;
  latest_csv_created_ms: number | null;
  latest_csv_readable: boolean | null;
  state: "off" | "detected" | "waiting" | "processing" | "pass" | "warning" | "fail";
  step: string;
  task_id: string;
  timer_start_ms: number | null;
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
    state: "detected",
    step: "15",
    task_id: "208v-system-100% Load",
    timer_start_ms: Date.now(),
  };
}

function detectedTransformerTask(): MockTask {
  return {
    detected_steps: [14],
    label: "208V Transformer Check",
    latest_csv: "C:\\PDU500\\262343000072\\STEP14.csv",
    latest_csv_created_ms: Date.now() - 60_000,
    latest_csv_readable: true,
    state: "detected",
    step: "14",
    task_id: "208v-transformer",
    timer_start_ms: null,
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
    mocks.listenAutomationTaskBatchProgress.mockResolvedValue(() => {});
    const summary = unitSummary("C:\\PDU500\\262343000072", "262343000072", [
      detectedTransformerTask(),
      detectedSystemTask(),
    ]);
    mocks.chooseUnitFolder.mockResolvedValue("C:\\PDU500\\262343000072");
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

  it("does not auto-scroll when batch-running previous tests", async () => {
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
});
