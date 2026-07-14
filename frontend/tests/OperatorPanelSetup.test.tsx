import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  chooseUnitFolder: vi.fn(),
  chooseSharedNotificationsFolder: vi.fn(),
  changeSettingsPassword: vi.fn(),
  getAppNotificationSettings: vi.fn(),
  getBackendStatus: vi.fn(),
  getNotificationStatus: vi.fn(),
  getSuggestedUnitFolder: vi.fn(),
  listenAutomationTaskBatchProgress: vi.fn(),
  loadLayoutProfile: vi.fn(),
  openPrintReportDialog: vi.fn(),
  openReportLocation: vi.fn(),
  openReportPath: vi.fn(),
  postShiftSummary: vi.fn(),
  previewShiftSummary: vi.fn(),
  processAutomationTasks: vi.fn(),
  processAutomationTask: vi.fn(),
  saveFinalOperatorName: vi.fn(),
  saveAppNotificationSettings: vi.fn(),
  sendNotificationTest: vi.fn(),
  saveTransformerSn: vi.fn(),
  scanUnitFolder: vi.fn(),
  setupUnitFolder: vi.fn(),
  validateReadyForPrint: vi.fn(),
  verifySettingsPassword: vi.fn(),
}));

vi.mock("@/integrations/tauri/backend", () => ({
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
  openPrintReportDialog: mocks.openPrintReportDialog,
  openReportLocation: mocks.openReportLocation,
  openReportPath: mocks.openReportPath,
  postShiftSummary: mocks.postShiftSummary,
  previewShiftSummary: mocks.previewShiftSummary,
  processAutomationTasks: mocks.processAutomationTasks,
  processAutomationTask: mocks.processAutomationTask,
  saveFinalOperatorName: mocks.saveFinalOperatorName,
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

function waitingTransformerTask(processReady: boolean): MockTask {
  return {
    ...detectedTransformerTask(),
    phase_deadline_ms: processReady ? Date.now() - 1 : Date.now() + 30_000,
    process_ready: processReady,
    state: "waiting",
    wait_phase: processReady ? "ready" : "timing",
  };
}

describe("OperatorPanel inline Transformer SN setup", () => {
  let scrollIntoView: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    vi.clearAllMocks();
    window.localStorage.clear();
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
      detected_count: 0,
      serial_number: "262343000072",
      unit_folder: "C:\\PDU500\\262343000072",
    });
    mocks.getNotificationStatus.mockResolvedValue(null);
    mocks.getAppNotificationSettings.mockResolvedValue({
      enabled: true,
      events: { changeover: true, complete: true, problem: true, stuck: false, summary: true },
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
      stations: [
        { id: "test-station-1", name: "Test Station 1", role: "floor" },
        { id: "test-station-3", name: "Test Station 3", role: "floor" },
        { id: "test-station-4", name: "Test Station 4", role: "floor" },
        { id: "pdu-lab", name: "PDU Lab", role: "floor" },
      ],
      floor_sync: {
        configured: false,
        source: "local",
        updated_at: null,
        updated_by_station_id: null,
        message: "Shared folder not set — settings stay on this PC only.",
      },
    });
    mocks.saveAppNotificationSettings.mockImplementation(async (request) => ({
      ...request,
      webhook_configured: Boolean(request.teams_webhook_url),
      is_summary_poster: request.station_id === (request.summary_poster_station_id || "pdu-lab"),
      stations: request.stations ?? [
        { id: "test-station-1", name: "Test Station 1", role: "floor" },
        { id: "test-station-3", name: "Test Station 3", role: "floor" },
        { id: "test-station-4", name: "Test Station 4", role: "floor" },
        { id: "pdu-lab", name: "PDU Lab", role: "floor" },
      ],
      floor_sync: {
        configured: Boolean(request.shared_shift_log_path?.trim()),
        source: request.shared_shift_log_path?.trim() ? "floor" : "local",
        updated_at: null,
        updated_by_station_id: null,
        message: request.shared_shift_log_path?.trim()
          ? "Syncing via shared folder."
          : "Shared folder not set — settings stay on this PC only.",
      },
    }));
    mocks.changeSettingsPassword.mockResolvedValue(undefined);
    mocks.sendNotificationTest.mockResolvedValue(undefined);
    mocks.previewShiftSummary.mockResolvedValue(null);
    mocks.postShiftSummary.mockResolvedValue(null);
    mocks.verifySettingsPassword.mockImplementation(async (password) => password === "0601");
    mocks.listenAutomationTaskBatchProgress.mockResolvedValue(() => {});
    mocks.scanUnitFolder.mockResolvedValue(unitSummary("C:\\PDU500\\262343000072", "262343000072"));
    mocks.setupUnitFolder.mockResolvedValue(unitSummary("C:\\PDU500\\262343000072", "262343000072"));
    mocks.saveFinalOperatorName.mockResolvedValue("C:\\PDU500\\262343000072\\print.xlsx");
    mocks.processAutomationTasks.mockResolvedValue(null);
    mocks.saveTransformerSn.mockResolvedValue(undefined);
    mocks.openPrintReportDialog.mockResolvedValue(undefined);
    mocks.validateReadyForPrint.mockResolvedValue({
      blocking_issues: [],
      message: "Ready to print.",
      ready: true,
    });
  });

  afterEach(() => {
    delete (HTMLElement.prototype as { scrollIntoView?: unknown }).scrollIntoView;
    window.localStorage.clear();
  });

  async function selectUnit(
    unitFolder = "C:\\PDU500\\262343000072",
    serialNumber = "262343000072",
    summary = unitSummary(unitFolder, serialNumber),
  ) {
    mocks.chooseUnitFolder.mockResolvedValue(unitFolder);
    mocks.setupUnitFolder.mockResolvedValue(summary);
    fireEvent.click(screen.getByRole("button", { name: "Browse unit folder" }));

    await waitFor(() => {
      expect(screen.getByLabelText("Selected test unit")).toHaveValue(serialNumber);
    });
    await waitFor(() => {
      expect(mocks.setupUnitFolder).toHaveBeenCalledWith(unitFolder, "", serialNumber);
    });
  }

  async function setupUnitReadyForPrint() {
    await selectUnit();

    fireEvent.change(screen.getByLabelText("Transformer SN"), {
      target: { value: "TX-PRINT" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save Transformer SN" }));
    await waitFor(() => {
      expect(mocks.saveTransformerSn).toHaveBeenCalledWith("C:\\PDU500\\262343000072", "TX-PRINT");
    });
  }

  it("keeps the operator panel usable without a Teams status footer line", async () => {
    render(<App />);

    expect(screen.queryByText(/Teams alerts ·/)).not.toBeInTheDocument();
    expect(await screen.findByRole("button", { name: "Browse unit folder" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Open notification settings" })).toBeEnabled();
  });

  it("shows the inline unit selector and Transformer SN input without opening setup", async () => {
    render(<App />);

    const unitSelector = screen.getByPlaceholderText("Select Test Unit...");
    expect(screen.getByPlaceholderText("Transformer SN...")).toBeInTheDocument();

    expect(unitSelector).toHaveValue("");
    expect(mocks.getSuggestedUnitFolder).not.toHaveBeenCalled();
    expect(screen.queryByText("Unit Setup")).not.toBeInTheDocument();
  });

  it("renders Open Report and Print Report side-by-side", async () => {
    render(<App />);

    const reportActions = screen.getByLabelText("Report actions");

    expect(reportActions).toHaveClass("grid", "grid-cols-2");
    expect(within(reportActions).getByRole("button", { name: "Open Report" })).toBeInTheDocument();
    expect(within(reportActions).getByRole("button", { name: "Print Report" })).toBeInTheDocument();
  });

  it("sets up reports when a folder is selected and saves Transformer SN before starting", async () => {
    mocks.chooseUnitFolder.mockResolvedValue("C:\\PDU500\\11111111");
    mocks.setupUnitFolder.mockResolvedValue(unitSummary("C:\\PDU500\\11111111", "11111111"));

    render(<App />);

    await selectUnit("C:\\PDU500\\11111111", "11111111");

    fireEvent.change(screen.getByLabelText("Transformer SN"), {
      target: { value: "TX-999" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save Transformer SN" }));

    await waitFor(() => {
      expect(mocks.setupUnitFolder).toHaveBeenCalledWith("C:\\PDU500\\11111111", "", "11111111");
    });
    await waitFor(() => {
      expect(mocks.saveTransformerSn).toHaveBeenCalledWith("C:\\PDU500\\11111111", "TX-999");
    });
    expect(screen.queryByText("Unit Setup")).not.toBeInTheDocument();
  });

  it("shows setup errors inline and does not fake a saved Transformer SN", async () => {
    mocks.setupUnitFolder.mockRejectedValue({ code: "workbook_locked", message: "main report workbook is locked" });

    render(<App />);

    mocks.chooseUnitFolder.mockResolvedValue("C:\\PDU500\\262343000072");
    fireEvent.click(screen.getByRole("button", { name: "Browse unit folder" }));

    await screen.findByText("main report workbook is locked");

    fireEvent.change(screen.getByLabelText("Transformer SN"), {
      target: { value: "TX-LOCKED" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save Transformer SN" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("main report workbook is locked");
    expect(screen.queryByText("Unit Setup")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Start" })).toBeInTheDocument();
  });

  it("uses the batch command when running detected previous tests", async () => {
    mocks.setupUnitFolder.mockResolvedValue(
      unitSummary("C:\\PDU500\\262343000072", "262343000072", [detectedTransformerTask()]),
    );
    mocks.processAutomationTasks.mockResolvedValue({
      committed: true,
      committed_count: 1,
      message: "Batch processed 1 task",
      results: [
        {
          code: 0,
          failure: null,
          log: [],
          message: "208V Transformer Check processed",
          print_report_path: null,
          report_path: "C:\\PDU500\\262343000072\\main.xlsx",
          state: "pass",
          task_id: "208v-transformer",
        },
      ],
      stopped_task_id: null,
    });

    render(<App />);

    await selectUnit("C:\\PDU500\\262343000072", "262343000072", unitSummary("C:\\PDU500\\262343000072", "262343000072", [detectedTransformerTask()]));

    fireEvent.change(screen.getByLabelText("Transformer SN"), {
      target: { value: "TX-DETECTED" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Start" }));

    expect(await screen.findByText("Previous Tests Detected")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Open notification settings" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Batch Run Previous Tests" }));

    await waitFor(() => {
      expect(mocks.processAutomationTasks).toHaveBeenCalledWith("C:\\PDU500\\262343000072", [
        "208v-transformer",
      ]);
    });
    expect(mocks.processAutomationTask).not.toHaveBeenCalled();
  });

  it("processes a waiting task once a later scan marks it ready", async () => {
    const waitingSummary = unitSummary("C:\\PDU500\\262343000072", "262343000072", [
      waitingTransformerTask(false),
    ]);
    const readySummary = unitSummary("C:\\PDU500\\262343000072", "262343000072", [
      waitingTransformerTask(true),
    ]);
    mocks.setupUnitFolder.mockResolvedValue(waitingSummary);
    mocks.scanUnitFolder.mockResolvedValue(readySummary);
    mocks.processAutomationTask.mockResolvedValue({
      code: 0,
      csv_fingerprint: "fixture",
      failure: null,
      log: [],
      message: "208V Transformer Check processed",
      print_report_path: null,
      report_path: "C:\\PDU500\\262343000072\\main.xlsx",
      source_csv_path: "STEP14.csv",
      state: "pass",
      task_id: "208v-transformer",
    });

    render(<App />);
    await selectUnit("C:\\PDU500\\262343000072", "262343000072", waitingSummary);
    fireEvent.change(screen.getByLabelText("Transformer SN"), {
      target: { value: "TX-WAITING" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Start" }));

    await waitFor(() => {
      expect(mocks.scanUnitFolder).toHaveBeenCalledWith("C:\\PDU500\\262343000072");
    });
    await waitFor(() => {
      expect(mocks.processAutomationTask).toHaveBeenCalledTimes(1);
      expect(mocks.processAutomationTask).toHaveBeenCalledWith(
        "C:\\PDU500\\262343000072",
        "208v-transformer",
      );
    });
  });

  it("can cancel the previous-tests prompt without starting", async () => {
    render(<App />);

    await selectUnit(
      "C:\\PDU500\\262343000072",
      "262343000072",
      unitSummary("C:\\PDU500\\262343000072", "262343000072", [detectedTransformerTask()]),
    );

    fireEvent.click(screen.getByRole("button", { name: "Start" }));

    expect(await screen.findByText("Previous Tests Detected")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));

    await waitFor(() => {
      expect(screen.queryByText("Previous Tests Detected")).not.toBeInTheDocument();
    });
    expect(mocks.processAutomationTasks).not.toHaveBeenCalled();
    expect(mocks.processAutomationTask).not.toHaveBeenCalled();
  });

  it("opens the Print Report operator modal with default names", async () => {
    render(<App />);

    await setupUnitReadyForPrint();

    fireEvent.click(screen.getByRole("button", { name: "Print Report" }));

    expect(await screen.findByLabelText("Operator name")).toHaveValue("Sean");
    expect(screen.queryByRole("option", { name: "Sean" })).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Show operator names" }));
    const savedOperators = await screen.findByRole("listbox", { name: "Saved operators" });

    expect(within(savedOperators).getByRole("option", { name: "Sean" })).toBeInTheDocument();
    expect(within(savedOperators).getByRole("option", { name: "Long" })).toBeInTheDocument();
    expect(within(savedOperators).getByRole("option", { name: "Jose" })).toBeInTheDocument();
  });

  it("accepts and persists a new typed operator name", async () => {
    render(<App />);

    await setupUnitReadyForPrint();

    fireEvent.click(screen.getByRole("button", { name: "Print Report" }));
    fireEvent.change(await screen.findByLabelText("Operator name"), {
      target: { value: "Priya" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Confirm & Print" }));

    await waitFor(() => {
      expect(mocks.saveFinalOperatorName).toHaveBeenCalledWith("C:\\PDU500\\262343000072", "Priya");
    });
    await waitFor(() => {
      expect(mocks.openPrintReportDialog).toHaveBeenCalledWith("C:\\PDU500\\262343000072");
    });
    expect(JSON.parse(window.localStorage.getItem("pdu.operatorNames") ?? "[]")).toEqual([
      "Sean",
      "Long",
      "Jose",
      "Priya",
    ]);
  });

  it("filters operator dropdown suggestions while typing", async () => {
    render(<App />);

    await setupUnitReadyForPrint();

    fireEvent.click(screen.getByRole("button", { name: "Print Report" }));
    fireEvent.change(await screen.findByLabelText("Operator name"), {
      target: { value: "L" },
    });

    const savedOperators = await screen.findByRole("listbox", { name: "Saved operators" });

    expect(within(savedOperators).getByRole("option", { name: "Long" })).toBeInTheDocument();
    expect(within(savedOperators).queryByRole("option", { name: "Sean" })).not.toBeInTheDocument();
    expect(within(savedOperators).queryByRole("option", { name: "Jose" })).not.toBeInTheDocument();
  });

  it("removes an existing operator name from the local list", async () => {
    render(<App />);

    await setupUnitReadyForPrint();

    fireEvent.click(screen.getByRole("button", { name: "Print Report" }));
    fireEvent.click(await screen.findByRole("button", { name: "Show operator names" }));
    fireEvent.click(await screen.findByRole("button", { name: "Remove Long" }));

    expect(screen.queryByRole("option", { name: "Long" })).not.toBeInTheDocument();
    expect(JSON.parse(window.localStorage.getItem("pdu.operatorNames") ?? "[]")).toEqual([
      "Sean",
      "Jose",
    ]);
  });

  it("blocks blank operator name confirmation", async () => {
    render(<App />);

    await setupUnitReadyForPrint();

    fireEvent.click(screen.getByRole("button", { name: "Print Report" }));
    fireEvent.change(await screen.findByLabelText("Operator name"), {
      target: { value: "   " },
    });
    fireEvent.click(screen.getByRole("button", { name: "Confirm & Print" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("Operator name is required.");
    expect(mocks.saveFinalOperatorName).not.toHaveBeenCalled();
    expect(mocks.openPrintReportDialog).not.toHaveBeenCalled();
  });

  it("calls backend save and print-dialog commands on confirm", async () => {
    render(<App />);

    await setupUnitReadyForPrint();

    fireEvent.click(screen.getByRole("button", { name: "Print Report" }));
    await waitFor(() => {
      expect(mocks.validateReadyForPrint).toHaveBeenCalledWith("C:\\PDU500\\262343000072");
    });
    fireEvent.click(await screen.findByRole("button", { name: "Confirm & Print" }));

    await waitFor(() => {
      expect(mocks.saveFinalOperatorName).toHaveBeenCalledWith("C:\\PDU500\\262343000072", "Sean");
    });
    await waitFor(() => {
      expect(mocks.openPrintReportDialog).toHaveBeenCalledWith("C:\\PDU500\\262343000072");
    });
  });

  it("shows backend print blockers before collecting the final operator name", async () => {
    mocks.validateReadyForPrint.mockResolvedValueOnce({
      blocking_issues: [
        {
          code: "task_fail",
          label: "208V Transformer Check",
          reason: "Task failed and has not been explicitly accepted.",
          task_id: "208v-transformer",
        },
      ],
      message: "Report is not ready to print. 1 blocking issue must be resolved.",
      ready: false,
    });

    render(<App />);

    await setupUnitReadyForPrint();

    fireEvent.click(screen.getByRole("button", { name: "Print Report" }));

    const blockerTitle = await screen.findByText("Print Blocked");
    const blockerDialog = blockerTitle.closest("section");

    expect(blockerDialog).not.toBeNull();
    expect(within(blockerDialog as HTMLElement).getByText("208V Transformer Check")).toBeInTheDocument();
    expect(
      within(blockerDialog as HTMLElement).getByText("Task failed and has not been explicitly accepted."),
    ).toBeInTheDocument();
    expect(screen.queryByLabelText("Operator name")).not.toBeInTheDocument();
    expect(mocks.saveFinalOperatorName).not.toHaveBeenCalled();
    expect(mocks.openPrintReportDialog).not.toHaveBeenCalled();
  });

  it("shows print-dialog error details when Excel automation fails", async () => {
    mocks.openPrintReportDialog.mockRejectedValueOnce({
      code: "print_dialog_failed",
      details: "PrintPreviewAndPrint failed: command is unavailable",
      message: "The Excel print dialog could not be opened for the print report.",
    });

    render(<App />);

    await setupUnitReadyForPrint();

    fireEvent.click(screen.getByRole("button", { name: "Print Report" }));
    fireEvent.click(await screen.findByRole("button", { name: "Confirm & Print" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "PrintPreviewAndPrint failed: command is unavailable",
    );
    expect(screen.getByLabelText("Operator name")).toBeInTheDocument();
  });

  it("blocks Print Report when Transformer SN is missing or cannot be saved", async () => {
    render(<App />);

    await selectUnit();

    fireEvent.click(screen.getByRole("button", { name: "Print Report" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Transformer SN is missing.");
    expect(screen.queryByLabelText("Operator name")).not.toBeInTheDocument();

    mocks.saveTransformerSn.mockRejectedValueOnce({ code: "workbook_locked", message: "SN save failed" });
    fireEvent.change(screen.getByLabelText("Transformer SN"), {
      target: { value: "TX-UNSAVED" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Print Report" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("SN save failed");
    expect(screen.queryByLabelText("Operator name")).not.toBeInTheDocument();
  });

  it("saves a Transformer SN after setup and blocks report opening while missing", async () => {
    render(<App />);

    await selectUnit();

    fireEvent.click(screen.getByRole("button", { name: "Open Report" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Transformer SN is missing.");
    expect(mocks.openReportPath).not.toHaveBeenCalled();

    fireEvent.change(screen.getByLabelText("Transformer SN"), {
      target: { value: "TX-LATE" },
    });
    fireEvent.blur(screen.getByLabelText("Transformer SN"));

    await waitFor(() => {
      expect(mocks.saveTransformerSn).toHaveBeenCalledWith("C:\\PDU500\\262343000072", "TX-LATE");
    });
    expect(await screen.findByText("Saved")).toBeInTheDocument();
  });

  it("blocks failure report opening while Transformer SN is missing", async () => {
    mocks.processAutomationTask.mockResolvedValue({
      code: 1,
      failure: null,
      log: [],
      message: "Report step failed",
      print_report_path: null,
      report_path: "C:\\PDU500\\262343000072\\main.xlsx",
      state: "fail",
      task_id: "208v-transformer",
    });

    render(<App />);

    await selectUnit();

    fireEvent.click(screen.getByText("208V Transformer Check"));
    expect(await screen.findByText("Step Failed")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Open" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Transformer SN is missing.");
    expect(mocks.openReportPath).not.toHaveBeenCalled();
  });

  it("opens basic notification settings without password and gates advanced", async () => {
    render(<App />);

    fireEvent.click(screen.getByRole("button", { name: "Open notification settings" }));
    expect(await screen.findByRole("heading", { level: 1, name: "Settings" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^Shifts$/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Summary options/i })).toBeInTheDocument();
    expect(screen.getByRole("region", { name: /End of shift/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Post Summary/i })).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /Station & Identities/i }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /Teams & Notifications/i }),
    ).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /^Advanced$/i }));
    expect(await screen.findByLabelText("Password")).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Password"), { target: { value: "wrong" } });
    fireEvent.click(screen.getByRole("button", { name: "Unlock" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Incorrect password");

    fireEvent.change(screen.getByLabelText("Password"), { target: { value: "0601" } });
    fireEvent.click(screen.getByRole("button", { name: "Unlock" }));
    expect(
      await screen.findByRole("button", { name: /Station & Identities/i }),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Teams & Notifications/i })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /^End of shift$/i })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /Teams & Notifications/i }));
    expect(await screen.findByLabelText("Teams webhook URL")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Back to settings menu" }));
    fireEvent.click(screen.getByRole("button", { name: "Back to settings menu" }));
    fireEvent.click(screen.getByRole("button", { name: "Back to operator panel" }));
    expect(screen.getByRole("button", { name: "Open notification settings" })).toBeInTheDocument();
  });
});
