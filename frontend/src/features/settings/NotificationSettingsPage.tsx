import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type FormEvent,
} from "react";
import { ArrowLeft, FolderOpen, KeyRound, LoaderCircle, Save, Send, X } from "lucide-react";

import { cn } from "@/shared/lib/utils";

import {
  NOTIFICATION_STATIONS,
  createDefaultNotificationSettings,
  stationNameForId,
  type AppNotificationSettingsView,
  type ChangeSettingsPassword,
  type ChooseSharedNotificationsFolder,
  type GetNotificationStatus,
  type LoadNotificationSettings,
  type NotificationRuntimeStatus,
  type SaveNotificationSettings,
  type SaveNotificationSettingsRequest,
  type SendNotificationTest,
} from "./settingsTypes";

const inputClassName =
  "mt-1 h-9 w-full rounded border border-[#454542] bg-[#1f1f1e] px-2 text-[8.5pt] text-white placeholder:text-[#777772] outline-none focus:border-[#1f74ae] focus:ring-2 focus:ring-cyan-200/25 disabled:cursor-not-allowed disabled:opacity-60";
const PING_POLL_ATTEMPTS = 45;
const PING_POLL_INTERVAL_MS = 500;

type ResultMessage = {
  tone: "success" | "warning" | "error";
  text: string;
};

type PasswordFields = {
  current: string;
  next: string;
  confirm: string;
};

const emptyPasswordFields: PasswordFields = {
  current: "",
  next: "",
  confirm: "",
};

export interface NotificationSettingsPageProps {
  onBack: () => void;
  loadSettings: LoadNotificationSettings;
  saveSettings: SaveNotificationSettings;
  changePassword: ChangeSettingsPassword;
  sendTestPing: SendNotificationTest;
  getNotificationStatus: GetNotificationStatus;
  chooseSharedFolder: ChooseSharedNotificationsFolder;
}

export function NotificationSettingsPage({
  onBack,
  loadSettings,
  saveSettings,
  changePassword,
  sendTestPing,
  getNotificationStatus,
  chooseSharedFolder,
}: NotificationSettingsPageProps) {
  const pingAbortRef = useRef<AbortController | null>(null);
  const [settings, setSettings] = useState<AppNotificationSettingsView | null>(null);
  const [savedFingerprint, setSavedFingerprint] = useState("");
  const [passwordFields, setPasswordFields] = useState<PasswordFields>(emptyPasswordFields);
  const [isLoading, setIsLoading] = useState(true);
  const [loadError, setLoadError] = useState("");
  const [isSaving, setIsSaving] = useState(false);
  const [saveResult, setSaveResult] = useState<ResultMessage | null>(null);
  const [isChangingPassword, setIsChangingPassword] = useState(false);
  const [passwordResult, setPasswordResult] = useState<ResultMessage | null>(null);
  const [isTesting, setIsTesting] = useState(false);
  const [testResult, setTestResult] = useState<ResultMessage | null>(null);
  const [isBrowsingSharedFolder, setIsBrowsingSharedFolder] = useState(false);
  const pageBusy = isSaving || isChangingPassword || isTesting || isBrowsingSharedFolder;

  useEffect(() => {
    let cancelled = false;

    void loadSettings()
      .then((loadedSettings) => {
        if (cancelled) {
          return;
        }
        const loaded = loadedSettings ?? createDefaultNotificationSettings();
        setSettings(loaded);
        setSavedFingerprint(settingsFingerprint(loaded));
        setSaveResult(null);
      })
      .catch((error) => {
        if (!cancelled) {
          setLoadError(errorMessage(error, "Notification settings could not be loaded."));
        }
      })
      .finally(() => {
        if (!cancelled) {
          setIsLoading(false);
        }
      });

    return () => {
      cancelled = true;
    };
  }, [loadSettings]);

  useEffect(() => {
    return () => pingAbortRef.current?.abort();
  }, []);

  const settingsDirty = useMemo(
    () => Boolean(settings && savedFingerprint && settingsFingerprint(settings) !== savedFingerprint),
    [savedFingerprint, settings],
  );
  const passwordDirty = Boolean(
    passwordFields.current || passwordFields.next || passwordFields.confirm,
  );
  const isDirty = settingsDirty || passwordDirty;

  function updateSettings(update: Partial<AppNotificationSettingsView>) {
    setSettings((current) => (current ? { ...current, ...update } : current));
    setSaveResult(null);
    setTestResult(null);
  }

  function updatePasswordField(field: keyof PasswordFields, value: string) {
    setPasswordFields((current) => ({ ...current, [field]: value }));
    setPasswordResult(null);
  }

  function handleBack() {
    if (pageBusy) {
      return;
    }
    if (isDirty && !window.confirm("Discard unsaved notification settings?")) {
      return;
    }

    onBack();
  }

  async function handleRetryLoad() {
    setIsLoading(true);
    setLoadError("");
    try {
      const loaded = (await loadSettings()) ?? createDefaultNotificationSettings();
      setSettings(loaded);
      setSavedFingerprint(settingsFingerprint(loaded));
      setSaveResult(null);
    } catch (error) {
      setLoadError(errorMessage(error, "Notification settings could not be loaded."));
    } finally {
      setIsLoading(false);
    }
  }

  async function handleSave(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!settings || pageBusy) {
      return;
    }

    const request = saveRequestFromSettings(settings);
    setIsSaving(true);
    setSaveResult(null);
    try {
      const saved = await saveSettings(request);
      const nextSettings = saved ?? settingsAfterSave(settings, request);
      setSettings(nextSettings);
      setSavedFingerprint(settingsFingerprint(nextSettings));
      setSaveResult({ tone: "success", text: "Notification settings saved." });
    } catch (error) {
      setSaveResult({
        tone: "error",
        text: errorMessage(error, "Notification settings could not be saved."),
      });
    } finally {
      setIsSaving(false);
    }
  }

  async function handlePasswordChange(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (pageBusy) {
      return;
    }

    if (!passwordFields.current) {
      setPasswordResult({ tone: "error", text: "Enter the current password." });
      return;
    }
    if (!passwordFields.next.trim()) {
      setPasswordResult({ tone: "error", text: "Enter a new password." });
      return;
    }
    if (passwordFields.next !== passwordFields.confirm) {
      setPasswordResult({ tone: "error", text: "New password and confirmation do not match." });
      return;
    }

    setIsChangingPassword(true);
    setPasswordResult(null);
    try {
      await changePassword(
        passwordFields.current,
        passwordFields.next,
        passwordFields.confirm,
      );
      setPasswordFields(emptyPasswordFields);
      setPasswordResult({ tone: "success", text: "Settings password updated." });
    } catch (error) {
      setPasswordResult({
        tone: "error",
        text: errorMessage(error, "The settings password could not be updated."),
      });
    } finally {
      setIsChangingPassword(false);
    }
  }

  async function handleBrowseSharedFolder() {
    if (pageBusy || !settings) {
      return;
    }
    setIsBrowsingSharedFolder(true);
    setSaveResult(null);
    try {
      const selected = await chooseSharedFolder();
      if (selected) {
        updateSettings({ shared_shift_log_path: selected });
      }
    } catch (error) {
      setSaveResult({
        tone: "error",
        text: errorMessage(error, "The shared folder could not be selected."),
      });
    } finally {
      setIsBrowsingSharedFolder(false);
    }
  }

  async function handleTestPing() {
    if (pageBusy || settingsDirty) {
      return;
    }

    pingAbortRef.current?.abort();
    const controller = new AbortController();
    pingAbortRef.current = controller;
    setIsTesting(true);
    setTestResult(null);

    let baseline: NotificationRuntimeStatus | null = null;
    try {
      baseline = await getNotificationStatus();
    } catch {
      baseline = null;
    }

    try {
      await sendTestPing();
    } catch (error) {
      if (!controller.signal.aborted) {
        setTestResult({
          tone: "error",
          text: errorMessage(error, "The test ping could not be queued."),
        });
        setIsTesting(false);
      }
      return;
    }

    let latest: NotificationRuntimeStatus | null = null;
    try {
      for (let attempt = 0; attempt < PING_POLL_ATTEMPTS; attempt += 1) {
        latest = await getNotificationStatus();
        if (latest && statusAdvanced(latest, baseline)) {
          if (!controller.signal.aborted) {
            setTestResult(messageFromRuntimeStatus(latest));
          }
          return;
        }
        if (attempt < PING_POLL_ATTEMPTS - 1) {
          const shouldContinue = await abortableDelay(PING_POLL_INTERVAL_MS, controller.signal);
          if (!shouldContinue) {
            return;
          }
        }
      }

      if (!controller.signal.aborted) {
        setTestResult({
          tone: "warning",
          text: latest
            ? `Test ping queued. Latest notification status: ${latest.message}`
            : "Test ping queued. Check Teams for the card.",
        });
      }
    } catch (error) {
      if (!controller.signal.aborted) {
        setTestResult({
          tone: "warning",
          text: `Test ping queued, but its result could not be read: ${errorMessage(
            error,
            "Notification status is unavailable.",
          )}`,
        });
      }
    } finally {
      if (!controller.signal.aborted) {
        setIsTesting(false);
      }
    }
  }

  return (
    <main className="flex h-screen min-h-[400px] w-full min-w-[360px] max-w-full flex-col overflow-hidden bg-[#20201f] text-white">
      <header className="flex shrink-0 items-center gap-3 border-b border-[#454542] px-4 py-3">
        <button
          type="button"
          aria-label="Back to operator panel"
          autoFocus
          onClick={handleBack}
          disabled={pageBusy}
          className="inline-flex h-9 items-center gap-1.5 rounded-md bg-[#3a3a38] px-3 text-[8.5pt] font-semibold text-white transition hover:bg-[#454542] disabled:cursor-not-allowed disabled:opacity-60"
        >
          <ArrowLeft className="h-4 w-4" aria-hidden="true" />
          Back
        </button>
        <div className="min-w-0">
          <h1 className="truncate text-[13pt] font-semibold leading-tight">Notification settings</h1>
          <p className="mt-0.5 text-[7.5pt] leading-tight text-[#b7b1a8]">
            Station identity and Teams delivery
          </p>
        </div>
      </header>

      <div className="min-h-0 flex-1 overflow-y-auto p-4 [scrollbar-width:thin]">
        <div className="mx-auto w-full max-w-[620px] space-y-4">
          {isLoading ? (
            <div role="status" className="flex items-center justify-center gap-2 py-12 text-[9pt] text-[#d8d2c8]">
              <LoaderCircle className="h-4 w-4 animate-spin" aria-hidden="true" />
              Loading notification settings...
            </div>
          ) : loadError ? (
            <section className="rounded-md border border-[#6f332c] bg-[#301f22] p-4">
              <div role="alert" className="text-[8.5pt] leading-snug text-[#f4b1a9]">
                {loadError}
              </div>
              <button
                type="button"
                onClick={() => void handleRetryLoad()}
                className="mt-3 inline-flex min-h-8 items-center justify-center rounded bg-[#3a3a38] px-3 text-[8pt] font-semibold transition hover:bg-[#454542]"
              >
                Retry
              </button>
            </section>
          ) : settings ? (
            <>
              <form
                aria-label="Notification settings form"
                onSubmit={(event) => void handleSave(event)}
                className="rounded-md border border-[#454542] bg-[#292928] p-4 shadow-sm"
              >
                <div className="grid gap-3 sm:grid-cols-2">
                  <label className="block text-[8pt] font-semibold text-[#d8d2c8]">
                    Station
                    <select
                      value={settings.station_id}
                      disabled={pageBusy}
                      onChange={(event) => {
                        const stationId = event.target.value;
                        updateSettings({
                          station_id: stationId,
                          station_name: stationNameForId(stationId),
                        });
                      }}
                      className={inputClassName}
                    >
                      {NOTIFICATION_STATIONS.map((station) => (
                        <option key={station.id} value={station.id}>
                          {station.name}
                        </option>
                      ))}
                    </select>
                  </label>

                  <label className="block text-[8pt] font-semibold text-[#d8d2c8]">
                    Destination name
                    <input
                      value={settings.teams_destination_name}
                      disabled={pageBusy}
                      onChange={(event) =>
                        updateSettings({ teams_destination_name: event.target.value })
                      }
                      autoComplete="off"
                      className={inputClassName}
                    />
                  </label>
                </div>

                <label className="mt-3 block text-[8pt] font-semibold text-[#d8d2c8]">
                  Teams webhook URL
                  <input
                    type="password"
                    value={settings.teams_webhook_url}
                    disabled={pageBusy}
                    onChange={(event) => updateSettings({ teams_webhook_url: event.target.value })}
                    autoComplete="off"
                    spellCheck={false}
                    placeholder={
                      settings.webhook_configured
                        ? "Saved webhook configured; enter a URL only to replace it"
                        : "Paste the Power Automate Workflow URL"
                    }
                    aria-describedby="webhook-help"
                    className={inputClassName}
                  />
                </label>
                <p id="webhook-help" className="mt-1 text-[7.2pt] leading-snug text-[#b7b1a8]">
                  The signed URL stays masked and is never included in automation logs.
                </p>

                <label className="mt-3 flex min-h-9 items-center gap-2 rounded border border-[#454542] bg-[#242423] px-3 text-[8.5pt] font-medium">
                  <input
                    type="checkbox"
                    checked={settings.enabled}
                    disabled={pageBusy}
                    onChange={(event) => updateSettings({ enabled: event.target.checked })}
                    className="h-4 w-4 accent-[#1f74ae]"
                  />
                  Notifications enabled
                </label>

                <div className="mt-3">
                  <div className="text-[8pt] font-semibold text-[#d8d2c8]">
                    Shared OneDrive folder
                  </div>
                  <div className="mt-1 flex gap-1.5">
                    <input
                      readOnly
                      value={settings.shared_shift_log_path}
                      disabled={pageBusy}
                      autoComplete="off"
                      spellCheck={false}
                      placeholder="Optional — browse to a shared OneDrive folder"
                      aria-label="Shared OneDrive folder"
                      aria-describedby="shift-log-help"
                      title={settings.shared_shift_log_path || undefined}
                      className={cn(inputClassName, "mt-0 min-w-0 flex-1")}
                    />
                    <button
                      type="button"
                      onClick={() => void handleBrowseSharedFolder()}
                      disabled={pageBusy}
                      className="inline-flex h-9 shrink-0 items-center justify-center gap-1.5 rounded border border-[#454542] bg-[#3a3a38] px-2.5 text-[8pt] font-semibold text-white transition hover:bg-[#454542] disabled:cursor-not-allowed disabled:opacity-60"
                    >
                      {isBrowsingSharedFolder ? (
                        <LoaderCircle className="h-3.5 w-3.5 animate-spin" aria-hidden="true" />
                      ) : (
                        <FolderOpen className="h-3.5 w-3.5" aria-hidden="true" />
                      )}
                      Browse
                    </button>
                    <button
                      type="button"
                      aria-label="Clear shared folder"
                      title="Clear shared folder"
                      onClick={() => updateSettings({ shared_shift_log_path: "" })}
                      disabled={pageBusy || !settings.shared_shift_log_path}
                      className="inline-flex h-9 w-9 shrink-0 items-center justify-center rounded border border-[#454542] bg-[#3a3a38] text-[#d8d2c8] transition hover:bg-[#454542] hover:text-white disabled:cursor-not-allowed disabled:opacity-45"
                    >
                      <X className="h-3.5 w-3.5" aria-hidden="true" />
                    </button>
                  </div>
                  <p id="shift-log-help" className="mt-1 text-[7.2pt] leading-snug text-[#b7b1a8]">
                    Pick the same shared folder on every station (for example a hidden OneDrive
                    folder). On Save the app creates <code className="text-[#d8d2c8]">shift_log.json</code>{" "}
                    and <code className="text-[#d8d2c8]">stations/test-station-1…4</code> inside it.
                    Leave empty to disable multi-station rollups.
                  </p>
                </div>

                <div className="mt-4 flex flex-wrap items-center justify-between gap-2 border-t border-[#454542] pt-3">
                  <button
                    type="button"
                    onClick={() => void handleTestPing()}
                    disabled={pageBusy || settingsDirty}
                    className="inline-flex min-h-9 items-center justify-center gap-1.5 rounded-md bg-[#3a3a38] px-3 py-2 text-[8.5pt] font-semibold text-white shadow-sm transition hover:bg-[#454542] disabled:cursor-not-allowed disabled:opacity-60"
                  >
                    {isTesting ? (
                      <LoaderCircle className="h-3.5 w-3.5 animate-spin" aria-hidden="true" />
                    ) : (
                      <Send className="h-3.5 w-3.5" aria-hidden="true" />
                    )}
                    {isTesting ? "Testing..." : "Send test ping"}
                  </button>
                  <button
                    type="submit"
                    disabled={pageBusy || !settingsDirty}
                    className="inline-flex min-h-9 items-center justify-center gap-1.5 rounded-md bg-[#1d7f47] px-3 py-2 text-[8.5pt] font-semibold text-white shadow-sm transition hover:bg-[#238a50] disabled:cursor-not-allowed disabled:opacity-60"
                  >
                    {isSaving ? (
                      <LoaderCircle className="h-3.5 w-3.5 animate-spin" aria-hidden="true" />
                    ) : (
                      <Save className="h-3.5 w-3.5" aria-hidden="true" />
                    )}
                    {isSaving ? "Saving..." : "Save settings"}
                  </button>
                </div>
                {settingsDirty ? (
                  <p className="mt-2 text-[7.2pt] leading-tight text-[#e8bd69]">
                    Save changes before sending a test ping.
                  </p>
                ) : null}
                {saveResult ? <ResultLine result={saveResult} /> : null}
                {testResult ? <ResultLine result={testResult} /> : null}
              </form>

              <form
                aria-label="Change settings password"
                onSubmit={(event) => void handlePasswordChange(event)}
                className="rounded-md border border-[#454542] bg-[#292928] p-4 shadow-sm"
              >
                <div className="flex items-center gap-2">
                  <KeyRound className="h-4 w-4 text-[#8dc7ef]" aria-hidden="true" />
                  <h2 className="text-[10pt] font-semibold">Change password</h2>
                </div>
                <p className="mt-1 text-[7.2pt] leading-snug text-[#b7b1a8]">
                  This password is a light operator lock, not a security credential.
                </p>

                <div className="mt-3 grid gap-3 sm:grid-cols-3">
                  <label className="block text-[8pt] font-semibold text-[#d8d2c8]">
                    Current password
                    <input
                      type="password"
                      value={passwordFields.current}
                      disabled={pageBusy}
                      onChange={(event) => updatePasswordField("current", event.target.value)}
                      autoComplete="current-password"
                      className={inputClassName}
                    />
                  </label>
                  <label className="block text-[8pt] font-semibold text-[#d8d2c8]">
                    New password
                    <input
                      type="password"
                      value={passwordFields.next}
                      disabled={pageBusy}
                      onChange={(event) => updatePasswordField("next", event.target.value)}
                      autoComplete="new-password"
                      className={inputClassName}
                    />
                  </label>
                  <label className="block text-[8pt] font-semibold text-[#d8d2c8]">
                    Confirm password
                    <input
                      type="password"
                      value={passwordFields.confirm}
                      disabled={pageBusy}
                      onChange={(event) => updatePasswordField("confirm", event.target.value)}
                      autoComplete="new-password"
                      className={inputClassName}
                    />
                  </label>
                </div>

                <div className="mt-3 flex justify-end">
                  <button
                    type="submit"
                    disabled={pageBusy}
                    className="inline-flex min-h-9 items-center justify-center gap-1.5 rounded-md bg-[#1f74ae] px-3 py-2 text-[8.5pt] font-semibold text-white shadow-sm transition hover:bg-[#2874a8] disabled:cursor-wait disabled:opacity-70"
                  >
                    {isChangingPassword ? (
                      <LoaderCircle className="h-3.5 w-3.5 animate-spin" aria-hidden="true" />
                    ) : null}
                    {isChangingPassword ? "Updating..." : "Update password"}
                  </button>
                </div>
                {passwordResult ? <ResultLine result={passwordResult} /> : null}
              </form>
            </>
          ) : null}
        </div>
      </div>
    </main>
  );
}

function ResultLine({ result }: { result: ResultMessage }) {
  return (
    <div
      role={result.tone === "error" ? "alert" : "status"}
      className={cn(
        "mt-2 text-[7.5pt] leading-snug",
        result.tone === "success"
          ? "text-[#86efac]"
          : result.tone === "warning"
            ? "text-[#e8bd69]"
            : "text-[#f4b1a9]",
      )}
    >
      {result.text}
    </div>
  );
}

function settingsFingerprint(settings: AppNotificationSettingsView) {
  return JSON.stringify({
    enabled: settings.enabled,
    teams_destination_name: settings.teams_destination_name,
    teams_webhook_url: settings.teams_webhook_url,
    station_id: settings.station_id,
    station_name: settings.station_name,
    idle_timeout_minutes: settings.idle_timeout_minutes,
    events: settings.events,
    shared_shift_log_path: settings.shared_shift_log_path,
  });
}

function saveRequestFromSettings(
  settings: AppNotificationSettingsView,
): SaveNotificationSettingsRequest {
  return {
    enabled: settings.enabled,
    teams_destination_name: settings.teams_destination_name.trim(),
    teams_webhook_url: settings.teams_webhook_url.trim(),
    station_id: settings.station_id,
    station_name: stationNameForId(settings.station_id),
    idle_timeout_minutes: settings.idle_timeout_minutes,
    events: settings.events,
    shared_shift_log_path: settings.shared_shift_log_path.trim(),
  };
}

function settingsAfterSave(
  current: AppNotificationSettingsView,
  request: SaveNotificationSettingsRequest,
): AppNotificationSettingsView {
  return {
    ...current,
    ...request,
    webhook_configured:
      current.webhook_configured || Boolean(request.teams_webhook_url.trim()),
  };
}

function statusAdvanced(
  latest: NotificationRuntimeStatus,
  baseline: NotificationRuntimeStatus | null,
) {
  if (
    latest.event_kind !== "test_ping" ||
    latest.state === "idle" ||
    latest.state === "ready"
  ) {
    return false;
  }
  if (!baseline) {
    return Boolean(latest.updated_at);
  }
  if (latest.updated_at && latest.updated_at !== baseline.updated_at) {
    return true;
  }
  return latest.state !== baseline.state || latest.message !== baseline.message;
}

function messageFromRuntimeStatus(status: NotificationRuntimeStatus): ResultMessage {
  if (status.state === "failed") {
    return { tone: "error", text: status.message };
  }
  if (status.state === "skipped") {
    return { tone: "warning", text: status.message };
  }
  return { tone: "success", text: status.message };
}

function abortableDelay(milliseconds: number, signal: AbortSignal) {
  return new Promise<boolean>((resolve) => {
    if (signal.aborted) {
      resolve(false);
      return;
    }

    const handle = window.setTimeout(() => {
      signal.removeEventListener("abort", handleAbort);
      resolve(true);
    }, milliseconds);
    const handleAbort = () => {
      window.clearTimeout(handle);
      resolve(false);
    };
    signal.addEventListener("abort", handleAbort, { once: true });
  });
}

function errorMessage(error: unknown, fallback: string) {
  if (error instanceof Error && error.message.trim()) {
    return error.message;
  }
  if (error && typeof error === "object" && "message" in error) {
    const message = (error as { message?: unknown }).message;
    if (typeof message === "string" && message.trim()) {
      return message;
    }
  }
  return fallback;
}
