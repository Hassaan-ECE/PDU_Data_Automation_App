export type NotificationEventToggles = {
  problem: boolean;
  complete: boolean;
  changeover: boolean;
  stuck: boolean;
  summary: boolean;
};

export type ShiftWindow = {
  label: string;
  start_time: string;
  end_time: string;
};

export type StationRole = "floor" | "admin";

export type StationCatalogEntry = {
  id: string;
  name: string;
  role: StationRole;
};

export type CatalogCreateRequest = {
  name: string;
  role: StationRole;
  select_for_this_pc: boolean;
};

export type FloorSyncStatus = {
  configured: boolean;
  source: string;
  updated_at?: string | null;
  updated_by_station_id?: string | null;
  message: string;
};

export type AppNotificationSettingsView = {
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
};

/** Which section a save may mutate (matches backend SettingsSaveScope snake_case). */
export type SettingsSaveScope =
  | "operator"
  | "identity"
  | "teams"
  | "advanced"
  | "connect"
  | "local";

export type SaveNotificationSettingsRequest = Omit<
  AppNotificationSettingsView,
  "webhook_configured" | "is_summary_poster" | "floor_sync"
> & {
  scope: SettingsSaveScope;
  /** Required when Connect targets an existing floor file. */
  connect_password?: string;
  /** Creates at most one backend-id identity while holding the floor lock. */
  catalog_create?: CatalogCreateRequest;
};

export type NotificationRuntimeStatus = {
  state: "idle" | "ready" | "sent" | "skipped" | "failed";
  message: string;
  station_name: string | null;
  destination_name: string | null;
  updated_at: string | null;
  event_kind?: "test_ping" | "problem" | "complete" | "changeover" | "stuck" | "summary" | null;
};

export type ShiftSummaryPreview = {
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
};

export type ShiftSummaryResult = {
  message: string;
  text: string;
};

export type LoadNotificationSettings = () => Promise<AppNotificationSettingsView | null>;
export type SaveNotificationSettings = (
  request: SaveNotificationSettingsRequest,
) => Promise<AppNotificationSettingsView | null>;
export type ChangeSettingsPassword = (
  currentPassword: string,
  newPassword: string,
  confirmPassword: string,
) => Promise<void>;
export type SendNotificationTest = () => Promise<void>;
export type GetNotificationStatus = () => Promise<NotificationRuntimeStatus | null>;
export type VerifySettingsPassword = (password: string) => Promise<boolean>;
export type ChooseSharedNotificationsFolder = () => Promise<string | null>;
export type PreviewShiftSummary = (shiftLabel?: string) => Promise<ShiftSummaryPreview | null>;
export type PostShiftSummary = (shiftLabel: string) => Promise<ShiftSummaryResult | null>;

/** Fallback catalog when backend has not returned stations yet (tests / offline). */
export const NOTIFICATION_STATIONS = [
  { id: "test-station-1", name: "Test Station 1", role: "floor" },
  { id: "test-station-3", name: "Test Station 3", role: "floor" },
  { id: "test-station-4", name: "Test Station 4", role: "floor" },
  { id: "pdu-lab", name: "PDU Lab", role: "floor" },
] as const;

export type NotificationStationId = (typeof NOTIFICATION_STATIONS)[number]["id"];

export function stationNameForId(
  stationId: string,
  catalog?: StationCatalogEntry[] | null,
) {
  const fromCatalog = catalog?.find((station) => station.id === stationId)?.name;
  if (fromCatalog) return fromCatalog;
  return (
    NOTIFICATION_STATIONS.find((station) => station.id === stationId)?.name ??
    "Test Station 1"
  );
}

export function defaultIncludedStationIds() {
  return NOTIFICATION_STATIONS.map((station) => station.id);
}

export function defaultFloorSyncLocal(): FloorSyncStatus {
  return {
    configured: false,
    source: "local",
    updated_at: null,
    updated_by_station_id: null,
    message: "Shared folder not set — settings stay on this PC only.",
  };
}

export function createDefaultNotificationSettings(): AppNotificationSettingsView {
  const stations = NOTIFICATION_STATIONS.map((s) => ({ ...s }));
  return {
    enabled: true,
    teams_destination_name: "PDU Testing",
    teams_webhook_url: "",
    webhook_configured: false,
    station_id: "test-station-1",
    station_name: "Test Station 1",
    idle_timeout_minutes: 30,
    events: {
      problem: true,
      complete: true,
      changeover: true,
      stuck: false,
      summary: true,
    },
    shared_shift_log_path: "",
    shifts: [],
    summary_poster_station_id: "pdu-lab",
    summary_included_station_ids: defaultIncludedStationIds(),
    is_summary_poster: false,
    stations,
    floor_sync: defaultFloorSyncLocal(),
  };
}

/**
 * While Settings is open, peer floor reloads apply only when the form is clean.
 * Exported for unit tests.
 */
export function shouldApplySettingsReload(isDirty: boolean): boolean {
  return !isDirty;
}

/**
 * Decide which backend scope to use for the current save.
 * view is the Settings page view id
 * (home, shifts, summaryOptions, advanced, identities, teams, password).
 */
export function resolveSaveScope(
  view: string,
  current: AppNotificationSettingsView,
  lastSaved: AppNotificationSettingsView | null,
  unlockPassword: string,
  /** Dedicated existing-floor password for Connect (preferred over Advanced unlock). */
  floorConnectPassword = "",
): { scope: SettingsSaveScope; connect_password?: string } {
  if (view === "shifts" || view === "summaryOptions") {
    return { scope: "operator" };
  }

  const savedPath = (lastSaved?.shared_shift_log_path ?? "").trim();
  const nextPath = current.shared_shift_log_path.trim();

  if (nextPath === "" && savedPath !== "") {
    return { scope: "local" };
  }
  if (nextPath !== "" && nextPath !== savedPath) {
    // Prefer explicit floor password so a new PC can connect when floor ≠ local Advanced password.
    const password = (floorConnectPassword.trim() || unlockPassword).trim();
    return {
      scope: "connect",
      connect_password: password,
    };
  }

  if (view === "identities") {
    return { scope: "identity" };
  }
  if (view === "teams") {
    return { scope: "teams" };
  }
  if (view === "advanced" || view === "password" || view === "home") {
    return { scope: "advanced" };
  }

  return { scope: "operator" };
}

/** True when Station & Identities Save will Connect (new/changed shared path). */
export function isPendingSharedFolderConnect(
  current: AppNotificationSettingsView | null,
  lastSaved: AppNotificationSettingsView | null,
): boolean {
  if (!current) return false;
  const nextPath = current.shared_shift_log_path.trim();
  const savedPath = (lastSaved?.shared_shift_log_path ?? "").trim();
  return nextPath !== "" && nextPath !== savedPath;
}
