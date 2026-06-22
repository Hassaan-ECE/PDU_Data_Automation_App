import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  chooseUnitFolder: vi.fn(),
  getBackendStatus: vi.fn(),
  getSuggestedUnitFolder: vi.fn(),
  loadLayoutProfile: vi.fn(),
  openReportLocation: vi.fn(),
  openReportPath: vi.fn(),
  processAutomationTask: vi.fn(),
  saveTransformerSn: vi.fn(),
  scanUnitFolder: vi.fn(),
  setupUnitFolder: vi.fn(),
}));

vi.mock("@/integrations/tauri/backend", () => ({
  chooseUnitFolder: mocks.chooseUnitFolder,
  getBackendStatus: mocks.getBackendStatus,
  getSuggestedUnitFolder: mocks.getSuggestedUnitFolder,
  isTauriRuntime: () => false,
  loadLayoutProfile: mocks.loadLayoutProfile,
  openReportLocation: mocks.openReportLocation,
  openReportPath: mocks.openReportPath,
  processAutomationTask: mocks.processAutomationTask,
  saveTransformerSn: mocks.saveTransformerSn,
  scanUnitFolder: mocks.scanUnitFolder,
  setupUnitFolder: mocks.setupUnitFolder,
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

describe("OperatorPanel inline Transformer SN setup", () => {
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
      detected_count: 0,
      serial_number: "262343000072",
      unit_folder: "C:\\PDU500\\262343000072",
    });
    mocks.scanUnitFolder.mockResolvedValue(unitSummary("C:\\PDU500\\262343000072", "262343000072"));
    mocks.setupUnitFolder.mockResolvedValue(unitSummary("C:\\PDU500\\262343000072", "262343000072"));
    mocks.saveTransformerSn.mockResolvedValue(undefined);
  });

  afterEach(() => {
    delete (HTMLElement.prototype as { scrollIntoView?: unknown }).scrollIntoView;
  });

  async function selectUnit(unitFolder = "C:\\PDU500\\262343000072", serialNumber = "262343000072") {
    mocks.chooseUnitFolder.mockResolvedValue(unitFolder);
    mocks.scanUnitFolder.mockResolvedValue(unitSummary(unitFolder, serialNumber));
    fireEvent.click(screen.getByRole("button", { name: "Browse unit folder" }));

    await waitFor(() => {
      expect(screen.getByLabelText("Selected test unit")).toHaveValue(serialNumber);
    });
  }

  it("shows the inline unit selector and Transformer SN input without opening setup", async () => {
    render(<App />);

    const unitSelector = screen.getByPlaceholderText("Select Test Unit...");
    expect(screen.getByPlaceholderText("Transformer SN...")).toBeInTheDocument();

    expect(unitSelector).toHaveValue("");
    expect(mocks.getSuggestedUnitFolder).not.toHaveBeenCalled();
    expect(screen.queryByText("Unit Setup")).not.toBeInTheDocument();
  });

  it("uses a browsed folder and Transformer SN for setup before starting", async () => {
    mocks.chooseUnitFolder.mockResolvedValue("C:\\PDU500\\11111111");
    mocks.scanUnitFolder.mockResolvedValue(unitSummary("C:\\PDU500\\11111111", "11111111"));
    mocks.setupUnitFolder.mockResolvedValue(unitSummary("C:\\PDU500\\11111111", "11111111"));

    render(<App />);

    await selectUnit("C:\\PDU500\\11111111", "11111111");

    fireEvent.change(screen.getByLabelText("Transformer SN"), {
      target: { value: "TX-999" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Start" }));

    await waitFor(() => {
      expect(mocks.setupUnitFolder).toHaveBeenCalledWith("C:\\PDU500\\11111111", "TX-999", "11111111");
    });
    expect(screen.queryByText("Unit Setup")).not.toBeInTheDocument();
  });

  it("shows setup errors inline and does not fake a saved Transformer SN", async () => {
    mocks.setupUnitFolder.mockRejectedValue({ code: "workbook_locked", message: "main report workbook is locked" });

    render(<App />);

    await selectUnit();

    fireEvent.change(screen.getByLabelText("Transformer SN"), {
      target: { value: "TX-LOCKED" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Start" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("main report workbook is locked");
    expect(screen.queryByText("Unit Setup")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Start" })).toBeInTheDocument();
  });

  it("continues into the previous-tests prompt after inline setup succeeds", async () => {
    mocks.setupUnitFolder.mockResolvedValue(
      unitSummary("C:\\PDU500\\262343000072", "262343000072", [detectedTransformerTask()]),
    );

    render(<App />);

    await selectUnit();

    fireEvent.change(screen.getByLabelText("Transformer SN"), {
      target: { value: "TX-DETECTED" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Start" }));

    expect(await screen.findByText("Previous Tests Detected")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Run Previous Tests" })).toBeInTheDocument();
  });

  it("saves a Transformer SN after setup and blocks report opening while missing", async () => {
    render(<App />);

    await selectUnit();

    fireEvent.click(screen.getByRole("button", { name: "Start" }));
    await waitFor(() => {
      expect(mocks.setupUnitFolder).toHaveBeenCalled();
    });

    fireEvent.click(await screen.findByRole("button", { name: "Pause" }));
    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Resume" })).toBeInTheDocument();
    });

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

    fireEvent.click(screen.getByRole("button", { name: "Start" }));
    await waitFor(() => {
      expect(mocks.setupUnitFolder).toHaveBeenCalled();
    });
    fireEvent.click(await screen.findByRole("button", { name: "Pause" }));
    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Resume" })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByText("208V Transformer Check"));
    expect(await screen.findByText("Step Failed")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Open" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Transformer SN is missing.");
    expect(mocks.openReportPath).not.toHaveBeenCalled();
  });
});
