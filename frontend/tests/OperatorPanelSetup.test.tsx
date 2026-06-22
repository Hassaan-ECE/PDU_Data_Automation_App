import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  chooseUnitFolder: vi.fn(),
  getBackendStatus: vi.fn(),
  getSuggestedUnitFolder: vi.fn(),
  loadLayoutProfile: vi.fn(),
  openReportLocation: vi.fn(),
  openReportPath: vi.fn(),
  processAutomationTask: vi.fn(),
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
  scanUnitFolder: mocks.scanUnitFolder,
  setupUnitFolder: mocks.setupUnitFolder,
}));

import { App } from "@/app/App";

function unitSummary(
  unitFolder: string,
  serialNumber: string,
  tasks: Array<{
    detected_steps: number[];
    label: string;
    latest_csv: string | null;
    latest_csv_created_ms: number | null;
    latest_csv_readable: boolean | null;
    state: "off" | "detected" | "waiting" | "processing" | "pass" | "warning" | "fail";
    step: string;
    task_id: string;
    timer_start_ms: number | null;
  }> = [],
) {
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

function detectedTransformerTask() {
  return {
    detected_steps: [14],
    label: "208V Transformer Check",
    latest_csv: "C:\\PDU500\\262343000072\\STEP14.csv",
    latest_csv_created_ms: Date.now() - 60_000,
    latest_csv_readable: true,
    state: "detected" as const,
    step: "14",
    task_id: "208v-transformer",
    timer_start_ms: null,
  };
}

describe("OperatorPanel Transformer SN setup", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.getBackendStatus.mockResolvedValue({
      app_name: "PDU Data Automation",
      backend: "tauri-rust",
      process_uptime_ms: 1,
      version: "0.2.7",
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
  });

  it("opens setup on Start and requires a Transformer SN before confirming", async () => {
    render(<App />);

    fireEvent.click(screen.getByRole("button", { name: "Start" }));

    expect(await screen.findByText("Unit Setup")).toBeInTheDocument();
    await waitFor(() => {
      expect(screen.getByLabelText("Selected unit folder")).toHaveValue("C:\\PDU500\\262343000072");
    });

    const continueButton = screen.getByRole("button", { name: "Continue" });
    expect(continueButton).toBeDisabled();

    fireEvent.change(screen.getByLabelText("Transformer SN"), {
      target: { value: "TX-12345" },
    });

    expect(continueButton).toBeEnabled();
  });

  it("uses a browsed folder and Transformer SN for setup before starting", async () => {
    mocks.chooseUnitFolder.mockResolvedValue("C:\\PDU500\\11111111");
    mocks.scanUnitFolder.mockResolvedValue(unitSummary("C:\\PDU500\\11111111", "11111111"));
    mocks.setupUnitFolder.mockResolvedValue(unitSummary("C:\\PDU500\\11111111", "11111111"));

    render(<App />);

    fireEvent.click(screen.getByRole("button", { name: "Start" }));
    expect(await screen.findByText("Unit Setup")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Browse unit folder" }));

    await waitFor(() => {
      expect(screen.getByLabelText("Selected unit folder")).toHaveValue("C:\\PDU500\\11111111");
    });

    fireEvent.change(screen.getByLabelText("Transformer SN"), {
      target: { value: "TX-999" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Continue" }));

    await waitFor(() => {
      expect(mocks.setupUnitFolder).toHaveBeenCalledWith("C:\\PDU500\\11111111", "TX-999", "11111111");
    });
    await waitFor(() => {
      expect(screen.queryByText("Unit Setup")).not.toBeInTheDocument();
    });
  });

  it("shows setup errors and does not dismiss the dialog", async () => {
    mocks.setupUnitFolder.mockRejectedValue({ code: "workbook_locked", message: "main report workbook is locked" });

    render(<App />);

    fireEvent.click(screen.getByRole("button", { name: "Start" }));
    expect(await screen.findByText("Unit Setup")).toBeInTheDocument();

    await waitFor(() => {
      expect(screen.getByLabelText("Selected unit folder")).toHaveValue("C:\\PDU500\\262343000072");
    });

    fireEvent.change(screen.getByLabelText("Transformer SN"), {
      target: { value: "TX-LOCKED" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Continue" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("main report workbook is locked");
    expect(screen.getByText("Unit Setup")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Start" })).toBeInTheDocument();
  });

  it("continues into the previous-steps prompt when setup finds detected tests", async () => {
    mocks.setupUnitFolder.mockResolvedValue(
      unitSummary("C:\\PDU500\\262343000072", "262343000072", [detectedTransformerTask()]),
    );

    render(<App />);

    fireEvent.click(screen.getByRole("button", { name: "Start" }));
    expect(await screen.findByText("Unit Setup")).toBeInTheDocument();

    await waitFor(() => {
      expect(screen.getByLabelText("Selected unit folder")).toHaveValue("C:\\PDU500\\262343000072");
    });

    fireEvent.change(screen.getByLabelText("Transformer SN"), {
      target: { value: "TX-DETECTED" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Continue" }));

    expect(await screen.findByText("Previous Tests Detected")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Run Previous Tests" })).toBeInTheDocument();
    expect(screen.queryByText("Unit Setup")).not.toBeInTheDocument();
  });
});
