import { useEffect, useMemo, useRef, useState, type FormEvent } from "react";
import {
  ArrowLeft,
  CalendarClock,
  ChevronDown,
  ChevronRight,
  ClipboardList,
  FolderOpen,
  KeyRound,
  ListChecks,
  LoaderCircle,
  Radio,
  Save,
  Send,
  Shield,
  X,
} from "lucide-react";

import { cn } from "@/shared/lib/utils";
import { ScrollRegion } from "@/shared/ui/ScrollRegion";

import { SettingsPasswordModal } from "./SettingsPasswordModal";
import { ShiftRangePicker } from "./ShiftRangePicker";
import { shiftScheduleError } from "./shiftTime";
import {
  NOTIFICATION_STATIONS,
  createDefaultNotificationSettings,
  isPendingSharedFolderConnect,
  resolveSaveScope,
  shouldApplySettingsReload,
  stationNameForId,
  type AppNotificationSettingsView,
  type ChangeSettingsPassword,
  type ChooseSharedNotificationsFolder,
  type GetNotificationStatus,
  type LoadNotificationSettings,
  type NotificationRuntimeStatus,
  type PostShiftSummary,
  type PreviewShiftSummary,
  type SaveNotificationSettings,
  type SaveNotificationSettingsRequest,
  type SendNotificationTest,
  type SettingsSaveScope,
  type ShiftSummaryPreview,
  type ShiftWindow,
  type VerifySettingsPassword,
} from "./settingsTypes";

const inputClassName =
  "mt-1 h-9 w-full rounded-md border border-[#454542] bg-[#1f1f1e] px-2.5 text-[8.5pt] text-white placeholder:text-[#777772] outline-none transition focus:border-[#1f74ae] focus:ring-2 focus:ring-cyan-200/25 disabled:cursor-not-allowed disabled:opacity-60";
const selectClassName = cn(
  inputClassName,
  "cursor-pointer appearance-none pr-9 font-medium",
);
const PING_POLL_ATTEMPTS = 45;
const PING_POLL_INTERVAL_MS = 500;
/** Reload floor/local settings while Settings is open and the form is clean. */
const SETTINGS_OPEN_POLL_MS = 45_000;

type ResultMessage = { tone: "success" | "warning" | "error"; text: string };
type PasswordFields = { current: string; next: string; confirm: string };
export type SettingsView =
  | "home"
  | "shifts"
  | "summaryOptions"
  | "advanced"
  | "station"
  | "password";

const emptyPasswordFields: PasswordFields = { current: "", next: "", confirm: "" };

export interface NotificationSettingsPageProps {
  onBack: () => void;
  loadSettings: LoadNotificationSettings;
  saveSettings: SaveNotificationSettings;
  changePassword: ChangeSettingsPassword;
  sendTestPing: SendNotificationTest;
  getNotificationStatus: GetNotificationStatus;
  chooseSharedFolder: ChooseSharedNotificationsFolder;
  previewShiftSummary: PreviewShiftSummary;
  postShiftSummary: PostShiftSummary;
  verifyPassword: VerifySettingsPassword;
}

export function NotificationSettingsPage({
  onBack,
  loadSettings,
  saveSettings,
  changePassword,
  sendTestPing,
  getNotificationStatus,
  chooseSharedFolder,
  previewShiftSummary,
  postShiftSummary,
  verifyPassword,
}: NotificationSettingsPageProps) {
  const pingAbortRef = useRef<AbortController | null>(null);
  const secondShiftDraftRef = useRef<ShiftWindow | null>(null);
  const [view, setView] = useState<SettingsView>("home");
  const [advancedUnlocked, setAdvancedUnlocked] = useState(false);
  const [advancedPasswordOpen, setAdvancedPasswordOpen] = useState(false);
  /** Briefly retained for Connect when floor password matches Advanced unlock. */
  const [advancedUnlockPassword, setAdvancedUnlockPassword] = useState("");
  /**
   * Dedicated existing-floor password for Connect when the shared floor password
   * differs from this PC's local Advanced password (e.g. floor 4242, local 0601).
   */
  const [floorConnectPassword, setFloorConnectPassword] = useState("");
  const [floorPasswordPromptOpen, setFloorPasswordPromptOpen] = useState(false);

  const [settings, setSettings] = useState<AppNotificationSettingsView | null>(null);
  const [savedSettings, setSavedSettings] = useState<AppNotificationSettingsView | null>(null);
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
  const [summaryShiftLabel, setSummaryShiftLabel] = useState("");
  const [summaryPreview, setSummaryPreview] = useState<ShiftSummaryPreview | null>(null);
  const [summaryResult, setSummaryResult] = useState<ResultMessage | null>(null);
  const [isSummaryBusy, setIsSummaryBusy] = useState(false);
  /** When set, show the unsaved-leave dialog before navigating to this target. */
  const [leaveTarget, setLeaveTarget] = useState<"home" | "advanced" | "exit" | null>(null);

  // Preview load must not block ← navigation (isSummaryBusy is only for post/preview UI).
  const pageBusy = isSaving || isChangingPassword || isTesting || isBrowsingSharedFolder;
  const isDirtyRef = useRef(false);
  const isSavingRef = useRef(false);
  const savedSettingsRef = useRef<AppNotificationSettingsView | null>(null);

  useEffect(() => {
    let cancelled = false;
    void loadSettings()
      .then((loadedSettings) => {
        if (cancelled) return;
        const loaded = normalizeLoaded(loadedSettings ?? createDefaultNotificationSettings());
        setSettings(loaded);
        setSavedSettings(loaded);
        setSavedFingerprint(settingsFingerprint(loaded));
        setSummaryShiftLabel(loaded.shifts[0]?.label ?? "");
      })
      .catch((error) => {
        if (!cancelled) {
          setLoadError(errorMessage(error, "Notification settings could not be loaded."));
        }
      })
      .finally(() => {
        if (!cancelled) setIsLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [loadSettings]);

  useEffect(() => () => pingAbortRef.current?.abort(), []);

  useEffect(() => {
    if (view !== "home" || !settings || isLoading || loadError) return;
    let cancelled = false;
    // Load summary preview off the effect body so setState is not sync-in-effect (eslint).
    void (async () => {
      await Promise.resolve();
      if (cancelled) return;
      setIsSummaryBusy(true);
      try {
        const preview = await previewShiftSummary(summaryShiftLabel);
        if (!cancelled) setSummaryPreview(preview);
      } catch {
        if (!cancelled) setSummaryPreview(null);
      } finally {
        if (!cancelled) setIsSummaryBusy(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [view, settings, isLoading, loadError, summaryShiftLabel, previewShiftSummary]);

  const settingsDirty = useMemo(
    () => Boolean(settings && savedFingerprint && settingsFingerprint(settings) !== savedFingerprint),
    [savedFingerprint, settings],
  );
  const passwordDirty = Boolean(
    passwordFields.current || passwordFields.next || passwordFields.confirm,
  );
  const isDirty = settingsDirty || passwordDirty;
  const shiftValidationError = settings ? shiftScheduleError(settings.shifts) : "";

  // Keep latest dirty/saving/saved snapshots for the open Settings poll (do not read in render).
  useEffect(() => {
    isDirtyRef.current = isDirty;
    isSavingRef.current = isSaving;
    savedSettingsRef.current = savedSettings;
  }, [isDirty, isSaving, savedSettings]);

  // While Settings is mounted, re-load floor/local settings every ~45s if the form is clean.
  useEffect(() => {
    if (isLoading || loadError) return;
    let cancelled = false;
    const tick = async () => {
      if (!shouldApplySettingsReload(isDirtyRef.current) || isSavingRef.current) return;
      try {
        const loadedSettings = await loadSettings();
        if (cancelled || !shouldApplySettingsReload(isDirtyRef.current) || isSavingRef.current) {
          return;
        }
        const loaded = normalizeLoaded(loadedSettings ?? createDefaultNotificationSettings());
        const prevUpdatedAt = savedSettingsRef.current?.floor_sync?.updated_at ?? null;
        const nextUpdatedAt = loaded.floor_sync?.updated_at ?? null;
        setSettings(loaded);
        setSavedSettings(loaded);
        setSavedFingerprint(settingsFingerprint(loaded));
        if (
          prevUpdatedAt &&
          nextUpdatedAt &&
          prevUpdatedAt !== nextUpdatedAt
        ) {
          setSaveResult({
            tone: "success",
            text: "Floor settings updated from shared folder.",
          });
        }
      } catch {
        // Soft-fail open poll; delivery still uses the backend worker poll.
      }
    };
    const id = window.setInterval(() => {
      void tick();
    }, SETTINGS_OPEN_POLL_MS);
    return () => {
      cancelled = true;
      window.clearInterval(id);
    };
  }, [isLoading, loadError, loadSettings]);

  function updateSettings(update: Partial<AppNotificationSettingsView>) {
    setSettings((current) => (current ? { ...current, ...update } : current));
    setSaveResult(null);
    setTestResult(null);
  }

  function requestAdvanced() {
    if (advancedUnlocked) {
      setView("advanced");
      return;
    }
    setAdvancedPasswordOpen(true);
  }

  function completeLeave(target: "home" | "advanced" | "exit") {
    setLeaveTarget(null);
    setSummaryResult(null);
    setPasswordResult(null);
    setSaveResult(null);
    setTestResult(null);
    if (target === "exit") {
      onBack();
      return;
    }
    setView(target === "advanced" && advancedUnlocked ? "advanced" : "home");
  }

  function discardUnsavedChanges() {
    if (savedSettings) {
      setSettings(savedSettings);
      setSavedFingerprint(settingsFingerprint(savedSettings));
    }
    setPasswordFields(emptyPasswordFields);
    setSaveResult(null);
    setPasswordResult(null);
    setTestResult(null);
  }

  function requestLeave(target: "home" | "advanced" | "exit") {
    if (pageBusy) return;
    if (!isDirty) {
      completeLeave(target);
      return;
    }
    setLeaveTarget(target);
  }

  function handleLeaveSettings() {
    requestLeave("exit");
  }

  function handleBackNavigation() {
    if (pageBusy) return;
    if (view === "home") {
      handleLeaveSettings();
      return;
    }
    if (view === "station" || view === "password") {
      requestLeave(advancedUnlocked ? "advanced" : "home");
      return;
    }
    // shifts, summaryOptions, advanced → settings home
    requestLeave("home");
  }

  async function handleSave(event?: FormEvent): Promise<boolean> {
    event?.preventDefault();
    if (!settings || pageBusy) return false;
    if (view === "shifts" && shiftValidationError) {
      setSaveResult({ tone: "error", text: shiftValidationError });
      return false;
    }
    const { scope, connect_password } = resolveSaveScope(
      view,
      settings,
      savedSettings,
      advancedUnlockPassword,
      floorConnectPassword,
    );
    const request = saveRequestFromSettings(settings, scope, connect_password);
    setIsSaving(true);
    setSaveResult(null);
    try {
      const saved = await saveSettings(request);
      const nextSettings = normalizeLoaded(saved ?? settingsAfterSave(settings, request));
      setSettings(nextSettings);
      setSavedSettings(nextSettings);
      setSavedFingerprint(settingsFingerprint(nextSettings));
      setFloorConnectPassword("");
      setFloorPasswordPromptOpen(false);
      setSaveResult({ tone: "success", text: "Settings saved." });
      return true;
    } catch (error) {
      const raw = errorMessage(error, "Settings could not be saved.");
      if (/floor password/i.test(raw)) {
        setFloorPasswordPromptOpen(true);
        setSaveResult({
          tone: "error",
          text: "Floor password is incorrect. Enter the shared floor password below (it may differ from this PC’s Advanced password), then Save again.",
        });
      } else {
        setSaveResult({
          tone: "error",
          text: raw,
        });
      }
      return false;
    } finally {
      setIsSaving(false);
    }
  }

  async function handleSaveAndLeave() {
    if (!leaveTarget || pageBusy) return;

    if (passwordDirty) {
      if (!passwordFields.current || !passwordFields.next.trim()) {
        setPasswordResult({
          tone: "error",
          text: "Finish updating the password (or discard) before leaving.",
        });
        setLeaveTarget(null);
        return;
      }
      if (passwordFields.next !== passwordFields.confirm) {
        setPasswordResult({
          tone: "error",
          text: "New password and confirmation do not match.",
        });
        setLeaveTarget(null);
        return;
      }
      setIsChangingPassword(true);
      setPasswordResult(null);
      try {
        await changePassword(passwordFields.current, passwordFields.next, passwordFields.confirm);
        setPasswordFields(emptyPasswordFields);
      } catch (error) {
        setPasswordResult({
          tone: "error",
          text: errorMessage(error, "The settings password could not be updated."),
        });
        setLeaveTarget(null);
        return;
      } finally {
        setIsChangingPassword(false);
      }
    }

    if (settingsDirty) {
      const saved = await handleSave();
      if (!saved) {
        setLeaveTarget(null);
        return;
      }
    }

    completeLeave(leaveTarget);
  }

  function handleDiscardAndLeave() {
    if (!leaveTarget || pageBusy) return;
    discardUnsavedChanges();
    completeLeave(leaveTarget);
  }

  async function handlePasswordChange(event: FormEvent) {
    event.preventDefault();
    if (pageBusy) return;
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
      await changePassword(passwordFields.current, passwordFields.next, passwordFields.confirm);
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
    if (pageBusy || !settings) return;
    setIsBrowsingSharedFolder(true);
    try {
      const selected = await chooseSharedFolder();
      if (selected) updateSettings({ shared_shift_log_path: selected });
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
    if (pageBusy || settingsDirty) return;
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
    try {
      for (let attempt = 0; attempt < PING_POLL_ATTEMPTS; attempt += 1) {
        const latest = await getNotificationStatus();
        if (latest && statusAdvanced(latest, baseline)) {
          if (!controller.signal.aborted) setTestResult(messageFromRuntimeStatus(latest));
          return;
        }
        if (attempt < PING_POLL_ATTEMPTS - 1) {
          const ok = await abortableDelay(PING_POLL_INTERVAL_MS, controller.signal);
          if (!ok) return;
        }
      }
      if (!controller.signal.aborted) {
        setTestResult({
          tone: "warning",
          text: "Test ping queued. Check Teams for the card.",
        });
      }
    } finally {
      if (!controller.signal.aborted) setIsTesting(false);
    }
  }

  async function handlePostSummary() {
    if (!settings) return;
    const shiftName =
      summaryShiftLabel || settings.shifts[0]?.label || "this shift";
    if (
      !window.confirm(
        `Post the end-of-shift summary now for ${shiftName}?\n\n` +
          "This sends the floor card early and skips the next scheduled end-of-shift post for this period. " +
          "Other stations will see that the summary was already sent and should not post again.\n\n" +
          "The shared shift log is cleared after a successful post.",
      )
    ) {
      return;
    }
    setIsSummaryBusy(true);
    setSummaryResult(null);
    try {
      const result = await postShiftSummary(summaryShiftLabel);
      setSummaryResult({
        tone: "success",
        text: result?.message ?? "End-of-shift summary posted.",
      });
      if (result?.text) {
        setSummaryPreview({
          text: result.text,
          is_summary_poster: settings.is_summary_poster,
          poster_station_id: settings.summary_poster_station_id,
          poster_station_name: stationNameForId(
            settings.summary_poster_station_id,
            settings.stations,
          ),
          event_count: 0,
          shared_folder_configured: Boolean(settings.shared_shift_log_path),
          already_posted: true,
          last_summary_at: new Date().toISOString(),
          last_summary_by: settings.station_name,
          last_summary_shift: summaryShiftLabel || settings.shifts[0]?.label || null,
        });
      } else {
        setSummaryPreview(await previewShiftSummary(summaryShiftLabel));
      }
    } catch (error) {
      setSummaryResult({
        tone: "error",
        text: errorMessage(error, "End-of-shift summary could not be posted."),
      });
    } finally {
      setIsSummaryBusy(false);
    }
  }

  function setShiftCount(count: 1 | 2) {
    if (!settings) return;
    const next = [...settings.shifts];
    if (count === 1 && next[1]) {
      secondShiftDraftRef.current = next[1];
    }
    while (next.length < count) {
      next.push(
        next.length === 0
          ? { label: "Day", start_time: "06:00", end_time: "15:00" }
          : secondShiftDraftRef.current ?? {
              label: "Night",
              start_time: next[0]?.end_time ?? "15:00",
              end_time: "23:00",
            },
      );
    }
    while (next.length > count) next.pop();
    updateSettings({ shifts: next });
  }

  const shiftMode: 1 | 2 = settings?.shifts.length === 2 ? 2 : 1;

  const pageTitle = useMemo(() => {
    switch (view) {
      case "shifts":
        return "Shifts";
      case "summaryOptions":
        return "Summary options";
      case "advanced":
        return "Advanced";
      case "station":
        return "Station & Teams";
      case "password":
        return "Change password";
      default:
        return "Settings";
    }
  }, [view]);

  function updateShift(index: number, patch: Partial<ShiftWindow>) {
    if (!settings) return;
    const currentFirstEnd = settings.shifts[0]?.end_time;
    const shouldKeepSecondShiftConnected =
      index === 0 &&
      Boolean(patch.end_time) &&
      settings.shifts[1]?.start_time === currentFirstEnd;
    const next = settings.shifts.map((shift, i) =>
      i === index ? { ...shift, ...patch } : shift,
    );
    if (shouldKeepSecondShiftConnected && next[1] && patch.end_time) {
      next[1] = { ...next[1], start_time: patch.end_time };
    }
    updateSettings({
      shifts: next,
    });
  }

  function toggleIncludedStation(stationId: string) {
    if (!settings) return;
    const current = new Set(settings.summary_included_station_ids);
    if (current.has(stationId)) {
      if (current.size <= 1) return;
      current.delete(stationId);
    } else {
      current.add(stationId);
    }
    const included = settings.stations.map((s) => s.id).filter((id) => current.has(id));
    let poster = settings.summary_poster_station_id;
    if (!current.has(poster)) {
      poster = included[0] ?? poster;
    }
    updateSettings({
      summary_included_station_ids: included,
      summary_poster_station_id: poster,
      is_summary_poster: settings.station_id === poster,
    });
  }

  function setMainStation(stationId: string) {
    if (!settings || pageBusy) return;
    const current = new Set(settings.summary_included_station_ids);
    current.add(stationId);
    updateSettings({
      summary_poster_station_id: stationId,
      is_summary_poster: settings.station_id === stationId,
      summary_included_station_ids: settings.stations
        .map((s) => s.id)
        .filter((id) => current.has(id)),
    });
  }

  return (
    <main className="flex h-screen min-h-[400px] w-full min-w-[360px] max-w-full flex-col overflow-hidden bg-[#20201f] text-white">
      <header className="relative flex shrink-0 items-center justify-center border-b border-[#454542] px-4 py-3">
        <button
          type="button"
          aria-label={view === "home" ? "Back to operator panel" : "Back to settings menu"}
          onClick={handleBackNavigation}
          disabled={pageBusy}
          className="absolute left-3 top-1/2 inline-flex h-8 w-8 -translate-y-1/2 items-center justify-center text-[#d8d2c8] outline-none transition hover:text-white focus:outline-none focus-visible:text-white disabled:cursor-not-allowed disabled:opacity-40"
        >
          <ArrowLeft className="h-5 w-5" aria-hidden="true" />
        </button>
        <h1 className="px-10 text-center text-[13pt] font-semibold leading-tight tracking-tight">
          {pageTitle}
        </h1>
        {view === "home" ? (
          <button
            type="button"
            aria-label="Advanced"
            title="Advanced"
            onClick={() => requestAdvanced()}
            disabled={pageBusy}
            className="absolute right-3 top-1/2 inline-flex h-8 w-8 -translate-y-1/2 items-center justify-center rounded-md bg-[#3a3a38] text-[#d8d2c8] shadow-sm outline-none transition hover:bg-[#454542] hover:text-white focus:outline-none focus-visible:text-white disabled:cursor-not-allowed disabled:opacity-40"
          >
            <Shield className="h-4 w-4" aria-hidden="true" />
          </button>
        ) : null}
      </header>

      {isLoading ? (
        <div className="flex flex-1 items-center justify-center gap-2 text-[9pt] text-[#d8d2c8]">
          <LoaderCircle className="h-4 w-4 animate-spin" /> Loading...
        </div>
      ) : loadError ? (
        <div className="p-4 text-[#f4b1a9]">{loadError}</div>
      ) : settings ? (
        <ScrollRegion
          aria-label="Notification settings content"
          contentClassName="mx-auto w-full max-w-[620px] space-y-3 p-4"
        >
          {view === "home" ? (
            <div className="space-y-3">
              <div className="space-y-2">
                <MenuButton
                  icon={CalendarClock}
                  title="Shifts"
                  onClick={() => {
                    if (settings.shifts.length === 0) setShiftCount(1);
                    setView("shifts");
                  }}
                />
                <MenuButton
                  icon={ListChecks}
                  title="Summary options"
                  onClick={() => setView("summaryOptions")}
                />
              </div>

              <section
                aria-label="End of shift"
                className="rounded-md border border-[#454542] bg-[#292928] p-3"
              >
                <div className="mb-2 flex items-center justify-between gap-2">
                  <div className="flex items-center gap-2 text-[10pt] font-semibold">
                    <ClipboardList className="h-4 w-4 shrink-0 text-[#8dc7ef]" aria-hidden="true" />
                    End of shift
                  </div>
                  {settings.shifts.length > 1 ? (
                    <select
                      value={summaryShiftLabel || settings.shifts[0]?.label || ""}
                      disabled={pageBusy}
                      aria-label="Shift"
                      onChange={(event) => setSummaryShiftLabel(event.target.value)}
                      className="h-8 max-w-[55%] rounded-md border border-[#454542] bg-[#1f1f1e] px-2 text-[8pt] font-semibold text-white outline-none focus:border-[#1f74ae]"
                    >
                      {settings.shifts.map((shift) => (
                        <option key={shift.label} value={shift.label}>
                          {shift.label}
                        </option>
                      ))}
                    </select>
                  ) : null}
                </div>

                {summaryPreview?.already_posted ? (
                  <div
                    role="status"
                    className="mb-2 rounded-md border border-[#e8bd69]/40 bg-[#e8bd69]/10 px-3 py-2 text-[8pt] text-[#e8bd69]"
                  >
                    Already posted
                    {summaryPreview.last_summary_by
                      ? ` by ${summaryPreview.last_summary_by}`
                      : ""}
                    {summaryPreview.last_summary_at
                      ? ` at ${summaryPreview.last_summary_at}`
                      : ""}
                    .
                  </div>
                ) : null}

                <div className="rounded border border-[#454542] bg-[#1f1f1e] p-3">
                  {isSummaryBusy && !summaryPreview ? (
                    <div className="flex min-h-[96px] items-center justify-center gap-2 text-[8pt] text-[#b7b1a8]">
                      <LoaderCircle className="h-3.5 w-3.5 animate-spin" aria-hidden="true" />
                      Loading preview...
                    </div>
                  ) : summaryPreview?.text ? (
                    <pre className="whitespace-pre-wrap text-[7.5pt] leading-snug text-[#d8d2c8]">
                      {summaryPreview.text}
                    </pre>
                  ) : (
                    <p className="text-[8pt] text-[#9a958c]">
                      No preview yet. Configure shared folder and shifts, then reopen Settings.
                    </p>
                  )}
                </div>

                <button
                  type="button"
                  disabled={
                    pageBusy ||
                    isSummaryBusy ||
                    settingsDirty ||
                    !settings.events.summary ||
                    !settings.shared_shift_log_path ||
                    Boolean(summaryPreview?.already_posted)
                  }
                  onClick={() => void handlePostSummary()}
                  className="mt-2 inline-flex min-h-9 w-full items-center justify-center rounded-md bg-[#1d7f47] px-3 text-[8.5pt] font-semibold text-white disabled:opacity-60"
                >
                  {isSummaryBusy ? (
                    <LoaderCircle className="mr-1.5 h-3.5 w-3.5 animate-spin" aria-hidden="true" />
                  ) : null}
                  Post Summary
                </button>
                {summaryResult ? <ResultLine result={summaryResult} /> : null}
              </section>
            </div>
          ) : null}

          {view === "advanced" && advancedUnlocked ? (
            <div className="space-y-2">
              <MenuButton
                icon={Radio}
                title="Station & Teams"
                onClick={() => setView("station")}
              />
              <MenuButton
                icon={KeyRound}
                title="Change password"
                onClick={() => setView("password")}
              />
            </div>
          ) : null}

          {view === "shifts" ? (
            <form onSubmit={(e) => void handleSave(e)} className="space-y-3 rounded-md border border-[#454542] bg-[#292928] p-4">
              <div>
                <div className="text-[8pt] font-semibold text-[#d8d2c8]">Shift mode</div>
                <div className="mt-1.5 grid grid-cols-2 gap-2">
                  <button
                    type="button"
                    disabled={pageBusy}
                    onClick={() => setShiftCount(1)}
                    className={cn(
                      "inline-flex min-h-10 items-center justify-center rounded-md border px-3 text-[8.5pt] font-semibold transition",
                      shiftMode === 1
                        ? "border-[#1f74ae] bg-[#1f74ae] text-white shadow-sm"
                        : "border-[#454542] bg-[#242423] text-[#d8d2c8] hover:border-[#5a5a56] hover:bg-[#2c2c2a]",
                    )}
                  >
                    Single shift
                  </button>
                  <button
                    type="button"
                    disabled={pageBusy}
                    onClick={() => setShiftCount(2)}
                    className={cn(
                      "inline-flex min-h-10 items-center justify-center rounded-md border px-3 text-[8.5pt] font-semibold transition",
                      shiftMode === 2
                        ? "border-[#1f74ae] bg-[#1f74ae] text-white shadow-sm"
                        : "border-[#454542] bg-[#242423] text-[#d8d2c8] hover:border-[#5a5a56] hover:bg-[#2c2c2a]",
                    )}
                  >
                    Double shift
                  </button>
                </div>
              </div>

              {settings.shifts.map((shift, index) => (
                <div key={index} className="space-y-2 rounded-md border border-[#454542] bg-[#242423] p-3">
                  <label className="block text-[8pt] font-semibold text-[#d8d2c8]">
                    Label
                    <input
                      value={shift.label}
                      disabled={pageBusy}
                      onChange={(e) => updateShift(index, { label: e.target.value })}
                      className={inputClassName}
                    />
                  </label>
                  <ShiftRangePicker
                    startTime={shift.start_time}
                    endTime={shift.end_time}
                    disabled={pageBusy}
                    onStartChange={(start_time) => updateShift(index, { start_time })}
                    onEndChange={(end_time) => updateShift(index, { end_time })}
                  />
                </div>
              ))}

              {shiftValidationError ? (
                <div role="alert" className="text-[7.5pt] leading-snug text-[#f4b1a9]">
                  {shiftValidationError}
                </div>
              ) : null}

              <div className="flex justify-end border-t border-[#454542] pt-3">
                <button
                  type="submit"
                  disabled={pageBusy || !settingsDirty || Boolean(shiftValidationError)}
                  className="inline-flex min-h-9 items-center justify-center gap-1.5 rounded-md bg-[#1d7f47] px-3 text-[8.5pt] font-semibold text-white disabled:opacity-60"
                >
                  {isSaving ? <LoaderCircle className="h-3.5 w-3.5 animate-spin" /> : <Save className="h-3.5 w-3.5" />}
                  {isSaving ? "Saving..." : "Save"}
                </button>
              </div>
              {saveResult ? <ResultLine result={saveResult} /> : null}
            </form>
          ) : null}

          {view === "summaryOptions" ? (
            <form onSubmit={(e) => void handleSave(e)} className="space-y-3 rounded-md border border-[#454542] bg-[#292928] p-4">
              <div>
                <div className="flex items-center justify-between gap-2 text-[8pt] font-semibold text-[#d8d2c8]">
                  <span>Stations in summary</span>
                  <span className="font-medium text-[#9a958c]">Main poster</span>
                </div>
                <div className="mt-2 space-y-1.5" role="list" aria-label="Stations in summary">
                  {settings.stations.map((station) => {
                    const checked = settings.summary_included_station_ids.includes(station.id);
                    const isMain = settings.summary_poster_station_id === station.id;
                    return (
                      <div
                        key={station.id}
                        role="listitem"
                        className="flex min-h-10 items-center gap-2 rounded-md border border-[#454542] bg-[#242423] px-3"
                      >
                        <label className="flex min-w-0 flex-1 cursor-pointer items-center gap-2 text-[8.5pt]">
                          <input
                            type="checkbox"
                            checked={checked}
                            disabled={
                              pageBusy ||
                              (checked && settings.summary_included_station_ids.length <= 1) ||
                              (checked && isMain && settings.summary_included_station_ids.length <= 1)
                            }
                            onChange={() => toggleIncludedStation(station.id)}
                            className="h-4 w-4 accent-[#1f74ae]"
                            aria-label={`Include ${station.name}`}
                          />
                          <span className="truncate">{station.name}</span>
                        </label>
                        <button
                          type="button"
                          role="radio"
                          aria-checked={isMain}
                          aria-label={`${station.name} main poster`}
                          disabled={pageBusy}
                          onClick={() => setMainStation(station.id)}
                          className={cn(
                            "inline-flex h-7 shrink-0 items-center justify-center rounded-full border px-2.5 text-[7.5pt] font-semibold transition",
                            isMain
                              ? "border-[#1f74ae] bg-[#1f74ae] text-white"
                              : "border-[#454542] bg-[#1f1f1e] text-[#b7b1a8] hover:border-[#5a5a56] hover:text-white",
                          )}
                        >
                          Main
                        </button>
                      </div>
                    );
                  })}
                </div>
              </div>

              <div className="flex items-center justify-between gap-3 border-t border-[#454542] pt-3">
                <button
                  type="button"
                  role="switch"
                  aria-checked={settings.events.summary}
                  aria-label="Enable summary notifications"
                  disabled={pageBusy}
                  onClick={() =>
                    updateSettings({
                      events: { ...settings.events, summary: !settings.events.summary },
                    })
                  }
                  className={cn(
                    "inline-flex min-h-9 items-center gap-2 rounded-md border px-2.5 text-[8.5pt] font-semibold transition disabled:opacity-60",
                    settings.events.summary
                      ? "border-[#1d7f47]/60 bg-[#1d7f47]/20 text-[#86efac]"
                      : "border-[#454542] bg-[#242423] text-[#b7b1a8]",
                  )}
                >
                  <span
                    className={cn(
                      "relative inline-flex h-5 w-9 shrink-0 items-center rounded-full transition",
                      settings.events.summary ? "bg-[#1d7f47]" : "bg-[#3a3a38]",
                    )}
                    aria-hidden="true"
                  >
                    <span
                      className={cn(
                        "inline-block h-4 w-4 rounded-full bg-white shadow transition",
                        settings.events.summary ? "translate-x-4" : "translate-x-0.5",
                      )}
                    />
                  </span>
                  {settings.events.summary ? "Enabled" : "Disabled"}
                </button>
                <button
                  type="submit"
                  disabled={pageBusy || !settingsDirty}
                  className="inline-flex min-h-9 items-center justify-center gap-1.5 rounded-md bg-[#1d7f47] px-3 text-[8.5pt] font-semibold text-white disabled:opacity-60"
                >
                  {isSaving ? <LoaderCircle className="h-3.5 w-3.5 animate-spin" /> : <Save className="h-3.5 w-3.5" />}
                  {isSaving ? "Saving..." : "Save"}
                </button>
              </div>
              {saveResult ? <ResultLine result={saveResult} /> : null}
            </form>
          ) : null}

          {view === "station" && advancedUnlocked ? (
            <form onSubmit={(e) => void handleSave(e)} className="space-y-3 rounded-md border border-[#454542] bg-[#292928] p-4">
              <div className="grid gap-3 sm:grid-cols-2">
                <FieldSelect
                  label="This PC station"
                  value={settings.station_id}
                  disabled={pageBusy}
                  onChange={(stationId) => {
                    updateSettings({
                      station_id: stationId,
                      station_name: stationNameForId(stationId, settings.stations),
                      is_summary_poster: stationId === settings.summary_poster_station_id,
                    });
                  }}
                  options={settings.stations.map((s) => ({ value: s.id, label: s.name }))}
                />
                <label className="block text-[8pt] font-semibold text-[#d8d2c8]">
                  Destination name
                  <input
                    value={settings.teams_destination_name}
                    disabled={pageBusy}
                    onChange={(e) => updateSettings({ teams_destination_name: e.target.value })}
                    className={inputClassName}
                  />
                </label>
              </div>
              <div className="rounded-md border border-[#454542] bg-[#242423] px-3 py-2 text-[7.5pt] leading-snug text-[#9a958c]">
                {settings.floor_sync?.message ||
                  (settings.shared_shift_log_path
                    ? "Syncing via shared folder."
                    : "Shared folder not set — settings stay on this PC only.")}
              </div>
              <div>
                <div className="text-[8pt] font-semibold text-[#d8d2c8]">Station display names</div>
                <p className="mt-1 text-[7.5pt] leading-snug text-[#9a958c]">
                  Rename labels used on Teams cards and Settings. Stable ids stay fixed so history does not break.
                  Changes sync to other PCs that share this folder.
                </p>
                <div className="mt-2 space-y-2">
                  {settings.stations.map((station) => (
                    <label key={station.id} className="block text-[8pt] font-semibold text-[#d8d2c8]">
                      <span className="flex items-baseline justify-between gap-2">
                        <span>Display name</span>
                        <span className="font-normal text-[#777772]">{station.id}</span>
                      </span>
                      <input
                        value={station.name}
                        disabled={pageBusy}
                        maxLength={64}
                        onChange={(e) => {
                          const name = e.target.value;
                          const stations = settings.stations.map((entry) =>
                            entry.id === station.id ? { ...entry, name } : entry,
                          );
                          updateSettings({
                            stations,
                            station_name:
                              settings.station_id === station.id
                                ? name
                                : settings.station_name,
                          });
                        }}
                        className={inputClassName}
                      />
                    </label>
                  ))}
                </div>
              </div>
              <label className="block text-[8pt] font-semibold text-[#d8d2c8]">
                Teams webhook URL
                <input
                  type="password"
                  value={settings.teams_webhook_url}
                  disabled={pageBusy}
                  onChange={(e) => updateSettings({ teams_webhook_url: e.target.value })}
                  placeholder={
                    settings.webhook_configured
                      ? "Saved webhook configured; enter a URL only to replace it"
                      : "Paste the Power Automate Workflow URL"
                  }
                  className={inputClassName}
                />
              </label>
              <label className="flex min-h-9 items-center gap-2 rounded border border-[#454542] bg-[#242423] px-3 text-[8.5pt]">
                <input
                  type="checkbox"
                  checked={settings.enabled}
                  disabled={pageBusy}
                  onChange={(e) => updateSettings({ enabled: e.target.checked })}
                  className="h-4 w-4 accent-[#1f74ae]"
                />
                Notifications enabled
              </label>
              <div>
                <div className="text-[8pt] font-semibold text-[#d8d2c8]">Shared OneDrive folder</div>
                <div className="mt-1 flex gap-1.5">
                  <input
                    readOnly
                    value={settings.shared_shift_log_path}
                    aria-label="Shared OneDrive folder"
                    className={cn(inputClassName, "mt-0 min-w-0 flex-1")}
                    placeholder="Browse to shared folder"
                  />
                  <button
                    type="button"
                    onClick={() => void handleBrowseSharedFolder()}
                    disabled={pageBusy}
                    className="inline-flex h-9 items-center gap-1 rounded border border-[#454542] bg-[#3a3a38] px-2 text-[8pt] font-semibold"
                  >
                    <FolderOpen className="h-3.5 w-3.5" /> Browse
                  </button>
                  <button
                    type="button"
                    aria-label="Clear shared folder"
                    onClick={() => {
                      updateSettings({ shared_shift_log_path: "" });
                      setFloorConnectPassword("");
                      setFloorPasswordPromptOpen(false);
                    }}
                    disabled={pageBusy || !settings.shared_shift_log_path}
                    className="inline-flex h-9 w-9 items-center justify-center rounded border border-[#454542] bg-[#3a3a38]"
                  >
                    <X className="h-3.5 w-3.5" />
                  </button>
                </div>
                {(isPendingSharedFolderConnect(settings, savedSettings) ||
                  floorPasswordPromptOpen) && (
                  <label className="mt-2 block text-[8pt] font-semibold text-[#d8d2c8]">
                    Existing floor password
                    <input
                      type="password"
                      value={floorConnectPassword}
                      disabled={pageBusy}
                      autoComplete="off"
                      onChange={(e) => setFloorConnectPassword(e.target.value)}
                      placeholder="Required when the shared floor password differs from this PC"
                      aria-label="Existing floor password"
                      className={inputClassName}
                    />
                    <span className="mt-1 block text-[7.5pt] font-normal leading-snug text-[#9a958c]">
                      Use the shared Settings password stored in the floor file (not only this PC’s
                      Advanced unlock). Leave blank only if the floor still uses the same password
                      as this PC, or you are seeding a brand-new folder.
                    </span>
                  </label>
                )}
              </div>
              <div className="flex flex-wrap justify-between gap-2 border-t border-[#454542] pt-3">
                <button
                  type="button"
                  disabled={pageBusy || settingsDirty}
                  onClick={() => void handleTestPing()}
                  className="inline-flex min-h-9 items-center justify-center gap-1.5 rounded-md bg-[#3a3a38] px-3 text-[8.5pt] font-semibold disabled:opacity-60"
                >
                  <Send className="h-3.5 w-3.5" /> Send test ping
                </button>
                <button
                  type="submit"
                  disabled={pageBusy || !settingsDirty}
                  className="inline-flex min-h-9 items-center justify-center gap-1.5 rounded-md bg-[#1d7f47] px-3 text-[8.5pt] font-semibold text-white disabled:opacity-60"
                >
                  <Save className="h-3.5 w-3.5" /> Save
                </button>
              </div>
              {saveResult ? <ResultLine result={saveResult} /> : null}
              {testResult ? <ResultLine result={testResult} /> : null}
            </form>
          ) : null}

          {view === "password" && advancedUnlocked ? (
            <form
              onSubmit={(e) => void handlePasswordChange(e)}
              className="rounded-md border border-[#454542] bg-[#292928] p-4"
            >
              <div className="grid gap-3 sm:grid-cols-3">
                <label className="block text-[8pt] font-semibold text-[#d8d2c8]">
                  Current password
                  <input
                    type="password"
                    value={passwordFields.current}
                    disabled={pageBusy}
                    onChange={(e) =>
                      setPasswordFields((c) => ({ ...c, current: e.target.value }))
                    }
                    className={inputClassName}
                  />
                </label>
                <label className="block text-[8pt] font-semibold text-[#d8d2c8]">
                  New password
                  <input
                    type="password"
                    value={passwordFields.next}
                    disabled={pageBusy}
                    onChange={(e) => setPasswordFields((c) => ({ ...c, next: e.target.value }))}
                    className={inputClassName}
                  />
                </label>
                <label className="block text-[8pt] font-semibold text-[#d8d2c8]">
                  Confirm password
                  <input
                    type="password"
                    value={passwordFields.confirm}
                    disabled={pageBusy}
                    onChange={(e) =>
                      setPasswordFields((c) => ({ ...c, confirm: e.target.value }))
                    }
                    className={inputClassName}
                  />
                </label>
              </div>
              <div className="mt-3 flex justify-end">
                <button
                  type="submit"
                  disabled={pageBusy}
                  className="inline-flex min-h-9 items-center justify-center rounded-md bg-[#1f74ae] px-3 text-[8.5pt] font-semibold text-white disabled:opacity-60"
                >
                  Update password
                </button>
              </div>
              {passwordResult ? <ResultLine result={passwordResult} /> : null}
            </form>
          ) : null}
        </ScrollRegion>
      ) : null}

      {leaveTarget ? (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/55 p-4"
          onMouseDown={(event) => {
            if (pageBusy) return;
            if (event.target === event.currentTarget) {
              setLeaveTarget(null);
            }
          }}
        >
          <section
            role="dialog"
            aria-modal="true"
            aria-labelledby="unsaved-settings-title"
            className="w-full max-w-[360px] rounded-md border border-[#454542] bg-[#292928] p-4 text-white shadow-2xl"
            onMouseDown={(event) => event.stopPropagation()}
          >
            <h2
              id="unsaved-settings-title"
              className="text-center text-[12pt] font-semibold leading-tight"
            >
              Unsaved settings
            </h2>
            <div className="mt-4 flex flex-col gap-2">
              <button
                type="button"
                disabled={pageBusy}
                onClick={() => void handleSaveAndLeave()}
                className="inline-flex min-h-9 items-center justify-center gap-1.5 rounded-md bg-[#1d7f47] px-3 text-[9pt] font-semibold text-white disabled:opacity-60"
              >
                {isSaving || isChangingPassword ? (
                  <LoaderCircle className="h-3.5 w-3.5 animate-spin" aria-hidden="true" />
                ) : (
                  <Save className="h-3.5 w-3.5" aria-hidden="true" />
                )}
                Save Changes
              </button>
              <button
                type="button"
                disabled={pageBusy}
                onClick={handleDiscardAndLeave}
                className="inline-flex min-h-9 items-center justify-center rounded-md bg-[#3a3a38] px-3 text-[9pt] font-semibold text-white disabled:opacity-60"
              >
                Discard Changes
              </button>
              <button
                type="button"
                disabled={pageBusy}
                onClick={() => setLeaveTarget(null)}
                className="inline-flex min-h-9 items-center justify-center rounded-md border border-[#454542] bg-transparent px-3 text-[9pt] font-semibold text-[#d8d2c8] disabled:opacity-60"
              >
                Cancel
              </button>
            </div>
          </section>
        </div>
      ) : null}

      <SettingsPasswordModal
        open={advancedPasswordOpen}
        verify={verifyPassword}
        onCancel={() => {
          setAdvancedPasswordOpen(false);
        }}
        onUnlock={(password) => {
          setAdvancedPasswordOpen(false);
          setAdvancedUnlocked(true);
          setAdvancedUnlockPassword(password);
          setView("advanced");
        }}
      />
    </main>
  );
}

function FieldSelect({
  label,
  value,
  options,
  disabled,
  onChange,
}: {
  label: string;
  value: string;
  options: { value: string; label: string }[];
  disabled?: boolean;
  onChange: (value: string) => void;
}) {
  return (
    <label className="block text-[8pt] font-semibold text-[#d8d2c8]">
      {label}
      <span className="relative mt-1 block">
        <select
          value={value}
          disabled={disabled}
          onChange={(event) => onChange(event.target.value)}
          className={selectClassName}
        >
          {options.map((option) => (
            <option key={option.value} value={option.value}>
              {option.label}
            </option>
          ))}
        </select>
        <ChevronDown
          className="pointer-events-none absolute right-2.5 top-1/2 h-4 w-4 -translate-y-1/2 text-[#9a958c]"
          aria-hidden="true"
        />
      </span>
    </label>
  );
}

function MenuButton({
  icon: Icon,
  title,
  onClick,
}: {
  icon: typeof Radio;
  title: string;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="flex w-full items-center gap-3 rounded-md border border-[#454542] bg-[#292928] px-3 py-3 text-left transition hover:border-[#5a5a56] hover:bg-[#30302f]"
    >
      <Icon className="h-4 w-4 shrink-0 text-[#8dc7ef]" />
      <div className="min-w-0 flex-1 text-[10pt] font-semibold">{title}</div>
      <ChevronRight className="h-4 w-4 text-[#777772]" />
    </button>
  );
}

function ResultLine({ result }: { result: ResultMessage }) {
  return (
    <div
      role={result.tone === "error" ? "alert" : "status"}
      className={cn(
        "mt-2 text-[7.5pt]",
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

function normalizeLoaded(settings: AppNotificationSettingsView): AppNotificationSettingsView {
  const stations =
    settings.stations?.length > 0
      ? settings.stations.map((s) => ({ id: s.id, name: s.name }))
      : NOTIFICATION_STATIONS.map((s) => ({ id: s.id, name: s.name }));
  const knownIds = new Set(stations.map((s) => s.id));
  const included =
    settings.summary_included_station_ids?.length > 0
      ? settings.summary_included_station_ids.filter((id) => knownIds.has(id))
      : stations.map((s) => s.id);
  return {
    ...settings,
    shifts: settings.shifts ?? [],
    stations,
    floor_sync: settings.floor_sync ?? {
      configured: Boolean(settings.shared_shift_log_path?.trim()),
      source: settings.shared_shift_log_path?.trim() ? "floor" : "local",
      updated_at: null,
      updated_by_station_id: null,
      message: settings.shared_shift_log_path?.trim()
        ? "Syncing via shared folder."
        : "Shared folder not set — settings stay on this PC only.",
    },
    summary_poster_station_id: settings.summary_poster_station_id || "pdu-lab",
    summary_included_station_ids:
      included.length > 0 ? included : stations.map((s) => s.id),
    is_summary_poster:
      settings.station_id === (settings.summary_poster_station_id || "pdu-lab"),
    station_name: stationNameForId(settings.station_id, stations),
    events: {
      problem: settings.events?.problem ?? true,
      complete: settings.events?.complete ?? true,
      changeover: settings.events?.changeover ?? true,
      stuck: false,
      summary: settings.events?.summary ?? true,
    },
  };
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
    shifts: settings.shifts,
    summary_poster_station_id: settings.summary_poster_station_id,
    summary_included_station_ids: settings.summary_included_station_ids,
    stations: settings.stations,
  });
}

function saveRequestFromSettings(
  settings: AppNotificationSettingsView,
  scope: SettingsSaveScope,
  connectPassword?: string,
): SaveNotificationSettingsRequest {
  const request: SaveNotificationSettingsRequest = {
    enabled: settings.enabled,
    teams_destination_name: settings.teams_destination_name.trim(),
    teams_webhook_url: settings.teams_webhook_url.trim(),
    station_id: settings.station_id,
    station_name: stationNameForId(settings.station_id, settings.stations),
    idle_timeout_minutes: settings.idle_timeout_minutes,
    events: settings.events,
    shared_shift_log_path: settings.shared_shift_log_path.trim(),
    shifts: settings.shifts.map((s) => ({
      label: s.label.trim(),
      start_time: s.start_time.trim(),
      end_time: s.end_time.trim(),
    })),
    summary_poster_station_id: settings.summary_poster_station_id.trim() || "pdu-lab",
    summary_included_station_ids: settings.summary_included_station_ids,
    stations: settings.stations.map((s) => ({
      id: s.id,
      name: s.name.trim(),
    })),
    scope,
  };
  if (connectPassword) {
    request.connect_password = connectPassword;
  }
  return request;
}

function settingsAfterSave(
  current: AppNotificationSettingsView,
  request: SaveNotificationSettingsRequest,
): AppNotificationSettingsView {
  return {
    ...current,
    enabled: request.enabled,
    teams_destination_name: request.teams_destination_name,
    teams_webhook_url: request.teams_webhook_url,
    station_id: request.station_id,
    station_name: request.station_name,
    idle_timeout_minutes: request.idle_timeout_minutes,
    events: request.events,
    shared_shift_log_path: request.shared_shift_log_path,
    shifts: request.shifts,
    summary_poster_station_id: request.summary_poster_station_id,
    summary_included_station_ids: request.summary_included_station_ids,
    stations: request.stations,
    webhook_configured: current.webhook_configured || Boolean(request.teams_webhook_url.trim()),
    is_summary_poster: request.station_id === (request.summary_poster_station_id || "pdu-lab"),
  };
}

function statusAdvanced(
  latest: NotificationRuntimeStatus,
  baseline: NotificationRuntimeStatus | null,
) {
  if (latest.event_kind !== "test_ping" || latest.state === "idle" || latest.state === "ready") {
    return false;
  }
  if (!baseline) return Boolean(latest.updated_at);
  if (latest.updated_at && latest.updated_at !== baseline.updated_at) return true;
  return latest.state !== baseline.state || latest.message !== baseline.message;
}

function messageFromRuntimeStatus(status: NotificationRuntimeStatus): ResultMessage {
  if (status.state === "failed") return { tone: "error", text: status.message };
  if (status.state === "skipped") return { tone: "warning", text: status.message };
  return { tone: "success", text: status.message };
}

function abortableDelay(ms: number, signal: AbortSignal) {
  return new Promise<boolean>((resolve) => {
    if (signal.aborted) {
      resolve(false);
      return;
    }
    const t = window.setTimeout(() => resolve(true), ms);
    signal.addEventListener(
      "abort",
      () => {
        window.clearTimeout(t);
        resolve(false);
      },
      { once: true },
    );
  });
}

function errorMessage(error: unknown, fallback: string) {
  if (error instanceof Error && error.message.trim()) return error.message;
  if (typeof error === "string" && error.trim()) return error;
  return fallback;
}
