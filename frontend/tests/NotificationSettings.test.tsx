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
    station_id: "test-station-3",
    station_name: "Test Station 3",
    idle_timeout_minutes: 30,
    events: {
      problem: true,
      complete: true,
      stuck: false,
      summary: true,
    },
    shared_shift_log_path: "",
    shifts: [],
    summary_poster_station_id: "pdu-lab",
    summary_included_station_ids: [
      "test-station-1",
      "test-station-3",
      "test-station-4",
      "pdu-lab",
    ],
    is_summary_poster: false,
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
    station_name: "Test Station 3",
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

function renderSettingsPage(
  overrides: Partial<React.ComponentProps<typeof NotificationSettingsPage>> = {},
) {
  const props: React.ComponentProps<typeof NotificationSettingsPage> = {
    onBack: vi.fn(),
    loadSettings: vi.fn(async () => settingsFixture()),
    saveSettings: vi.fn(async () => null),
    changePassword: vi.fn(async () => undefined),
    sendTestPing: vi.fn(async () => undefined),
    getNotificationStatus: vi.fn(async () => null),
    chooseSharedFolder: vi.fn(async () => null),
    previewShiftSummary: vi.fn(async () => null),
    postShiftSummary: vi.fn(async () => null),
    verifyPassword: vi.fn(async (password) => password === "0601"),
    ...overrides,
  };

  render(<NotificationSettingsPage {...props} />);
  return props;
}

async function unlockAdvancedStationTeams() {
  fireEvent.click(await screen.findByRole("button", { name: /^Advanced$/i }));
  fireEvent.change(await screen.findByLabelText("Password"), { target: { value: "0601" } });
  fireEvent.click(screen.getByRole("button", { name: "Unlock" }));
  fireEvent.click(await screen.findByRole("button", { name: /Station & Teams/i }));
  await screen.findByLabelText("This PC station");
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe("SettingsPasswordModal", () => {
  it("keeps the gate closed and shows an inline error for a wrong password", async () => {
    const verify = vi.fn(async () => false);
    const onUnlock = vi.fn();

    render(
      <SettingsPasswordModal open verify={verify} onCancel={vi.fn()} onUnlock={onUnlock} />,
    );

    fireEvent.change(screen.getByLabelText("Password"), { target: { value: "wrong" } });
    fireEvent.click(screen.getByRole("button", { name: "Unlock" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("Incorrect password");
    expect(onUnlock).not.toHaveBeenCalled();
  });
});

describe("NotificationSettingsPage", () => {
  it("opens operator pages without password and advanced with password", async () => {
    renderSettingsPage();

    expect(await screen.findByRole("heading", { name: "Settings" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^Shifts$/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Summary options/i })).toBeInTheDocument();
    expect(screen.getByRole("region", { name: /End of shift/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Post Summary/i })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Station & Teams/i })).not.toBeInTheDocument();

    fireEvent.click(await screen.findByRole("button", { name: /^Advanced$/i }));
    expect(await screen.findByLabelText("Password")).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Password"), { target: { value: "0601" } });
    fireEvent.click(screen.getByRole("button", { name: "Unlock" }));
    expect(await screen.findByRole("button", { name: /Station & Teams/i })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /^End of shift$/i })).not.toBeInTheDocument();
    expect(screen.getByRole("heading", { level: 1, name: "Advanced" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /Station & Teams/i }));
    expect(await screen.findByRole("heading", { level: 1, name: "Station & Teams" })).toBeInTheDocument();
    expect(await screen.findByLabelText("Teams webhook URL")).toBeInTheDocument();
  });

  it("saves station as PDU Lab from advanced settings", async () => {
    const saveSettings = vi.fn(async () => null);
    renderSettingsPage({ saveSettings });
    await unlockAdvancedStationTeams();

    fireEvent.change(screen.getByLabelText("This PC station"), {
      target: { value: "pdu-lab" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => {
      expect(saveSettings).toHaveBeenCalledWith(
        expect.objectContaining({
          station_id: "pdu-lab",
          station_name: "PDU Lab",
        }),
      );
    });
  });

  it("lets operators set main poster and included stations without password", async () => {
    const saveSettings = vi.fn(async () => null);
    renderSettingsPage({ saveSettings });

    fireEvent.click(await screen.findByRole("button", { name: /Summary options/i }));
    expect(screen.queryByLabelText("Station that posts end of shift")).not.toBeInTheDocument();
    expect(screen.getByRole("switch", { name: "Enable summary notifications" })).toHaveAttribute(
      "aria-checked",
      "true",
    );

    fireEvent.click(screen.getByRole("radio", { name: "Test Station 1 main poster" }));
    fireEvent.click(screen.getByLabelText("Include PDU Lab"));
    fireEvent.click(screen.getByRole("button", { name: /^Save$/i }));

    await waitFor(() => {
      expect(saveSettings).toHaveBeenCalledWith(
        expect.objectContaining({
          summary_poster_station_id: "test-station-1",
          summary_included_station_ids: expect.not.arrayContaining(["pdu-lab"]),
        }),
      );
    });
  });

  it("toggles summary notifications from the save row", async () => {
    const saveSettings = vi.fn(async () => null);
    renderSettingsPage({ saveSettings });

    fireEvent.click(await screen.findByRole("button", { name: /Summary options/i }));
    const toggle = screen.getByRole("switch", { name: "Enable summary notifications" });
    expect(toggle).toHaveAttribute("aria-checked", "true");
    fireEvent.click(toggle);
    expect(toggle).toHaveAttribute("aria-checked", "false");
    fireEvent.click(screen.getByRole("button", { name: /^Save$/i }));

    await waitFor(() => {
      expect(saveSettings).toHaveBeenCalledWith(
        expect.objectContaining({
          events: expect.objectContaining({ summary: false }),
        }),
      );
    });
  });

  it("picks shift times with separate five-minute hour and minute wheels", async () => {
    const saveSettings = vi.fn(async () => null);
    renderSettingsPage({ saveSettings });

    fireEvent.click(await screen.findByRole("button", { name: /^Shifts$/i }));
    expect(await screen.findByRole("heading", { level: 1, name: "Shifts" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Double shift" }));
    const minuteWheels = screen.getAllByRole("group", { name: "Minute wheel" });
    fireEvent.wheel(minuteWheels[0], { deltaY: 100 });
    expect(screen.getByRole("button", { name: "Edit Start time, 6:05 AM" })).toBeInTheDocument();
    fireEvent.click(screen.getAllByRole("button", { name: "Previous minute" })[0]);
    expect(screen.getByRole("button", { name: "Edit Start time, 6:00 AM" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /^Save$/i }));

    await waitFor(() => {
      expect(saveSettings).toHaveBeenCalledWith(
        expect.objectContaining({
          shifts: [
            expect.objectContaining({ start_time: "06:00", end_time: "15:00" }),
            expect.objectContaining({ start_time: "15:00", end_time: "23:00" }),
          ],
        }),
      );
    });
  });

  it("has change password under advanced after unlock", async () => {
    renderSettingsPage();
    fireEvent.click(await screen.findByRole("button", { name: /^Advanced$/i }));
    fireEvent.change(await screen.findByLabelText("Password"), { target: { value: "0601" } });
    fireEvent.click(screen.getByRole("button", { name: "Unlock" }));
    fireEvent.click(await screen.findByRole("button", { name: /Change password/i }));
    expect(await screen.findByRole("heading", { level: 1, name: "Change password" })).toBeInTheDocument();
    expect(await screen.findByLabelText("Current password")).toBeInTheDocument();
  });

  it("sends a test ping from advanced station page", async () => {
    const sendTestPing = vi.fn(async () => undefined);
    const getNotificationStatus = vi
      .fn<() => Promise<NotificationRuntimeStatus | null>>()
      .mockResolvedValueOnce(runtimeStatus("ready", "Ready.", "t0"))
      .mockResolvedValueOnce(runtimeStatus("sent", "Test card accepted by the Workflow.", "t1"));
    renderSettingsPage({ sendTestPing, getNotificationStatus });
    await unlockAdvancedStationTeams();
    fireEvent.click(screen.getByRole("button", { name: "Send test ping" }));
    await waitFor(() => expect(sendTestPing).toHaveBeenCalledOnce());
    expect(await screen.findByText("Test card accepted by the Workflow.")).toBeInTheDocument();
  });

  it("freezes save navigation while saving", async () => {
    const save = deferred<AppNotificationSettingsView | null>();
    renderSettingsPage({ saveSettings: vi.fn(() => save.promise) });
    fireEvent.click(await screen.findByRole("button", { name: /^Shifts$/i }));
    fireEvent.click(screen.getByRole("button", { name: "Double shift" }));
    fireEvent.click(screen.getByRole("button", { name: /^Save$/i }));
    expect(screen.getByRole("button", { name: "Back to settings menu" })).toBeDisabled();
    save.resolve(null);
    expect(await screen.findByText("Settings saved.")).toBeInTheDocument();
  });

  it("prompts to save, discard, or stay when leaving a submenu with unsaved settings", async () => {
    const onBack = vi.fn();
    const saveSettings = vi.fn(async () => null);
    renderSettingsPage({ onBack, saveSettings });

    fireEvent.click(await screen.findByRole("button", { name: /^Shifts$/i }));
    fireEvent.click(screen.getByRole("button", { name: "Double shift" }));
    fireEvent.click(screen.getByRole("button", { name: "Back to settings menu" }));

    expect(await screen.findByRole("dialog", { name: "Unsaved settings" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { level: 1, name: "Shifts" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("dialog", { name: "Unsaved settings" })).not.toBeInTheDocument();
    expect(screen.getByRole("heading", { level: 1, name: "Shifts" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Back to settings menu" }));
    fireEvent.click(await screen.findByRole("button", { name: "Save Changes" }));
    await waitFor(() => expect(saveSettings).toHaveBeenCalled());
    expect(await screen.findByRole("heading", { level: 1, name: "Settings" })).toBeInTheDocument();
    expect(onBack).not.toHaveBeenCalled();
  });

  it("discards unsaved submenu changes when chosen", async () => {
    renderSettingsPage();

    fireEvent.click(await screen.findByRole("button", { name: /^Shifts$/i }));
    fireEvent.click(screen.getByRole("button", { name: "Double shift" }));
    fireEvent.click(screen.getByRole("button", { name: "Back to settings menu" }));
    fireEvent.click(await screen.findByRole("button", { name: "Discard Changes" }));

    expect(await screen.findByRole("heading", { level: 1, name: "Settings" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /^Shifts$/i }));
    // Discarded double-shift seed falls back to single after empty re-open seed, or single if defaults restored
    expect(screen.getByRole("button", { name: "Single shift" })).toBeInTheDocument();
  });

  it("lets any station post end of shift from the settings home after confirm", async () => {
    const postShiftSummary = vi.fn(async () => ({
      message: "End-of-shift summary posted by Test Station 3. Other stations will see it was already sent.",
      text: "📊 posted",
    }));
    const previewShiftSummary = vi.fn(async () => ({
      text: "📊 preview",
      is_summary_poster: false,
      poster_station_id: "pdu-lab",
      poster_station_name: "PDU Lab",
      event_count: 2,
      shared_folder_configured: true,
      already_posted: false,
    }));
    const confirmSpy = vi.spyOn(window, "confirm").mockReturnValue(true);
    renderSettingsPage({
      postShiftSummary,
      previewShiftSummary,
      loadSettings: vi.fn(async () => ({
        ...settingsFixture(),
        shared_shift_log_path: "C:\\Shared\\PDU",
        is_summary_poster: false,
      })),
    });

    expect(await screen.findByRole("region", { name: /End of shift/i })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /Post Summary/i }));

    expect(confirmSpy).toHaveBeenCalled();
    await waitFor(() => expect(postShiftSummary).toHaveBeenCalled());
    expect(await screen.findByText(/Other stations will see/i)).toBeInTheDocument();
  });
});
