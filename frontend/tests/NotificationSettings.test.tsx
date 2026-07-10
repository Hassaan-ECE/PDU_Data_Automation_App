import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { NotificationSettingsPage } from "@/features/settings/NotificationSettingsPage";
import { SettingsPasswordModal } from "@/features/settings/SettingsPasswordModal";
import type {
  AppNotificationSettingsView,
  NotificationRuntimeStatus,
} from "@/features/settings/settingsTypes";

function settingsFixture(): AppNotificationSettingsView {
  return {
    enabled: true,
    teams_destination_name: "PDU Testing",
    teams_webhook_url: "",
    webhook_configured: true,
    station_id: "test-station-2",
    station_name: "Test Station 2",
    idle_timeout_minutes: 30,
    events: {
      problem: true,
      complete: true,
      stuck: false,
      summary: false,
    },
    shared_shift_log_path: "",
  };
}

function runtimeStatus(
  state: NotificationRuntimeStatus["state"],
  message: string,
  updatedAt: string,
  eventKind: NotificationRuntimeStatus["event_kind"] =
    state === "ready" ? null : "test_ping",
): NotificationRuntimeStatus {
  return {
    state,
    message,
    station_name: "Test Station 2",
    destination_name: "PDU Testing",
    updated_at: updatedAt,
    event_kind: eventKind,
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((promiseResolve) => {
    resolve = promiseResolve;
  });
  return { promise, resolve };
}

function renderSettingsPage(overrides: Partial<React.ComponentProps<typeof NotificationSettingsPage>> = {}) {
  const props: React.ComponentProps<typeof NotificationSettingsPage> = {
    onBack: vi.fn(),
    loadSettings: vi.fn(async () => settingsFixture()),
    saveSettings: vi.fn(async () => null),
    changePassword: vi.fn(async () => undefined),
    sendTestPing: vi.fn(async () => undefined),
    getNotificationStatus: vi.fn(async () => null),
    chooseSharedFolder: vi.fn(async () => null),
    ...overrides,
  };

  render(<NotificationSettingsPage {...props} />);
  return props;
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe("SettingsPasswordModal", () => {
  it("keeps the gate closed and shows an inline error for a wrong password", async () => {
    const verify = vi.fn(async () => false);
    const onUnlock = vi.fn();

    render(
      <SettingsPasswordModal
        open
        verify={verify}
        onCancel={vi.fn()}
        onUnlock={onUnlock}
      />,
    );

    fireEvent.change(screen.getByLabelText("Password"), { target: { value: "wrong" } });
    fireEvent.click(screen.getByRole("button", { name: "Unlock" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("Incorrect password");
    expect(verify).toHaveBeenCalledWith("wrong");
    expect(onUnlock).not.toHaveBeenCalled();
  });

  it("unlocks after the verification callback accepts the password", async () => {
    const onUnlock = vi.fn();

    render(
      <SettingsPasswordModal
        open
        verify={vi.fn(async (password) => password === "0601")}
        onCancel={vi.fn()}
        onUnlock={onUnlock}
      />,
    );

    fireEvent.change(screen.getByLabelText("Password"), { target: { value: "0601" } });
    fireEvent.submit(screen.getByLabelText("Password").closest("form") as HTMLFormElement);

    await waitFor(() => expect(onUnlock).toHaveBeenCalledOnce());
  });

  it("contains keyboard focus and restores it when cancelled", async () => {
    const trigger = document.createElement("button");
    trigger.textContent = "Open settings";
    document.body.append(trigger);
    trigger.focus();
    const onCancel = vi.fn();
    const { rerender } = render(
      <SettingsPasswordModal
        open
        verify={vi.fn(async () => false)}
        onCancel={onCancel}
        onUnlock={vi.fn()}
      />,
    );

    expect(screen.getByLabelText("Password")).toHaveFocus();
    fireEvent.keyDown(window, { key: "Tab", shiftKey: true });
    expect(screen.getByRole("button", { name: "Unlock" })).toHaveFocus();
    fireEvent.keyDown(window, { key: "Tab" });
    expect(screen.getByLabelText("Password")).toHaveFocus();

    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    rerender(
      <SettingsPasswordModal
        open={false}
        verify={vi.fn(async () => false)}
        onCancel={onCancel}
        onUnlock={vi.fn()}
      />,
    );
    expect(onCancel).toHaveBeenCalledOnce();
    expect(trigger).toHaveFocus();
    trigger.remove();
  });
});

describe("NotificationSettingsPage", () => {
  it("renders a masked webhook and saves station, destination, and browsed shared folder", async () => {
    const saveSettings = vi.fn(async () => null);
    const chooseSharedFolder = vi.fn(async () => "C:\\Users\\svc-pdu\\OneDrive\\.pdu-notifications");
    renderSettingsPage({ saveSettings, chooseSharedFolder });

    expect(await screen.findByDisplayValue("PDU Testing")).toBeInTheDocument();
    const webhookInput = screen.getByLabelText("Teams webhook URL");
    expect(webhookInput).toHaveAttribute("type", "password");
    expect(webhookInput).toHaveValue("");
    expect(webhookInput).toHaveAttribute(
      "placeholder",
      "Saved webhook configured; enter a URL only to replace it",
    );
    expect(document.body.textContent).not.toContain("sig=secret");

    fireEvent.change(screen.getByLabelText("Station"), {
      target: { value: "test-station-4" },
    });
    fireEvent.change(screen.getByLabelText("Destination name"), {
      target: { value: "PDU Floor Alerts" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Browse" }));
    expect(await screen.findByDisplayValue("C:\\Users\\svc-pdu\\OneDrive\\.pdu-notifications")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Save settings" }));

    await waitFor(() => {
      expect(saveSettings).toHaveBeenCalledWith(
        expect.objectContaining({
          station_id: "test-station-4",
          station_name: "Test Station 4",
          teams_destination_name: "PDU Floor Alerts",
          shared_shift_log_path: "C:\\Users\\svc-pdu\\OneDrive\\.pdu-notifications",
        }),
      );
    });
    expect(chooseSharedFolder).toHaveBeenCalledOnce();
    expect(await screen.findByText("Notification settings saved.")).toBeInTheDocument();
  });

  it("clears the shared folder selection", async () => {
    renderSettingsPage({
      loadSettings: vi.fn(async () => ({
        ...settingsFixture(),
        shared_shift_log_path: "C:\\shared\\pdu",
      })),
    });

    expect(await screen.findByDisplayValue("C:\\shared\\pdu")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Clear shared folder" }));
    expect(screen.getByLabelText("Shared OneDrive folder")).toHaveValue("");
  });

  it("warns before Back discards dirty settings", async () => {
    const onBack = vi.fn();
    const confirm = vi.spyOn(window, "confirm").mockReturnValueOnce(false).mockReturnValueOnce(true);
    renderSettingsPage({ onBack });

    const destination = await screen.findByLabelText("Destination name");
    fireEvent.change(destination, { target: { value: "Changed destination" } });
    fireEvent.click(screen.getByRole("button", { name: "Back to operator panel" }));
    expect(onBack).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Back to operator panel" }));
    expect(confirm).toHaveBeenCalledTimes(2);
    expect(onBack).toHaveBeenCalledOnce();
  });

  it("freezes form edits and navigation while settings are being saved", async () => {
    const save = deferred<AppNotificationSettingsView | null>();
    renderSettingsPage({ saveSettings: vi.fn(() => save.promise) });

    const destination = await screen.findByLabelText("Destination name");
    fireEvent.change(destination, { target: { value: "Changed destination" } });
    fireEvent.click(screen.getByRole("button", { name: "Save settings" }));

    expect(destination).toBeDisabled();
    expect(screen.getByRole("button", { name: "Back to operator panel" })).toBeDisabled();
    save.resolve(null);
    expect(await screen.findByText("Notification settings saved.")).toBeInTheDocument();
    expect(destination).toBeEnabled();
  });

  it("validates and submits current, new, and confirmation passwords", async () => {
    const changePassword = vi.fn(async () => undefined);
    renderSettingsPage({ changePassword });

    await screen.findByDisplayValue("PDU Testing");
    fireEvent.change(screen.getByLabelText("Current password"), { target: { value: "0601" } });
    fireEvent.change(screen.getByLabelText("New password"), { target: { value: "2468" } });
    fireEvent.change(screen.getByLabelText("Confirm password"), { target: { value: "1357" } });
    fireEvent.click(screen.getByRole("button", { name: "Update password" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "New password and confirmation do not match.",
    );
    expect(changePassword).not.toHaveBeenCalled();

    fireEvent.change(screen.getByLabelText("Confirm password"), { target: { value: "2468" } });
    fireEvent.click(screen.getByRole("button", { name: "Update password" }));

    await waitFor(() => expect(changePassword).toHaveBeenCalledWith("0601", "2468", "2468"));
    expect(await screen.findByText("Settings password updated.")).toBeInTheDocument();
  });

  it("sends a test ping and displays the fresh worker result", async () => {
    const sendTestPing = vi.fn(async () => undefined);
    const getNotificationStatus = vi
      .fn<() => Promise<NotificationRuntimeStatus | null>>()
      .mockResolvedValueOnce(runtimeStatus("ready", "Ready.", "2026-07-10T08:00:00-05:00"))
      .mockResolvedValueOnce(
        runtimeStatus(
          "sent",
          "Complete card accepted by the Workflow.",
          "2026-07-10T08:00:01-05:00",
          "complete",
        ),
      )
      .mockResolvedValueOnce(
        runtimeStatus(
          "sent",
          "Test card accepted by the Workflow.",
          "2026-07-10T08:00:02-05:00",
        ),
      );
    renderSettingsPage({ sendTestPing, getNotificationStatus });

    await screen.findByDisplayValue("PDU Testing");
    fireEvent.click(screen.getByRole("button", { name: "Send test ping" }));

    await waitFor(() => expect(sendTestPing).toHaveBeenCalledOnce());
    expect(await screen.findByText("Test card accepted by the Workflow.")).toBeInTheDocument();
  });
});
