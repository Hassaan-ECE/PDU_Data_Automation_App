import { act, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const backendMocks = vi.hoisted(() => ({
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

const updaterMocks = vi.hoisted(() => ({
  check: vi.fn(),
}));

vi.mock("@/integrations/tauri/backend", () => ({
  chooseUnitFolder: backendMocks.chooseUnitFolder,
  chooseSharedNotificationsFolder: backendMocks.chooseSharedNotificationsFolder,
  changeSettingsPassword: backendMocks.changeSettingsPassword,
  getAppNotificationSettings: backendMocks.getAppNotificationSettings,
  getBackendStatus: backendMocks.getBackendStatus,
  getNotificationStatus: backendMocks.getNotificationStatus,
  getSuggestedUnitFolder: backendMocks.getSuggestedUnitFolder,
  isTauriRuntime: () => true,
  listenAutomationTaskBatchProgress: backendMocks.listenAutomationTaskBatchProgress,
  loadLayoutProfile: backendMocks.loadLayoutProfile,
  openReportLocation: backendMocks.openReportLocation,
  openReportPath: backendMocks.openReportPath,
  postShiftSummary: backendMocks.postShiftSummary,
  previewShiftSummary: backendMocks.previewShiftSummary,
  processAutomationTasks: backendMocks.processAutomationTasks,
  processAutomationTask: backendMocks.processAutomationTask,
  saveAppNotificationSettings: backendMocks.saveAppNotificationSettings,
  sendNotificationTest: backendMocks.sendNotificationTest,
  saveTransformerSn: backendMocks.saveTransformerSn,
  scanUnitFolder: backendMocks.scanUnitFolder,
  setupUnitFolder: backendMocks.setupUnitFolder,
  validateReadyForPrint: backendMocks.validateReadyForPrint,
  verifySettingsPassword: backendMocks.verifySettingsPassword,
}));

vi.mock("@tauri-apps/plugin-updater", () => ({
  check: updaterMocks.check,
}));

import { App } from "@/app/App";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((promiseResolve, promiseReject) => {
    resolve = promiseResolve;
    reject = promiseReject;
  });

  return { promise, reject, resolve };
}

describe("OperatorPanel updater timing", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.useFakeTimers();
    updaterMocks.check.mockResolvedValue(null);
    backendMocks.chooseUnitFolder.mockResolvedValue(null);
    backendMocks.getNotificationStatus.mockResolvedValue(null);
    backendMocks.getSuggestedUnitFolder.mockResolvedValue(null);
    backendMocks.listenAutomationTaskBatchProgress.mockResolvedValue(() => {});
    backendMocks.openReportLocation.mockResolvedValue(undefined);
    backendMocks.openReportPath.mockResolvedValue(undefined);
    backendMocks.processAutomationTasks.mockResolvedValue(null);
    backendMocks.processAutomationTask.mockResolvedValue(null);
    backendMocks.saveTransformerSn.mockResolvedValue(undefined);
    backendMocks.scanUnitFolder.mockResolvedValue(null);
    backendMocks.setupUnitFolder.mockResolvedValue(null);
    backendMocks.validateReadyForPrint.mockResolvedValue({
      blocking_issues: [],
      message: "Ready to print.",
      ready: true,
    });
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("waits for startup requests to settle before scheduling the first updater check", async () => {
    const backendStatus = deferred<{
      app_name: string;
      backend: string;
      process_uptime_ms: number;
      version: string;
      window_setup_uptime_ms: number;
    }>();
    const layoutProfile = deferred<{
      display_name: string;
      profile_id: string;
      task_count: number;
      validation: { errors: string[]; warnings: string[] };
    }>();

    backendMocks.getBackendStatus.mockReturnValue(backendStatus.promise);
    backendMocks.loadLayoutProfile.mockReturnValue(layoutProfile.promise);

    render(<App />);

    await act(async () => {
      vi.advanceTimersByTime(30_000);
    });
    expect(updaterMocks.check).not.toHaveBeenCalled();

    await act(async () => {
      backendStatus.resolve({
        app_name: "PDU Data Automation",
        backend: "tauri-rust",
        process_uptime_ms: 1,
        version: "0.2.8",
        window_setup_uptime_ms: 1,
      });
      layoutProfile.resolve({
        display_name: "PDU500 0.2CT Rev02",
        profile_id: "pdu500.rev02",
        task_count: 65,
        validation: { errors: [], warnings: [] },
      });
      await Promise.resolve();
    });

    await act(async () => {
      vi.advanceTimersByTime(1_499);
    });
    expect(updaterMocks.check).not.toHaveBeenCalled();

    await act(async () => {
      vi.advanceTimersByTime(1);
      await Promise.resolve();
    });
    expect(updaterMocks.check).toHaveBeenCalledTimes(1);
  });
});
