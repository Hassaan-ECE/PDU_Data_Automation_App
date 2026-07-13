import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { NotificationSettingsPage } from "@/features/settings/NotificationSettingsPage";
import { SettingsPasswordModal } from "@/features/settings/SettingsPasswordModal";
import {
  resolveSaveScope,
  shouldApplySettingsReload,
  type AppNotificationSettingsView,
  type NotificationRuntimeStatus,
} from "@/features/settings/settingsTypes";

function settingsFixture(
  overrides: Partial<AppNotificationSettingsView> = {},
): AppNotificationSettingsView {
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
    stations: [
      { id: "test-station-1", name: "Test Station 1" },
      { id: "test-station-3", name: "Test Station 3" },
      { id: "test-station-4", name: "Test Station 4" },
      { id: "pdu-lab", name: "PDU Lab" },
    ],
    floor_sync: {
      configured: false,
      source: "local",
      updated_at: null,
      updated_by_station_id: null,
      message: "Shared folder not set — settings stay on this PC only.",
    },
    ...overrides,
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
  vi.useRealTimers();
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

  it("passes the unlock password to onUnlock when verified", async () => {
    const verify = vi.fn(async () => true);
    const onUnlock = vi.fn();

    render(
      <SettingsPasswordModal open verify={verify} onCancel={vi.fn()} onUnlock={onUnlock} />,
    );

    fireEvent.change(screen.getByLabelText("Password"), { target: { value: "0601" } });
    fireEvent.click(screen.getByRole("button", { name: "Unlock" }));

    await waitFor(() => expect(onUnlock).toHaveBeenCalledWith("0601"));
  });
});

describe("resolveSaveScope / shouldApplySettingsReload", () => {
  it("maps operator pages to operator scope and path changes to connect/local", () => {
    const base = settingsFixture();
    const withPath = settingsFixture({
      shared_shift_log_path: "C:\\Users\\a\\OneDrive\\.PDU_Notifications",
    });
    const cleared = settingsFixture({ shared_shift_log_path: "" });

    expect(resolveSaveScope("shifts", base, base, "0601").scope).toBe("operator");
    expect(resolveSaveScope("summaryOptions", base, base, "0601").scope).toBe("operator");
    expect(resolveSaveScope("station", base, base, "0601").scope).toBe("advanced");
    expect(resolveSaveScope("station", withPath, base, "0601")).toEqual({
      scope: "connect",
      connect_password: "0601",
    });
    expect(resolveSaveScope("station", withPath, base, "0601", "4242")).toEqual({
      scope: "connect",
      connect_password: "4242",
    });
    expect(resolveSaveScope("station", cleared, withPath, "0601").scope).toBe("local");
  });

  it("blocks peer reload apply while the form is dirty", () => {
    expect(shouldApplySettingsReload(false)).toBe(true);
    expect(shouldApplySettingsReload(true)).toBe(false);
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

  it("shows editable station display names with stable ids in Advanced", async () => {
    renderSettingsPage();
    await unlockAdvancedStationTeams();

    expect(screen.getByText("Station display names")).toBeInTheDocument();
    expect(screen.getByText("test-station-1")).toBeInTheDocument();
    expect(screen.getByText("test-station-3")).toBeInTheDocument();
    expect(screen.getByText("test-station-4")).toBeInTheDocument();
    expect(screen.getByText("pdu-lab")).toBeInTheDocument();

    const nameInputs = screen.getAllByDisplayValue(/Test Station|PDU Lab/);
    expect(nameInputs.length).toBeGreaterThanOrEqual(4);
  });

  it("saves Advanced renames with scope advanced", async () => {
    const saveSettings = vi.fn(async () => null);
    renderSettingsPage({ saveSettings });
    await unlockAdvancedStationTeams();

    const station3Input = screen
      .getAllByDisplayValue("Test Station 3")
      .find((el) => el.tagName === "INPUT");
    expect(station3Input).toBeTruthy();
    fireEvent.change(station3Input!, { target: { value: "Test Station 2" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => {
      expect(saveSettings).toHaveBeenCalledWith(
        expect.objectContaining({
          scope: "advanced",
          stations: expect.arrayContaining([
            expect.objectContaining({ id: "test-station-3", name: "Test Station 2" }),
          ]),
        }),
      );
    });
  });

  it("saves station as PDU Lab from advanced settings with advanced scope", async () => {
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
          scope: "advanced",
          station_id: "pdu-lab",
          station_name: "PDU Lab",
        }),
      );
    });
  });

  it("uses renamed station names on operator summary labels", async () => {
    renderSettingsPage({
      loadSettings: vi.fn(async () =>
        settingsFixture({
          stations: [
            { id: "test-station-1", name: "Bay A" },
            { id: "test-station-3", name: "Bay B" },
            { id: "test-station-4", name: "Bay C" },
            { id: "pdu-lab", name: "Main Desk" },
          ],
        }),
      ),
    });

    fireEvent.click(await screen.findByRole("button", { name: /Summary options/i }));
    expect(screen.getByRole("radio", { name: "Bay A main poster" })).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: "Main Desk main poster" })).toBeInTheDocument();
    expect(screen.getByLabelText("Include Bay B")).toBeInTheDocument();
    expect(screen.getByLabelText("Include Main Desk")).toBeInTheDocument();
  });

  it("lets operators set main poster and included stations with operator scope", async () => {
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
          scope: "operator",
          summary_poster_station_id: "test-station-1",
          summary_included_station_ids: expect.not.arrayContaining(["pdu-lab"]),
        }),
      );
    });
  });

  it("saves shifts with operator scope", async () => {
    const saveSettings = vi.fn(async () => null);
    renderSettingsPage({ saveSettings });

    fireEvent.click(await screen.findByRole("button", { name: /^Shifts$/i }));
    fireEvent.click(screen.getByRole("button", { name: "Double shift" }));
    fireEvent.click(screen.getByRole("button", { name: /^Save$/i }));

    await waitFor(() => {
      expect(saveSettings).toHaveBeenCalledWith(
        expect.objectContaining({
          scope: "operator",
          shifts: expect.any(Array),
        }),
      );
    });
  });

  it("uses connect scope and unlock password when browsing a new shared folder", async () => {
    const saveSettings = vi.fn(async () => null);
    const chooseSharedFolder = vi.fn(async () => "C:\\Shared\\.PDU_Notifications");
    renderSettingsPage({ saveSettings, chooseSharedFolder });
    await unlockAdvancedStationTeams();

    fireEvent.click(screen.getByRole("button", { name: /Browse/i }));
    await waitFor(() => expect(chooseSharedFolder).toHaveBeenCalled());
    expect(screen.getByLabelText("Existing floor password")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => {
      expect(saveSettings).toHaveBeenCalledWith(
        expect.objectContaining({
          scope: "connect",
          connect_password: "0601",
          shared_shift_log_path: "C:\\Shared\\.PDU_Notifications",
        }),
      );
    });
  });

  it("sends dedicated floor password on Connect when entered", async () => {
    const saveSettings = vi.fn(async () => null);
    const chooseSharedFolder = vi.fn(async () => "C:\\Shared\\.PDU_Notifications");
    renderSettingsPage({ saveSettings, chooseSharedFolder });
    await unlockAdvancedStationTeams();

    fireEvent.click(screen.getByRole("button", { name: /Browse/i }));
    await waitFor(() => expect(chooseSharedFolder).toHaveBeenCalled());
    fireEvent.change(screen.getByLabelText("Existing floor password"), {
      target: { value: "4242" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => {
      expect(saveSettings).toHaveBeenCalledWith(
        expect.objectContaining({
          scope: "connect",
          connect_password: "4242",
        }),
      );
    });
  });

  it("uses local scope when clearing the shared folder path", async () => {
    const saveSettings = vi.fn(async () => null);
    renderSettingsPage({
      saveSettings,
      loadSettings: vi.fn(async () =>
        settingsFixture({
          shared_shift_log_path: "C:\\Shared\\.PDU_Notifications",
          floor_sync: {
            configured: true,
            source: "floor",
            updated_at: "unix:1",
            updated_by_station_id: "pdu-lab",
            message: "Syncing via shared folder.",
          },
        }),
      ),
    });
    await unlockAdvancedStationTeams();

    fireEvent.click(screen.getByRole("button", { name: "Clear shared folder" }));
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => {
      expect(saveSettings).toHaveBeenCalledWith(
        expect.objectContaining({
          scope: "local",
          shared_shift_log_path: "",
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
          scope: "operator",
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
          scope: "operator",
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

  it("reloads settings on a clean form poll and does not clobber when dirty", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    let loadCount = 0;
    const loadSettings = vi.fn(async () => {
      loadCount += 1;
      if (loadCount === 1) {
        return settingsFixture({
          stations: [
            { id: "test-station-1", name: "Test Station 1" },
            { id: "test-station-3", name: "Test Station 3" },
            { id: "test-station-4", name: "Test Station 4" },
            { id: "pdu-lab", name: "PDU Lab" },
          ],
          floor_sync: {
            configured: true,
            source: "floor",
            updated_at: "unix:1",
            updated_by_station_id: "pdu-lab",
            message: "Syncing via shared folder.",
          },
        });
      }
      return settingsFixture({
        stations: [
          { id: "test-station-1", name: "Renamed From Peer" },
          { id: "test-station-3", name: "Test Station 3" },
          { id: "test-station-4", name: "Test Station 4" },
          { id: "pdu-lab", name: "PDU Lab" },
        ],
        floor_sync: {
          configured: true,
          source: "floor",
          updated_at: "unix:2",
          updated_by_station_id: "test-station-1",
          message: "Syncing via shared folder.",
        },
      });
    });

    renderSettingsPage({ loadSettings });
    expect(await screen.findByRole("heading", { name: "Settings" })).toBeInTheDocument();
    expect(loadSettings).toHaveBeenCalledTimes(1);

    // Dirty form: poll must not apply peer reload.
    fireEvent.click(screen.getByRole("button", { name: /Summary options/i }));
    fireEvent.click(screen.getByRole("radio", { name: "Test Station 1 main poster" }));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(45_000);
    });
    // Second load may be attempted, but UI still shows dirty Main selection
    expect(screen.getByRole("radio", { name: "Test Station 1 main poster" })).toHaveAttribute(
      "aria-checked",
      "true",
    );

    // Discard so form is clean, then poll can apply peer names.
    fireEvent.click(screen.getByRole("button", { name: "Back to settings menu" }));
    fireEvent.click(await screen.findByRole("button", { name: "Discard Changes" }));
    expect(await screen.findByRole("heading", { level: 1, name: "Settings" })).toBeInTheDocument();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(45_000);
    });

    await waitFor(() => expect(loadSettings.mock.calls.length).toBeGreaterThanOrEqual(2));

    fireEvent.click(screen.getByRole("button", { name: /Summary options/i }));
    expect(await screen.findByRole("radio", { name: "Renamed From Peer main poster" })).toBeInTheDocument();
  });
});
