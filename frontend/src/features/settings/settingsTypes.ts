export type NotificationEventToggles = {
  problem: boolean;
  complete: boolean;
  stuck: boolean;
  summary: boolean;
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
};

export type SaveNotificationSettingsRequest = Omit<
  AppNotificationSettingsView,
  "webhook_configured"
>;

export type NotificationRuntimeStatus = {
  state: "idle" | "ready" | "sent" | "skipped" | "failed";
  message: string;
  station_name: string | null;
  destination_name: string | null;
  updated_at: string | null;
  event_kind?: "test_ping" | "problem" | "complete" | "stuck" | "summary" | null;
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

export const NOTIFICATION_STATIONS = [
  { id: "test-station-1", name: "Test Station 1" },
  { id: "test-station-2", name: "Test Station 2" },
  { id: "test-station-3", name: "Test Station 3" },
  { id: "test-station-4", name: "Test Station 4" },
] as const;

export type NotificationStationId = (typeof NOTIFICATION_STATIONS)[number]["id"];

export function stationNameForId(stationId: string) {
  return (
    NOTIFICATION_STATIONS.find((station) => station.id === stationId)?.name ??
    "Test Station 1"
  );
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
      summary: false,
    },
    shared_shift_log_path: "",
  };
}
