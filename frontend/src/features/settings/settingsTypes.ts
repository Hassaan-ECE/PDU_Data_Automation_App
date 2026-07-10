export type NotificationEventToggles = {
  problem: boolean;
  complete: boolean;
  stuck: boolean;
  summary: boolean;
};

export type ShiftWindow = {
  label: string;
  start_time: string;
  end_time: string;
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
};

export type SaveNotificationSettingsRequest = Omit<
  AppNotificationSettingsView,
  "webhook_configured" | "is_summary_poster"
>;

export type NotificationRuntimeStatus = {
  state: "idle" | "ready" | "sent" | "skipped" | "failed";
  message: string;
  station_name: string | null;
  destination_name: string | null;
  updated_at: string | null;
  event_kind?: "test_ping" | "problem" | "complete" | "stuck" | "summary" | null;
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

/** Keep in sync with backend `notifications::stations::KNOWN_STATIONS`. */
export const NOTIFICATION_STATIONS = [
  { id: "test-station-1", name: "Test Station 1" },
  { id: "test-station-3", name: "Test Station 3" },
  { id: "test-station-4", name: "Test Station 4" },
  { id: "pdu-lab", name: "PDU Lab" },
] as const;

export type NotificationStationId = (typeof NOTIFICATION_STATIONS)[number]["id"];

export function stationNameForId(stationId: string) {
  return (
    NOTIFICATION_STATIONS.find((station) => station.id === stationId)?.name ??
    "Test Station 1"
  );
}

export function defaultIncludedStationIds() {
  return NOTIFICATION_STATIONS.map((station) => station.id);
}

export function createDefaultNotificationSettings(): AppNotificationSettingsView {
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
      stuck: false,
      summary: true,
    },
    shared_shift_log_path: "",
    shifts: [],
    summary_poster_station_id: "pdu-lab",
    summary_included_station_ids: defaultIncludedStationIds(),
    is_summary_poster: false,
  };
}
