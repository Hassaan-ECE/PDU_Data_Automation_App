import { useCallback, useEffect, useRef, useState } from "react";
import { check, type DownloadEvent, type Update } from "@tauri-apps/plugin-updater";

import { isTauriRuntime } from "@/integrations/tauri/backend";
import { markStartup } from "@/shared/lib/startupTiming";

import { buildIdleUpdateState, chooseFreshUpdateState, type UpdateState } from "./updateTypes";

const POST_APP_READY_UPDATE_CHECK_DELAY_MS = 1_500;
const UPDATE_CHECK_INTERVAL_MS = 5 * 60_000;

interface UseDesktopUpdatesOptions {
  announceStatus: (message: string) => void;
  currentVersion: string;
  enabled?: boolean;
}

export function useDesktopUpdates({ announceStatus, currentVersion, enabled = true }: UseDesktopUpdatesOptions) {
  const [updateState, setUpdateState] = useState<UpdateState>(() => buildIdleUpdateState(currentVersion));
  const automaticChecksStartedRef = useRef(false);
  const currentVersionRef = useRef(currentVersion);
  const firstUpdateCheckLoggedRef = useRef(false);
  const pendingUpdateRef = useRef<Update | null>(null);
  const updateStateRef = useRef(updateState);

  useEffect(() => {
    currentVersionRef.current = currentVersion;
    setUpdateState((current) => {
      if (current.currentVersion === currentVersion || current.status !== "idle") {
        return current;
      }

      const next = { ...current, currentVersion };
      updateStateRef.current = next;
      return next;
    });
  }, [currentVersion]);

  useEffect(() => {
    updateStateRef.current = updateState;
  }, [updateState]);

  const publishUpdateState = useCallback((state: UpdateState): UpdateState => {
    updateStateRef.current = state;
    setUpdateState(state);
    return state;
  }, []);

  const publishUpdateCheckResult = useCallback(
    (state: UpdateState): UpdateState => {
      if (!firstUpdateCheckLoggedRef.current) {
        firstUpdateCheckLoggedRef.current = true;
        markStartup("updater_check_finished", {
          error: state.error ?? null,
          latestVersion: state.latestVersion ?? null,
          status: state.status,
        });
      }

      return publishUpdateState(state);
    },
    [publishUpdateState],
  );

  const checkForUpdate = useCallback(async (): Promise<UpdateState> => {
    const version = currentVersionRef.current;

    if (!isTauriRuntime()) {
      return publishUpdateCheckResult(buildIdleUpdateState(version));
    }

    publishUpdateState({
      available: false,
      currentVersion: version,
      status: "checking",
    });

    try {
      const update = await check();
      pendingUpdateRef.current?.close().catch(() => undefined);
      pendingUpdateRef.current = update;

      if (!update) {
        return publishUpdateCheckResult({
          available: false,
          currentVersion: version,
          notes: "PDU Data Automation is up to date.",
          status: "not-available",
        });
      }

      return publishUpdateCheckResult(updateStateFromUpdate(update, "available", version));
    } catch (error) {
      pendingUpdateRef.current = null;
      return publishUpdateCheckResult(errorUpdateState(error, version));
    }
  }, [publishUpdateCheckResult, publishUpdateState]);

  useEffect(() => {
    if (!enabled || !isTauriRuntime() || automaticChecksStartedRef.current) {
      return undefined;
    }

    automaticChecksStartedRef.current = true;
    let active = true;
    const canCheckForUpdate = (): boolean =>
      !["downloading", "ready", "installing"].includes(updateStateRef.current.status);
    const runUpdateCheck = (): void => {
      if (!canCheckForUpdate()) {
        return;
      }

      void checkForUpdate()
        .then((state) => {
          if (active) {
            setUpdateState((current) => chooseFreshUpdateState(current, state));
          }
        })
        .catch(() => {
          if (active) {
            setUpdateState(buildIdleUpdateState(currentVersionRef.current));
          }
        });
    };
    const handleVisibilityChange = (): void => {
      if (document.visibilityState === "visible") {
        runUpdateCheck();
      }
    };

    let intervalId: number | undefined;
    const startUpdateChecks = (): void => {
      if (!active) {
        return;
      }

      runUpdateCheck();
      intervalId = window.setInterval(runUpdateCheck, UPDATE_CHECK_INTERVAL_MS);
      window.addEventListener("focus", runUpdateCheck);
      document.addEventListener("visibilitychange", handleVisibilityChange);
    };
    const startupDelayId = window.setTimeout(startUpdateChecks, POST_APP_READY_UPDATE_CHECK_DELAY_MS);

    return () => {
      active = false;
      window.clearTimeout(startupDelayId);
      if (intervalId !== undefined) {
        window.clearInterval(intervalId);
      }
      window.removeEventListener("focus", runUpdateCheck);
      document.removeEventListener("visibilitychange", handleVisibilityChange);
    };
  }, [checkForUpdate, enabled]);

  const handleUpdateAction = useCallback(async (): Promise<void> => {
    if (!isTauriRuntime()) {
      return;
    }

    const version = currentVersionRef.current;

    try {
      if (updateState.status === "ready") {
        const nextState = await installUpdate(pendingUpdateRef.current, version);
        publishUpdateState(nextState);
        if (nextState.status === "error" && nextState.error) {
          announceStatus(nextState.error);
        }
        return;
      }

      if (updateState.status === "downloading" || updateState.status === "checking" || updateState.status === "installing") {
        return;
      }

      if (updateState.available) {
        publishUpdateState({ ...updateState, status: "downloading" });
      }

      let update = pendingUpdateRef.current;
      if (!update) {
        const state = await checkForUpdate();
        if (!pendingUpdateRef.current || !state.available) {
          return;
        }
        update = pendingUpdateRef.current;
      }

      const nextState = await downloadUpdate(update, version, publishUpdateState);
      publishUpdateState(chooseFreshUpdateState(updateStateRef.current, nextState));
      if (nextState.status === "error" && nextState.error) {
        announceStatus(nextState.error);
      }
    } catch (error) {
      const message = error instanceof Error ? error.message : "Update failed.";
      publishUpdateState({ ...updateState, currentVersion: version, error: message, status: "error" });
      announceStatus(message);
    }
  }, [announceStatus, checkForUpdate, publishUpdateState, updateState]);

  return { handleUpdateAction, updateState };
}

async function downloadUpdate(
  update: Update,
  currentVersion: string,
  publishUpdateState: (state: UpdateState) => UpdateState,
): Promise<UpdateState> {
  let totalBytes: number | undefined;
  let downloadedBytes = 0;

  publishUpdateState(updateStateFromUpdate(update, "downloading", currentVersion, {
    downloadPhase: "copying",
    downloadProgress: 0,
  }));

  try {
    await update.download((event) => {
      publishUpdateState(updateDownloadState(update, event, currentVersion, totalBytes, downloadedBytes));

      if (event.event === "Started") {
        totalBytes = event.data.contentLength;
        downloadedBytes = 0;
      } else if (event.event === "Progress") {
        downloadedBytes += event.data.chunkLength;
      }
    });

    return updateStateFromUpdate(update, "ready", currentVersion, {
      downloadPhase: "ready",
      downloadProgress: 100,
    });
  } catch (error) {
    return errorUpdateState(error, currentVersion, update);
  }
}

async function installUpdate(update: Update | null, currentVersion: string): Promise<UpdateState> {
  if (!update) {
    return {
      available: false,
      currentVersion,
      error: "Download the update before installing it.",
      status: "error",
    };
  }

  try {
    await update.install();
    return updateStateFromUpdate(update, "installing", currentVersion);
  } catch (error) {
    return errorUpdateState(error, currentVersion, update);
  }
}

function updateStateFromUpdate(
  update: Update,
  status: UpdateState["status"],
  currentVersion: string,
  overrides: Partial<UpdateState> = {},
): UpdateState {
  return {
    available: true,
    currentVersion: update.currentVersion || currentVersion,
    latestVersion: update.version,
    notes: update.body,
    publishedAt: update.date,
    status,
    ...overrides,
  };
}

function updateDownloadState(
  update: Update,
  event: DownloadEvent,
  currentVersion: string,
  previousTotalBytes: number | undefined,
  previousDownloadedBytes: number,
): UpdateState {
  if (event.event === "Started") {
    return updateStateFromUpdate(update, "downloading", currentVersion, {
      downloadPhase: "copying",
      downloadProgress: event.data.contentLength ? 0 : undefined,
    });
  }

  if (event.event === "Finished") {
    return updateStateFromUpdate(update, "downloading", currentVersion, {
      downloadPhase: "verifying",
      downloadProgress: 100,
    });
  }

  const nextDownloadedBytes = previousDownloadedBytes + event.data.chunkLength;
  const downloadProgress =
    previousTotalBytes && previousTotalBytes > 0
      ? Math.min(99, Math.round((nextDownloadedBytes / previousTotalBytes) * 100))
      : undefined;

  return updateStateFromUpdate(update, "downloading", currentVersion, {
    downloadPhase: "copying",
    downloadProgress,
  });
}

function errorUpdateState(error: unknown, currentVersion: string, update?: Update): UpdateState {
  const message = error instanceof Error ? error.message : "Update failed.";
  if (update) {
    return updateStateFromUpdate(update, "error", currentVersion, { error: message });
  }

  return {
    available: false,
    currentVersion,
    error: message,
    status: "error",
  };
}
