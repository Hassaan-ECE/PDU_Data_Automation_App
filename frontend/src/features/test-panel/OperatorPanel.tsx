import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { ChevronDown, Save, Settings as SettingsIcon, Trash2 } from "lucide-react";

import {
  changeSettingsPassword,
  chooseSharedNotificationsFolder,
  chooseUnitFolder,
  closeReportWorkbook,
  getAppNotificationSettings,
  getBackendStatus,
  getNotificationStatus,
  loadLayoutProfile,
  openPrintReportDialog,
  openReportLocation,
  openReportPath,
  postShiftSummary,
  previewShiftSummary,
  saveFinalOperatorName,
  saveAppNotificationSettings,
  saveTransformerSn,
  scanUnitFolder,
  setupUnitFolder,
  sendNotificationTest,
  validateReadyForPrint,
  verifySettingsPassword,
  type BackendStatus,
  type LayoutLoadResponse,
  type PrintReadinessBlocker,
  type TaskProcessResult,
  type UnitFolderSummary,
} from "@/integrations/tauri/backend";
import { NotificationSettingsPage } from "@/features/settings/NotificationSettingsPage";
import { markStartup } from "@/shared/lib/startupTiming";
import { cn } from "@/shared/lib/utils";

import {
  addOperatorName,
  loadOperatorNames,
  matchingOperatorNames,
  operatorNameKey,
  storeOperatorNames,
} from "./operatorNames";
import {
  applyTaskStates,
  backendTaskStatusMap,
  detectedReadyMessage,
  detectedTaskCountFromStates,
  detailedMessageFromUnknownError,
  failureNoticeFromResult,
  findTaskPath,
  flattenTasks,
  formatTime,
  isWorkbookLockedError,
  isTerminalState,
  messageFromUnknownError,
  panelControlState,
  printReadinessMessage,
  remainingSecondsForTasks,
  resetButtonLabel,
  shouldRunnerContinueAfterResult,
  serialNumberFromFolder,
} from "./panelLogic";
import { legacyPanelItems } from "./taskModel";
import type {
  BacklogPromptState,
  BackendTaskStatusMap,
  TaskFailureNotice,
  TaskState,
  TransformerSnSaveStatus,
} from "./types";
import { UpdateActionButton } from "./UpdateActionButton";
import { useDesktopUpdates } from "./useDesktopUpdates";
import { WorkflowSteps } from "./WorkflowSteps";
import { useTaskRunner } from "./useTaskRunner";

const FLOOR_SIMULATION_ENABLED = import.meta.env.VITE_PDU_SIMULATION_MODE === "true";

type WorkbookClosePromptState = {
  path: string;
  message: string;
  resolve: (closed: boolean) => void;
} | null;

export function OperatorPanel() {
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const currentTaskIdRef = useRef<string | null>(null);
  const programmaticScrollRef = useRef(false);
  const programmaticScrollTimeoutRef = useRef<number | null>(null);
  const lastFollowScrolledTaskRef = useRef<string | null>(null);
  const processingTaskRef = useRef(false);
  const isRunningRef = useRef(false);
  const stopAfterCurrentTaskRef = useRef(false);
  const taskStatesRef = useRef<Record<string, TaskState>>({});
  const latestTaskStatusesRef = useRef<BackendTaskStatusMap>({});
  const processDetectedBacklogRef = useRef<boolean | null>(null);
  const selectedFolderRef = useRef("");
  const folderScanPendingRef = useRef(false);
  const setupPromiseRef = useRef<Promise<boolean> | null>(null);
  const setupErrorRef = useRef<string | null>(null);
  const setupConfirmedFolderRef = useRef("");
  const savedTransformerSnRef = useRef("");
  const transformerSnDraftRef = useRef("");
  const detectedCountRef = useRef(0);
  const floorSimulationSetupStartedRef = useRef(false);
  const floorSimulationAutoStartedRef = useRef(false);
  const heldFailureTaskIdRef = useRef<string | null>(null);
  const allTaskOrder = useMemo(() => flattenTasks(legacyPanelItems), []);
  const [unitFolder, setUnitFolder] = useState("");
  const [serialNumber, setSerialNumber] = useState("");
  const [nowMs, setNowMs] = useState(() => Date.now());
  const [isRunning, setIsRunning] = useState(false);
  const [currentTaskId, setCurrentTaskId] = useState<string | null>(null);
  const [expandedIds, setExpandedIds] = useState<Set<string>>(() => new Set());
  const [backendStatus, setBackendStatus] = useState<BackendStatus | null>(null);
  const [backendStatusLoaded, setBackendStatusLoaded] = useState(false);
  const [layoutProfile, setLayoutProfile] = useState<LayoutLoadResponse | null>(null);
  const [layoutProfileError, setLayoutProfileError] = useState("");
  const [layoutProfileLoaded, setLayoutProfileLoaded] = useState(false);
  const [scrollCue, setScrollCue] = useState({ top: false, bottom: false });
  const [currentStepFollowMode, setCurrentStepFollowMode] = useState(false);
  const [taskStates, setTaskStates] = useState<Record<string, TaskState>>({});
  const [latestTaskStatuses, setLatestTaskStatuses] = useState<BackendTaskStatusMap>({});
  const [failureNotices, setFailureNotices] = useState<Record<string, TaskFailureNotice>>({});
  const [processDetectedBacklog, setProcessDetectedBacklog] = useState<boolean | null>(null);
  const [reportPath, setReportPath] = useState("");
  const [printReportPath, setPrintReportPath] = useState("");
  const [detectedCount, setDetectedCount] = useState(0);
  const [lastMessage, setLastMessage] = useState("");
  const [setupWarnings, setSetupWarnings] = useState<string[]>([]);
  const [isScanningFolder, setIsScanningFolder] = useState(false);
  const [isSettingUpReports, setIsSettingUpReports] = useState(false);
  const [isChoosingFolder, setIsChoosingFolder] = useState(false);
  const [backlogPrompt, setBacklogPrompt] = useState<BacklogPromptState>(null);
  const [workbookClosePrompt, setWorkbookClosePrompt] = useState<WorkbookClosePromptState>(null);
  const [workbookCloseError, setWorkbookCloseError] = useState("");
  const [isClosingReportWorkbook, setIsClosingReportWorkbook] = useState(false);
  const [transformerSnDraft, setTransformerSnDraft] = useState("");
  const [savedTransformerSn, setSavedTransformerSn] = useState("");
  const [transformerSnSaveStatus, setTransformerSnSaveStatus] = useState<TransformerSnSaveStatus>("idle");
  const [transformerSnError, setTransformerSnError] = useState("");
  const [transformerSnInputFocused, setTransformerSnInputFocused] = useState(false);
  const [setupConfirmedFolder, setSetupConfirmedFolder] = useState("");
  const [resetClearsSelectionNext, setResetClearsSelectionNext] = useState(false);
  const [operatorNames, setOperatorNames] = useState<string[]>(loadOperatorNames);
  const [printOperatorPromptOpen, setPrintOperatorPromptOpen] = useState(false);
  const [operatorNameDraft, setOperatorNameDraft] = useState("");
  const [operatorNameError, setOperatorNameError] = useState("");
  const [printReadinessBlockers, setPrintReadinessBlockers] = useState<PrintReadinessBlocker[]>([]);
  const [operatorDropdownOpen, setOperatorDropdownOpen] = useState(false);
  const [operatorFilterText, setOperatorFilterText] = useState("");
  const [isOpeningPrintDialog, setIsOpeningPrintDialog] = useState(false);
  const [panelView, setPanelView] = useState<"operator" | "notification-settings">("operator");
  const appVersion = backendStatus?.version ?? "0.2.9";

  const panelItems = useMemo(() => applyTaskStates(legacyPanelItems, taskStates), [taskStates]);
  const detectedTaskCount = useMemo(
    () => detectedTaskCountFromStates(taskStates),
    [taskStates],
  );
  const remainingSeconds = useMemo(
    () =>
      unitFolder
        ? remainingSecondsForTasks(
            allTaskOrder,
            taskStates,
            latestTaskStatuses,
            currentTaskId,
            nowMs,
          )
        : 0,
    [allTaskOrder, currentTaskId, latestTaskStatuses, nowMs, taskStates, unitFolder],
  );
  const announceUpdateStatus = useCallback((message: string) => setLastMessage(message), []);
  const { handleUpdateAction, updateState } = useDesktopUpdates({
    announceStatus: announceUpdateStatus,
    currentVersion: appVersion,
    enabled: backendStatusLoaded && layoutProfileLoaded,
  });
  const setConfirmedSetupFolder = useCallback((folder: string) => {
    setupConfirmedFolderRef.current = folder;
    setSetupConfirmedFolder(folder);
  }, []);

  const statusText = useMemo(() => {
    if (!unitFolder) {
      return "No unit folder selected";
    }

    if (isRunning) {
      return lastMessage || "Sequence running";
    }

    return lastMessage || "Ready to start";
  }, [isRunning, lastMessage, unitFolder]);

  useEffect(() => {
    markStartup("react_mounted");
    void getBackendStatus()
      .then((status) => {
        setBackendStatus(status);
        markStartup(
          "backend_status_loaded",
          status
            ? {
                process_uptime_ms: status.process_uptime_ms ?? null,
                version: status.version,
                window_setup_uptime_ms: status.window_setup_uptime_ms ?? null,
              }
            : null,
        );
      })
      .catch((error) => {
        markStartup("backend_status_loaded", {
          error: messageFromUnknownError(error),
        });
      })
      .finally(() => setBackendStatusLoaded(true));
    void loadLayoutProfile()
      .then((profile) => {
        setLayoutProfile(profile);
        setLayoutProfileError("");
        markStartup(
          "layout_profile_loaded",
          profile
            ? {
                errors: profile.validation.errors.length,
                profile_id: profile.profile_id,
                task_count: profile.task_count,
                warnings: profile.validation.warnings.length,
              }
            : null,
        );
      })
      .catch((error) => {
        const message = messageFromUnknownError(error);

        setLayoutProfile(null);
        setLayoutProfileError(message);
        markStartup("layout_profile_loaded", {
          error: message,
        });
      })
      .finally(() => setLayoutProfileLoaded(true));
  }, []);

  useEffect(() => {
    taskStatesRef.current = taskStates;
  }, [taskStates]);

  useEffect(() => {
    processDetectedBacklogRef.current = processDetectedBacklog;
  }, [processDetectedBacklog]);

  useEffect(() => {
    isRunningRef.current = isRunning;
  }, [isRunning]);

  useEffect(() => {
    selectedFolderRef.current = unitFolder;
  }, [unitFolder]);

  useEffect(() => {
    savedTransformerSnRef.current = savedTransformerSn;
  }, [savedTransformerSn]);

  useEffect(() => {
    transformerSnDraftRef.current = transformerSnDraft;
  }, [transformerSnDraft]);

  const setRunnerActive = useCallback((active: boolean) => {
    isRunningRef.current = active;
    setIsRunning(active);
  }, []);

  const requestBacklogChoice = useCallback((count: number) => {
    return new Promise<boolean | null>((resolve) => {
      setBacklogPrompt({ count, resolve });
    });
  }, []);

  const resolveBacklogPrompt = useCallback((processBacklog: boolean | null) => {
    setBacklogPrompt((current) => {
      current?.resolve(processBacklog);
      return null;
    });
  }, []);

  const requestWorkbookClose = useCallback((path: string, message: string) => {
    return new Promise<boolean>((resolve) => {
      setWorkbookCloseError("");
      setWorkbookClosePrompt({ path, message, resolve });
    });
  }, []);

  const runPanelReportWriteWithRetry = useCallback(
    async <Result,>(path: string, operation: () => Promise<Result>): Promise<Result> => {
      try {
        return await operation();
      } catch (error) {
        if (!isWorkbookLockedError(error)) {
          throw error;
        }

        const shouldRetry = await requestWorkbookClose(path, messageFromUnknownError(error));
        if (!shouldRetry) {
          throw error;
        }

        return operation();
      }
    },
    [requestWorkbookClose],
  );

  const cancelWorkbookClosePrompt = useCallback(() => {
    if (isClosingReportWorkbook) {
      return;
    }

    setWorkbookCloseError("");
    setWorkbookClosePrompt((current) => {
      current?.resolve(false);
      return null;
    });
  }, [isClosingReportWorkbook]);

  const handleCloseReportWorkbook = useCallback(async () => {
    if (!workbookClosePrompt || !unitFolder || isClosingReportWorkbook) {
      return;
    }

    setIsClosingReportWorkbook(true);
    setWorkbookCloseError("");

    try {
      const result = await closeReportWorkbook(unitFolder, workbookClosePrompt.path);
      setLastMessage(`${result.message} Retrying report processing.`);
      setWorkbookClosePrompt((current) => {
        current?.resolve(true);
        return null;
      });
    } catch (error) {
      const message = detailedMessageFromUnknownError(error);
      setWorkbookCloseError(message);
      setLastMessage(message);
    } finally {
      setIsClosingReportWorkbook(false);
    }
  }, [isClosingReportWorkbook, unitFolder, workbookClosePrompt]);

  useEffect(() => {
    if (!isRunning) {
      return;
    }

    const handle = window.setInterval(() => setNowMs(Date.now()), 1000);

    return () => window.clearInterval(handle);
  }, [isRunning]);

  const updateScrollCue = useCallback(() => {
    const element = scrollRef.current;

    if (!element) {
      setScrollCue({ top: false, bottom: false });
      return;
    }

    const overflow = element.scrollHeight > element.clientHeight + 1;
    const atTop = element.scrollTop <= 1;
    const atBottom = element.scrollTop + element.clientHeight >= element.scrollHeight - 1;
    const nextCue = {
      top: overflow && !atTop,
      bottom: overflow && !atBottom,
    };

    setScrollCue((current) =>
      current.top === nextCue.top && current.bottom === nextCue.bottom ? current : nextCue,
    );
  }, []);

  const scrollCurrentTaskIntoView = useCallback((behavior: ScrollBehavior = "smooth") => {
    const currentElement = scrollRef.current?.querySelector('[data-current-task="true"]');

    if (
      !currentElement ||
      !("scrollIntoView" in currentElement) ||
      typeof currentElement.scrollIntoView !== "function"
    ) {
      return false;
    }

    programmaticScrollRef.current = true;
    if (programmaticScrollTimeoutRef.current !== null) {
      window.clearTimeout(programmaticScrollTimeoutRef.current);
    }

    currentElement.scrollIntoView({
      behavior,
      block: "center",
    });

    programmaticScrollTimeoutRef.current = window.setTimeout(() => {
      programmaticScrollRef.current = false;
      programmaticScrollTimeoutRef.current = null;
    }, 700);

    window.requestAnimationFrame(updateScrollCue);
    return true;
  }, [updateScrollCue]);

  const enableCurrentStepFollow = useCallback((behavior: ScrollBehavior = "smooth") => {
    lastFollowScrolledTaskRef.current = currentTaskIdRef.current;
    setCurrentStepFollowMode(true);
    window.requestAnimationFrame(() => {
      scrollCurrentTaskIntoView(behavior);
    });
  }, [scrollCurrentTaskIntoView]);

  const disableCurrentStepFollowForUserAction = useCallback((force = false) => {
    if (!currentTaskIdRef.current || (!force && programmaticScrollRef.current)) {
      return;
    }

    if (force && programmaticScrollTimeoutRef.current !== null) {
      window.clearTimeout(programmaticScrollTimeoutRef.current);
      programmaticScrollTimeoutRef.current = null;
    }

    if (force) {
      programmaticScrollRef.current = false;
    }

    setCurrentStepFollowMode(false);
  }, []);

  const handleWorkflowUserScrollIntent = useCallback(() => {
    disableCurrentStepFollowForUserAction(true);
  }, [disableCurrentStepFollowForUserAction]);

  const handleWorkflowScroll = useCallback(() => {
    updateScrollCue();
    disableCurrentStepFollowForUserAction();
  }, [disableCurrentStepFollowForUserAction, updateScrollCue]);

  useEffect(() => {
    return () => {
      if (programmaticScrollTimeoutRef.current !== null) {
        window.clearTimeout(programmaticScrollTimeoutRef.current);
      }
    };
  }, []);

  useEffect(() => {
    if (!currentStepFollowMode || !currentTaskId) {
      return;
    }

    if (lastFollowScrolledTaskRef.current === currentTaskId) {
      return;
    }

    lastFollowScrolledTaskRef.current = currentTaskId;
    window.requestAnimationFrame(() => {
      scrollCurrentTaskIntoView();
    });
  }, [currentStepFollowMode, currentTaskId, scrollCurrentTaskIntoView]);

  useEffect(() => {
    const element = scrollRef.current;

    if (!element) {
      return;
    }

    updateScrollCue();
    window.addEventListener("resize", updateScrollCue);

    if (typeof ResizeObserver === "undefined") {
      return () => window.removeEventListener("resize", updateScrollCue);
    }

    const resizeObserver = new ResizeObserver(updateScrollCue);
    resizeObserver.observe(element);

    if (element.firstElementChild) {
      resizeObserver.observe(element.firstElementChild);
    }

    return () => {
      resizeObserver.disconnect();
      window.removeEventListener("resize", updateScrollCue);
    };
  }, [expandedIds, panelItems, updateScrollCue]);

  const expandForTask = useCallback((taskId: string) => {
    const sectionPath = findTaskPath(legacyPanelItems, taskId);

    if (!sectionPath?.length) {
      return;
    }

    setExpandedIds((current) => {
      const next = new Set(current);

      for (const sectionId of sectionPath) {
        next.add(sectionId);
      }

      return next;
    });
  }, []);

  const activateTask = useCallback(
    (taskId: string | null) => {
      currentTaskIdRef.current = taskId;
      setCurrentTaskId(taskId);

      if (!taskId) {
        lastFollowScrolledTaskRef.current = null;
        setCurrentStepFollowMode(false);
      }

      if (taskId) {
        expandForTask(taskId);
      }
    },
    [expandForTask],
  );

  const focusTaskForAttention = useCallback(
    (taskId: string) => {
      activateTask(taskId);
      lastFollowScrolledTaskRef.current = taskId;
      setCurrentStepFollowMode(false);
      window.requestAnimationFrame(() => {
        scrollCurrentTaskIntoView("smooth");
      });
    },
    [activateTask, scrollCurrentTaskIntoView],
  );

  const replaceTaskStates = useCallback(
    (states: Record<string, TaskState>) => {
      taskStatesRef.current = states;
      setTaskStates(states);
    },
    [],
  );

  const updateTaskState = useCallback(
    (taskId: string, state: TaskState) => {
      setResetClearsSelectionNext(false);
      replaceTaskStates({
        ...taskStatesRef.current,
        [taskId]: state,
      });
    },
    [replaceTaskStates],
  );

  const applyFolderSummary = useCallback((summary: UnitFolderSummary, replace = false) => {
    setSerialNumber(summary.serial_number ?? "");
    setReportPath(summary.report_path ?? "");
    setPrintReportPath(summary.print_report_path ?? "");
    detectedCountRef.current = summary.detected_count;
    setDetectedCount(summary.detected_count);
    setSetupWarnings(summary.warnings);
    const taskStatuses = backendTaskStatusMap(summary.tasks);
    latestTaskStatusesRef.current = taskStatuses;
    setLatestTaskStatuses(taskStatuses);
    setNowMs(Date.now());

    if (replace) {
      heldFailureTaskIdRef.current = null;
      setFailureNotices({});
    }

    if (summary.tasks.length === 0) {
      return;
    }

    const current = taskStatesRef.current;
    const next = replace ? {} : { ...current };

    for (const task of summary.tasks) {
      const currentState = current[task.task_id];

      if (!replace && (isTerminalState(currentState) || currentState === "processing")) {
        continue;
      }

      next[task.task_id] = task.state;
    }

    replaceTaskStates(next);
  }, [replaceTaskStates]);

  const beginReportSetup = useCallback(
    (selected: string, transformerSn = "", unitSerialNumber = "", replace = false) => {
      setupErrorRef.current = null;
      setIsSettingUpReports(true);
      const trimmedTransformerSn = transformerSn.trim();

      const setupPromise = runPanelReportWriteWithRetry("", () =>
        setupUnitFolder(selected, trimmedTransformerSn, unitSerialNumber),
      )
        .then((summary) => {
          if (selectedFolderRef.current !== selected) {
            return true;
          }

          setConfirmedSetupFolder(selected);
          if (trimmedTransformerSn) {
            savedTransformerSnRef.current = trimmedTransformerSn;
            setSavedTransformerSn(trimmedTransformerSn);
            setTransformerSnSaveStatus("saved");
            setTransformerSnError("");
          } else {
            savedTransformerSnRef.current = "";
            setSavedTransformerSn("");
            setTransformerSnSaveStatus(transformerSnDraftRef.current.trim() ? "dirty" : "idle");
            setTransformerSnError("");
          }

          if (!summary) {
            const folderName = selected.split(/[\\/]/).filter(Boolean).at(-1) ?? "";

            setSerialNumber(folderName.match(/\d{6,}/)?.[0] ?? "");
            setReportPath("");
            setPrintReportPath("");
            setLastMessage("Ready to start");
            return true;
          }

          applyFolderSummary(summary, replace);
          setLastMessage(detectedReadyMessage(summary.detected_count));
          return true;
        })
        .catch((error) => {
          const message = messageFromUnknownError(error);

          if (selectedFolderRef.current === selected) {
            setConfirmedSetupFolder("");
            setupErrorRef.current = message;
            if (trimmedTransformerSn) {
              setTransformerSnSaveStatus("error");
              setTransformerSnError(message);
            }
            setLastMessage(message);
          }

          return false;
        });

      setupPromiseRef.current = setupPromise;
      void setupPromise.finally(() => {
        if (setupPromiseRef.current === setupPromise) {
          setupPromiseRef.current = null;
          setIsSettingUpReports(false);
        }
      });

      return setupPromise;
    },
    [applyFolderSummary, runPanelReportWriteWithRetry, setConfirmedSetupFolder],
  );

  const updateTransformerSnDraft = useCallback((value: string) => {
    transformerSnDraftRef.current = value;
    setTransformerSnDraft(value);
    setTransformerSnError("");

    const trimmed = value.trim();
    if (!trimmed) {
      setTransformerSnSaveStatus("idle");
    } else if (
      trimmed === savedTransformerSnRef.current &&
      setupConfirmedFolderRef.current === selectedFolderRef.current
    ) {
      setTransformerSnSaveStatus("saved");
    } else {
      setTransformerSnSaveStatus("dirty");
    }
  }, []);

  const resetTransformerSnState = useCallback(() => {
    savedTransformerSnRef.current = "";
    transformerSnDraftRef.current = "";
    setTransformerSnDraft("");
    setSavedTransformerSn("");
    setTransformerSnSaveStatus("idle");
    setTransformerSnError("");
  }, []);

  const saveTransformerSnDraft = useCallback(async () => {
    const selected = selectedFolderRef.current;
    const transformerSn = transformerSnDraft.trim();

    if (!transformerSn) {
      setTransformerSnSaveStatus("idle");
      setTransformerSnError("");
      return true;
    }

    if (!selected) {
      const message = "Select Test Unit before saving Transformer SN.";
      setTransformerSnSaveStatus("error");
      setTransformerSnError(message);
      setLastMessage(message);
      return false;
    }

    if (setupConfirmedFolderRef.current !== selected) {
      const setupPromise = setupPromiseRef.current;

      if (!setupPromise) {
        const message = setupErrorRef.current ?? "Report setup is not ready yet.";
        setTransformerSnSaveStatus("error");
        setTransformerSnError(message);
        setLastMessage(message);
        return false;
      }

      setTransformerSnSaveStatus("saving");
      setTransformerSnError("");
      setLastMessage("Finishing report setup");

      const setupOk = await setupPromise;

      if (selectedFolderRef.current !== selected) {
        return false;
      }

      if (!setupOk || setupErrorRef.current || setupConfirmedFolderRef.current !== selected) {
        const message = setupErrorRef.current ?? "Report setup is not ready yet.";
        setTransformerSnSaveStatus("error");
        setTransformerSnError(message);
        setLastMessage(message);
        return false;
      }
    }

    if (transformerSn === savedTransformerSnRef.current) {
      setTransformerSnSaveStatus("saved");
      setTransformerSnError("");
      return true;
    }

    setTransformerSnSaveStatus("saving");
    setTransformerSnError("");

    try {
      await runPanelReportWriteWithRetry(reportPath, () =>
        saveTransformerSn(selected, transformerSn),
      );
      savedTransformerSnRef.current = transformerSn;
      setSavedTransformerSn(transformerSn);
      setTransformerSnSaveStatus("saved");
      setLastMessage("Transformer SN saved");
      return true;
    } catch (error) {
      const message = messageFromUnknownError(error);
      setTransformerSnSaveStatus("error");
      setTransformerSnError(message);
      setLastMessage(message);
      return false;
    }
  }, [reportPath, runPanelReportWriteWithRetry, transformerSnDraft]);

  const ensureReportSetupReady = useCallback(async (expectedFolder?: string) => {
    if (expectedFolder && selectedFolderRef.current !== expectedFolder) {
      return false;
    }

    if (folderScanPendingRef.current) {
      setLastMessage("Scanning unit folder");
      return false;
    }

    const setupPromise = setupPromiseRef.current;

    if (!setupPromise) {
      if (setupErrorRef.current) {
        setLastMessage(setupErrorRef.current);
        return false;
      }

      if (expectedFolder && setupConfirmedFolderRef.current !== expectedFolder) {
        setLastMessage("Report setup is not ready yet.");
        return false;
      }

      return true;
    }

    setLastMessage("Finishing report setup");
    const setupOk = await setupPromise;

    if (expectedFolder && selectedFolderRef.current !== expectedFolder) {
      return false;
    }

    if (!setupOk && setupErrorRef.current) {
      setLastMessage(setupErrorRef.current);
    }

    return setupOk && !setupErrorRef.current && (!expectedFolder || setupConfirmedFolderRef.current === expectedFolder);
  }, []);

  const sequenceCompleteMessage = useCallback(() => {
    const transformerSn = transformerSnDraftRef.current.trim();

    if (!transformerSn) {
      return "Sequence complete. Transformer SN is missing before final printing.";
    }

    if (transformerSn !== savedTransformerSnRef.current) {
      return "Sequence complete. Transformer SN is unsaved before final printing.";
    }

    return "Sequence complete";
  }, []);

  const applyTaskProcessResult = useCallback(
    (result: TaskProcessResult, fromRunner: boolean, focusFailure = true): TaskState => {
      const taskId = result.task_id;
      const continueRunner = shouldRunnerContinueAfterResult(result, fromRunner);

      updateTaskState(taskId, result.state);
      setLastMessage(result.message);

      if (result.report_path) {
        setReportPath(result.report_path);
      }

      if (result.print_report_path) {
        setPrintReportPath(result.print_report_path);
      }

      if (result.state !== "pass" && result.state !== "waiting") {
        if (focusFailure) {
          if (!fromRunner || !heldFailureTaskIdRef.current) {
            heldFailureTaskIdRef.current = taskId;
            focusTaskForAttention(taskId);
          }
        }
        setFailureNotices((current) => ({
          ...current,
          [taskId]: failureNoticeFromResult(taskId, result, fromRunner),
        }));
        if (!continueRunner) {
          setRunnerActive(false);
        }
      }

      return result.state;
    },
    [focusTaskForAttention, setRunnerActive, updateTaskState],
  );

  const { runTask, startSequence, handlePassTask } = useTaskRunner({
    unitFolder,
    reportPath,
    allTaskOrder,
    failureNotices,
    isRunning,
    refs: {
      taskStatesRef,
      latestTaskStatusesRef,
      processDetectedBacklogRef,
      stopAfterCurrentTaskRef,
      processingTaskRef,
      isRunningRef,
      detectedCountRef,
      heldFailureTaskIdRef,
    },
    actions: {
      activateTask,
      updateTaskState,
      setRunnerActive,
      applyTaskProcessResult,
      applyFolderSummary,
      setFailureNotices,
      setLastMessage,
      setProcessDetectedBacklog,
      setResetClearsSelectionNext,
      sequenceCompleteMessage,
      requestBacklogChoice,
      ensureReportSetupReady,
      enableCurrentStepFollow,
      focusTaskForAttention,
      requestWorkbookClose,
    },
  });

  const handleOpenFailureLocation = useCallback(
    async (notice: TaskFailureNotice) => {
      if (processingTaskRef.current) {
        setLastMessage("Wait for the current step to finish before opening the report");
        return;
      }

      if (!unitFolder) {
        setLastMessage("No unit folder is selected");
        return;
      }

      if (setupConfirmedFolderRef.current !== unitFolder) {
        setLastMessage("Report setup must finish before opening the report.");
        return;
      }

      try {
        if (notice.location) {
          await openReportLocation(
            unitFolder,
            notice.location.workbook_path,
            notice.location.sheet,
            notice.location.cell,
          );
          setLastMessage(`Opened ${notice.location.sheet}!${notice.location.cell}`);
          return;
        }

        if (notice.reportPath) {
          await openReportPath(unitFolder, notice.reportPath);
          setLastMessage("Opened report");
          return;
        }

        setLastMessage("No report location is available for this error");
      } catch (error) {
        setLastMessage(messageFromUnknownError(error));
      }
    },
    [unitFolder],
  );

  const handleChooseFolder = useCallback(async () => {
    setIsChoosingFolder(true);
    const selected = await chooseUnitFolder().finally(() => setIsChoosingFolder(false));

    if (!selected) {
      return;
    }

    heldFailureTaskIdRef.current = null;
    setUnitFolder(selected);
    setSerialNumber(serialNumberFromFolder(selected));
    setRunnerActive(false);
    stopAfterCurrentTaskRef.current = false;
    setResetClearsSelectionNext(false);
    setReportPath("");
    setPrintReportPath("");
    setProcessDetectedBacklog(null);
    processDetectedBacklogRef.current = null;
    latestTaskStatusesRef.current = {};
    setLatestTaskStatuses({});
    setFailureNotices({});
    setPrintReadinessBlockers([]);
    resetTransformerSnState();
    activateTask(null);
    selectedFolderRef.current = selected;
    setupPromiseRef.current = null;
    setupErrorRef.current = null;
    setConfirmedSetupFolder("");
    folderScanPendingRef.current = false;
    setIsScanningFolder(false);
    setLastMessage("Setting up reports");

    const setupOk = await beginReportSetup(selected, "", serialNumberFromFolder(selected), true);

    if (!setupOk && selectedFolderRef.current === selected) {
      setTaskStates({});
      setFailureNotices({});
    }
  }, [
    activateTask,
    beginReportSetup,
    resetTransformerSnState,
    setConfirmedSetupFolder,
    setRunnerActive,
  ]);

  useEffect(() => {
    if (!FLOOR_SIMULATION_ENABLED || floorSimulationSetupStartedRef.current) {
      return;
    }

    floorSimulationSetupStartedRef.current = true;
    void handleChooseFolder();
  }, [handleChooseFolder]);

  useEffect(() => {
    if (
      !FLOOR_SIMULATION_ENABLED ||
      floorSimulationAutoStartedRef.current ||
      !unitFolder ||
      setupConfirmedFolder !== unitFolder
    ) {
      return;
    }

    floorSimulationAutoStartedRef.current = true;
    void startSequence(unitFolder);
  }, [setupConfirmedFolder, startSequence, unitFolder]);

  async function handleRunClick() {
    if (controlState.primaryAction === "current-step") {
      handleJumpToCurrentStep();
      return;
    }

    if (controlState.primaryAction === "pause") {
      if (processingTaskRef.current) {
        stopAfterCurrentTaskRef.current = true;
        setLastMessage("Pausing after current step");
      } else {
        stopAfterCurrentTaskRef.current = false;
        setRunnerActive(false);
        setLastMessage("Paused");
      }

      return;
    }

    if (!unitFolder) {
      setLastMessage("Select Test Unit before starting.");
      return;
    }

    if (!(await ensureReportSetupReady(unitFolder))) {
      return;
    }

    if (transformerSnDraft.trim() && transformerSnDraft.trim() !== savedTransformerSnRef.current) {
      const saved = await saveTransformerSnDraft();

      if (!saved) {
        return;
      }
    }

    await startSequence(unitFolder);
  }

  function handleSecondaryControlClick() {
    if (controlState.secondaryDisabled) {
      return;
    }

    if (controlState.secondaryAction === "follow-step") {
      handleJumpToCurrentStep();
      return;
    }

    void handleResetPanel();
  }

  function handleJumpToCurrentStep() {
    const taskId = currentTaskIdRef.current;

    if (!taskId) {
      setCurrentStepFollowMode(false);
      return;
    }

    expandForTask(taskId);
    enableCurrentStepFollow();
  }

  async function ensureTransformerSnReadyForPrint() {
    if (!unitFolder || setupConfirmedFolderRef.current !== unitFolder) {
      setLastMessage("Report setup must finish before printing the report.");
      return false;
    }

    if (!transformerSnDraft.trim()) {
      setLastMessage("Transformer SN is missing. Enter and save it before printing the report.");
      setTransformerSnSaveStatus("error");
      setTransformerSnError("Transformer SN is missing.");
      return false;
    }

    if (transformerSnDraft.trim() !== savedTransformerSnRef.current) {
      return saveTransformerSnDraft();
    }

    return true;
  }

  async function handleResetPanel() {
    if (!unitFolder) {
      heldFailureTaskIdRef.current = null;
      setTaskStates({});
      setFailureNotices({});
      setExpandedIds(new Set());
      setReportPath("");
      setPrintReportPath("");
      setResetClearsSelectionNext(false);
      setCurrentStepFollowMode(false);
      setConfirmedSetupFolder("");
      resetTransformerSnState();
      setLastMessage("");
      return;
    }

    if (resetClearsSelectionNext && !isRunningRef.current && !processingTaskRef.current) {
      heldFailureTaskIdRef.current = null;
      setUnitFolder("");
      setSerialNumber("");
      setReportPath("");
      setPrintReportPath("");
      detectedCountRef.current = 0;
      setDetectedCount(0);
      setSetupWarnings([]);
      setFailureNotices({});
      setExpandedIds(new Set());
      replaceTaskStates({});
      selectedFolderRef.current = "";
      folderScanPendingRef.current = false;
      setupPromiseRef.current = null;
      setupErrorRef.current = null;
      setConfirmedSetupFolder("");
      setCurrentStepFollowMode(false);
      setIsScanningFolder(false);
      setIsSettingUpReports(false);
      setProcessDetectedBacklog(null);
      processDetectedBacklogRef.current = null;
      latestTaskStatusesRef.current = {};
      setLatestTaskStatuses({});
      setResetClearsSelectionNext(false);
      resetTransformerSnState();
      setLastMessage("");
      return;
    }

    if (!resetClearsSelectionNext && !isRunningRef.current && !processingTaskRef.current && expandedIds.size > 0) {
      setExpandedIds(new Set());
      disableCurrentStepFollowForUserAction();
      setLastMessage("Collapsed all test groups. Press Reset again to reset the current SN.");
      return;
    }

    stopAfterCurrentTaskRef.current = false;
    setRunnerActive(false);
    setProcessDetectedBacklog(null);
    processDetectedBacklogRef.current = null;
    setConfirmedSetupFolder("");
    if (transformerSnDraftRef.current.trim()) {
      setTransformerSnSaveStatus("dirty");
    }
    setCurrentStepFollowMode(false);
    latestTaskStatusesRef.current = {};
    setLatestTaskStatuses({});
    heldFailureTaskIdRef.current = null;
    setFailureNotices({});
    activateTask(null);
    setExpandedIds(new Set());
    setResetClearsSelectionNext(true);
    setLastMessage("Refreshing unit folder");

    try {
      const summary = await scanUnitFolder(unitFolder);

      if (summary) {
        applyFolderSummary(summary, true);
        setLastMessage(
          summary.detected_count > 0
            ? `${summary.detected_count} detected test${summary.detected_count === 1 ? "" : "s"} ready. Press Start to continue, or Reset again to clear the selected SN.`
            : "Current SN reset to start. Press Reset again to clear the selected SN.",
        );
      }
    } catch (error) {
      setLastMessage(messageFromUnknownError(error));
    }
  }

  async function handleOpenReport() {
    if (processingTaskRef.current || isRunningRef.current) {
      setLastMessage("Pause and wait for the current step to finish before opening the report");
      return;
    }

    if (!reportPath) {
      setLastMessage("No report is available yet");
      return;
    }

    try {
      await openReportPath(unitFolder, reportPath);
      setLastMessage("Opened report");
    } catch (error) {
      setLastMessage(messageFromUnknownError(error));
    }
  }

  async function handlePrintReportClick() {
    if (processingTaskRef.current || isRunningRef.current) {
      setLastMessage("Pause and wait for the current step to finish before printing the report");
      return;
    }

    if (!unitFolder) {
      setLastMessage("Select Test Unit before printing the report.");
      return;
    }

    if (setupConfirmedFolderRef.current !== unitFolder) {
      setLastMessage("Report setup must finish before printing the report.");
      return;
    }

    if (!printReportPath) {
      setLastMessage("No print report is available yet.");
      return;
    }

    if (!(await ensureTransformerSnReadyForPrint())) {
      return;
    }

    try {
      const readiness = await validateReadyForPrint(unitFolder);

      if (!readiness.ready) {
        setPrintReadinessBlockers(readiness.blocking_issues);
        setLastMessage(readiness.message);
        return;
      }
    } catch (error) {
      const message = detailedMessageFromUnknownError(error);
      setLastMessage(message);
      return;
    }

    setPrintReadinessBlockers([]);
    setOperatorNameDraft((current) => current.trim() || operatorNames[0] || "");
    setOperatorNameError("");
    setOperatorDropdownOpen(false);
    setOperatorFilterText("");
    setPrintOperatorPromptOpen(true);
  }

  function handleRemoveOperatorName(name: string) {
    setOperatorNames((current) => {
      const key = operatorNameKey(name);
      const next = current.filter((operatorName) => operatorNameKey(operatorName) !== key);

      return storeOperatorNames(next);
    });

    if (operatorNameKey(operatorNameDraft) === operatorNameKey(name)) {
      setOperatorNameDraft("");
    }
  }

  function handleSelectOperatorName(name: string) {
    setOperatorNameDraft(name);
    setOperatorNameError("");
    setOperatorDropdownOpen(false);
    setOperatorFilterText("");
  }

  async function handleConfirmPrintReport() {
    if (processingTaskRef.current || isRunningRef.current) {
      const message = "Pause and wait for the current step to finish before printing the report";
      setOperatorNameError(message);
      setLastMessage(message);
      return;
    }

    const operatorName = operatorNameDraft.trim();

    if (!operatorName) {
      setOperatorNameError("Operator name is required.");
      return;
    }

    if (!unitFolder || setupConfirmedFolderRef.current !== unitFolder) {
      const message = "Report setup must finish before printing the report.";
      setOperatorNameError(message);
      setLastMessage(message);
      return;
    }

    setIsOpeningPrintDialog(true);
    setOperatorNameError("");
    setLastMessage("Saving final operator name");

    try {
      const readiness = await validateReadyForPrint(unitFolder);

      if (!readiness.ready) {
        const message = printReadinessMessage(readiness.blocking_issues) || readiness.message;
        setPrintReadinessBlockers(readiness.blocking_issues);
        setOperatorNameError(message);
        setLastMessage(readiness.message);
        return;
      }

      await runPanelReportWriteWithRetry(printReportPath, () =>
        saveFinalOperatorName(unitFolder, operatorName),
      );
      setOperatorNames((current) => storeOperatorNames(addOperatorName(current, operatorName)));
      setLastMessage("Opening print dialog");
      await openPrintReportDialog(unitFolder);
      setPrintOperatorPromptOpen(false);
      setPrintReadinessBlockers([]);
      setOperatorDropdownOpen(false);
      setOperatorFilterText("");
      setLastMessage("Print dialog opened");
    } catch (error) {
      const message = detailedMessageFromUnknownError(error);
      setOperatorNameError(message);
      setLastMessage(message);
    } finally {
      setIsOpeningPrintDialog(false);
    }
  }

  function toggleSection(id: string) {
    setExpandedIds((current) => {
      const next = new Set(current);

      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }

      return next;
    });
    disableCurrentStepFollowForUserAction();
  }

  const footerText = setupWarnings.length
    ? setupWarnings[0]
    : layoutProfileError
      ? layoutProfileError
    : isScanningFolder
      ? "Scanning unit folder..."
    : isSettingUpReports
      ? "Setting up reports..."
    : (detectedTaskCount || detectedCount) > 0 && !isRunning
      ? `${detectedTaskCount || detectedCount} detected task${(detectedTaskCount || detectedCount) === 1 ? "" : "s"}`
      : layoutProfile?.validation.warnings.length
        ? `${layoutProfile.display_name} - ${layoutProfile.validation.warnings.length} config warning`
        : backendStatus
          ? "Ready."
          : "Ready.";
  const appVersionText = `v${appVersion}`;
  const hasActiveSequence = Boolean(currentTaskId && unitFolder && setupConfirmedFolder === unitFolder);
  const unitDisplaySerialNumber = serialNumber || serialNumberFromFolder(unitFolder);
  const transformerSnTrimmed = transformerSnDraft.trim();
  const transformerSnCanSave =
    Boolean(transformerSnTrimmed) &&
    transformerSnSaveStatus !== "saving" &&
    transformerSnSaveStatus !== "saved";
  const showTransformerSnSavedInline =
    transformerSnSaveStatus === "saved" && Boolean(transformerSnTrimmed) && !transformerSnInputFocused;
  const transformerSnStatusText =
    transformerSnError ||
    (transformerSnSaveStatus === "saving"
      ? "Saving..."
      : transformerSnSaveStatus === "dirty"
          ? setupConfirmedFolder === unitFolder
            ? "Unsaved"
            : "Waiting for setup"
          : "");
  const transformerSnStatusTone =
    transformerSnSaveStatus === "error"
      ? "text-[#f4b1a9]"
      : transformerSnSaveStatus === "saved"
        ? "text-[#86efac]"
        : "text-[#d8d2c8]";
  const nextResetButtonLabel = resetButtonLabel({
    clearsSelectionNext: resetClearsSelectionNext,
    expandedCount: expandedIds.size,
    hasUnitFolder: Boolean(unitFolder),
    isRunning,
  });
  const controlState = panelControlState({
    hasActiveSequence,
    isFollowingCurrentStep: currentStepFollowMode,
    isRunning,
    resetLabel: nextResetButtonLabel,
  });
  const visibleOperatorNames = matchingOperatorNames(operatorNames, operatorFilterText);
  const settingsAccessDisabled =
    isRunning ||
    Boolean(currentTaskId) ||
    isSettingUpReports ||
    isScanningFolder ||
    isChoosingFolder ||
    isOpeningPrintDialog ||
    Boolean(backlogPrompt) ||
    printOperatorPromptOpen ||
    Boolean(workbookClosePrompt) ||
    printReadinessBlockers.length > 0;
  const workbookCloseName =
    workbookClosePrompt?.path.split(/[\\/]/).filter(Boolean).at(-1) ?? "report workbook";

  if (panelView === "notification-settings") {
    return (
      <NotificationSettingsPage
        onBack={() => setPanelView("operator")}
        loadSettings={getAppNotificationSettings}
        saveSettings={saveAppNotificationSettings}
        changePassword={changeSettingsPassword}
        sendTestPing={sendNotificationTest}
        getNotificationStatus={getNotificationStatus}
        chooseSharedFolder={chooseSharedNotificationsFolder}
        previewShiftSummary={previewShiftSummary}
        postShiftSummary={postShiftSummary}
        verifyPassword={verifySettingsPassword}
      />
    );
  }

  return (
    <main className="relative flex h-screen min-h-[400px] w-full min-w-[360px] max-w-full flex-col overflow-hidden bg-[#20201f] p-3.5 text-white">
      <button
        type="button"
        aria-label="Open notification settings"
        title={
          settingsAccessDisabled
            ? "Notification settings are unavailable while automation is active"
            : "Notification settings"
        }
        onClick={() => setPanelView("notification-settings")}
        disabled={settingsAccessDisabled}
        className="absolute right-3 top-3 z-10 inline-flex h-8 w-8 items-center justify-center rounded-md bg-[#3a3a38] text-[#d8d2c8] shadow-sm transition hover:bg-[#454542] hover:text-white disabled:cursor-not-allowed disabled:opacity-45 disabled:hover:bg-[#3a3a38] disabled:hover:text-[#d8d2c8]"
      >
        <SettingsIcon className="h-4 w-4" aria-hidden="true" />
      </button>
      <section className="px-1 py-2">
        <div className="text-center text-[26pt] font-bold leading-none tracking-normal text-white">
          {formatTime(remainingSeconds)}
        </div>
        <div className="mt-1 truncate text-center text-[8.5pt] leading-tight text-[#d8d2c8]">{statusText}</div>
      </section>

      <section className="mt-1 space-y-1.5 px-1">
        <div className="flex gap-1.5">
          <input
            readOnly
            aria-label="Selected test unit"
            title={unitFolder}
            value={unitDisplaySerialNumber}
            placeholder="Select Test Unit..."
            className="h-8 min-w-0 flex-1 basis-0 rounded border border-[#454542] bg-[#1f1f1e] px-2 text-[8.5pt] font-medium text-white placeholder:text-[#b7b1a8] outline-none"
          />
          <button
            type="button"
            aria-label="Browse unit folder"
            onClick={handleChooseFolder}
            disabled={isChoosingFolder || isRunning || isSettingUpReports}
            className="inline-flex h-8 w-9 shrink-0 items-center justify-center rounded bg-[#3a3a38] px-2 text-[8pt] font-semibold text-white shadow-sm hover:bg-[#454542] disabled:cursor-not-allowed disabled:opacity-65"
          >
            ...
          </button>
        </div>
        <div className="flex gap-1.5">
          <div className="relative min-w-0 flex-1 basis-0">
            <input
              aria-label="Transformer SN"
              value={transformerSnDraft}
              placeholder="Transformer SN..."
              onBlur={() => {
                setTransformerSnInputFocused(false);
                void saveTransformerSnDraft();
              }}
              onChange={(event) => updateTransformerSnDraft(event.target.value)}
              onFocus={() => setTransformerSnInputFocused(true)}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  event.preventDefault();
                  void saveTransformerSnDraft();
                }
              }}
              className={cn(
                "h-8 w-full min-w-0 rounded border bg-[#1f1f1e] px-2 text-[8.5pt] text-white placeholder:text-[#b7b1a8] outline-none focus:ring-2 focus:ring-cyan-200/25",
                showTransformerSnSavedInline && "pr-16",
                transformerSnSaveStatus === "error"
                  ? "border-[#d42c1a]"
                  : transformerSnSaveStatus === "saved"
                    ? "border-[#1d7f47]"
                    : "border-[#454542] focus:border-[#1f74ae]",
              )}
            />
            {showTransformerSnSavedInline ? (
              <span className="pointer-events-none absolute right-2 top-1/2 -translate-y-1/2 text-[7.3pt] font-semibold leading-none text-[#86efac]">
                Saved
              </span>
            ) : null}
          </div>
          <button
            type="button"
            aria-label="Save Transformer SN"
            title="Save Transformer SN"
            onMouseDown={(event) => event.preventDefault()}
            onClick={() => void saveTransformerSnDraft()}
            disabled={!transformerSnCanSave}
            className="inline-flex h-8 w-9 shrink-0 items-center justify-center rounded bg-[#3a3a38] px-2 text-[7.5pt] font-semibold text-white shadow-sm hover:bg-[#454542] disabled:cursor-not-allowed disabled:bg-[#353535] disabled:text-[#b7b1a8]"
          >
            {transformerSnSaveStatus === "saving" ? "..." : <Save className="h-3.5 w-3.5" aria-hidden="true" />}
          </button>
        </div>
        {transformerSnStatusText ? (
          <div
            role={transformerSnSaveStatus === "error" ? "alert" : undefined}
            className={cn(
              "truncate px-0.5 text-[7.3pt] leading-tight",
              transformerSnStatusTone,
            )}
            title={transformerSnError || undefined}
          >
            {transformerSnStatusText}
          </div>
        ) : null}
      </section>

      <WorkflowSteps
        scrollRef={scrollRef}
        scrollCue={scrollCue}
        panelItems={panelItems}
        expandedIds={expandedIds}
        currentTaskId={currentTaskId}
        failureNotices={failureNotices}
        onToggleSection={toggleSection}
        onRunTask={(taskId) => void runTask(taskId)}
        onPassTask={(taskId) => void handlePassTask(taskId)}
        onOpenFailureLocation={(notice) => void handleOpenFailureLocation(notice)}
        onOpenReport={() => void handleOpenReport()}
        onPrintReport={() => void handlePrintReportClick()}
        onScroll={handleWorkflowScroll}
        onTouchStart={handleWorkflowUserScrollIntent}
        onWheel={handleWorkflowUserScrollIntent}
      />

      <div className="px-1 pt-1.5">
        <UpdateActionButton state={updateState} onClick={() => void handleUpdateAction()} />
      </div>

      <div className="px-1 pb-1.5 text-[8.5pt] leading-tight text-[#d8d2c8]">
        {footerText}
      </div>

      <div className="grid grid-cols-2 gap-2">
        <button
          type="button"
          onClick={() => void handleRunClick()}
          className={cn(
            "inline-flex min-h-9 items-center justify-center gap-1.5 rounded-md px-3 py-2 text-[9pt] font-semibold shadow-sm transition",
            controlState.primaryAction === "pause"
              ? "bg-[#9b630c] text-white hover:bg-[#9b630c]"
              : controlState.primaryAction === "current-step"
                ? "bg-[#1f74ae] text-white hover:bg-[#2874a8]"
                : "bg-[#1d7f47] text-white hover:bg-[#1d7f46]",
          )}
        >
          {controlState.primaryLabel}
        </button>
        <button
          type="button"
          onClick={handleSecondaryControlClick}
          disabled={controlState.secondaryDisabled}
          className={cn(
            "inline-flex min-h-9 items-center justify-center gap-1.5 rounded-md px-3 py-2 text-[9pt] font-semibold text-white shadow-sm transition",
            controlState.secondaryAction === "follow-step"
              ? "bg-[#1f74ae] hover:bg-[#2874a8]"
              : "bg-[#3a3a38] hover:bg-[#454542]",
            controlState.secondaryDisabled && "cursor-not-allowed opacity-60 hover:bg-[#3a3a38]",
          )}
        >
          {controlState.secondaryLabel}
        </button>
      </div>

      <footer className="mt-2 border-t border-[#454542] pt-2 text-[7.5pt] leading-tight text-[#d8d2c8]">
        <div className="flex items-center justify-between gap-3">
          <span>{appVersionText}</span>
          <span className="font-medium">Built by Syed Hassaan Shah</span>
        </div>
      </footer>

      {workbookClosePrompt ? (
        <div className="fixed inset-0 z-[60] flex items-center justify-center bg-black/60 p-4">
          <section
            role="dialog"
            aria-modal="true"
            aria-labelledby="close-report-workbook-title"
            className="w-full max-w-[360px] rounded-md border border-[#6b4d24] bg-[#292928] p-4 text-white shadow-2xl"
          >
            <div
              id="close-report-workbook-title"
              className="text-center text-[12pt] font-semibold leading-tight"
            >
              Close Report Workbook
            </div>
            <p className="mt-3 text-center text-[8.5pt] leading-snug text-[#f1d5aa]">
              {workbookClosePrompt.message}
            </p>
            <div className="mt-3 rounded border border-[#454542] bg-[#1f1f1e] px-2 py-2 text-center text-[8pt] font-semibold text-white">
              {workbookCloseName}
            </div>
            {workbookCloseError ? (
              <div role="alert" className="mt-2 text-center text-[7.5pt] leading-tight text-[#f4b1a9]">
                {workbookCloseError}
              </div>
            ) : null}
            <div className="mt-4 grid grid-cols-2 gap-2">
              <button
                type="button"
                onClick={cancelWorkbookClosePrompt}
                disabled={isClosingReportWorkbook}
                className="inline-flex min-h-9 items-center justify-center rounded-md bg-[#3a3a38] px-3 py-2 text-[9pt] font-semibold text-white shadow-sm transition hover:bg-[#454542] disabled:cursor-not-allowed disabled:opacity-65"
              >
                Cancel
              </button>
              <button
                type="button"
                autoFocus
                onClick={() => void handleCloseReportWorkbook()}
                disabled={isClosingReportWorkbook}
                className="inline-flex min-h-9 items-center justify-center rounded-md bg-[#9b630c] px-3 py-2 text-[9pt] font-semibold text-white shadow-sm transition hover:bg-[#ad7111] disabled:cursor-not-allowed disabled:opacity-65"
              >
                {isClosingReportWorkbook ? "Closing..." : "Close Report"}
              </button>
            </div>
          </section>
        </div>
      ) : null}

      {printReadinessBlockers.length > 0 && !printOperatorPromptOpen ? (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/55 p-4">
          <section className="w-full max-w-[420px] rounded-md border border-[#454542] bg-[#292928] p-4 text-white shadow-2xl">
            <div className="text-center text-[12pt] font-semibold leading-tight">
              Print Blocked
            </div>
            <div className="mt-3 max-h-56 overflow-y-auto rounded border border-[#454542] bg-[#1f1f1e] p-2 text-[8pt] leading-snug text-[#f4d0cb] [scrollbar-width:thin]">
              {printReadinessBlockers.slice(0, 12).map((blocker, index) => (
                <div key={`${blocker.task_id ?? blocker.code}-${index}`} className="py-1">
                  <span className="font-semibold text-white">
                    {blocker.label ?? blocker.task_id ?? "Report"}
                  </span>
                  {": "}
                  <span>{blocker.reason}</span>
                </div>
              ))}
              {printReadinessBlockers.length > 12 ? (
                <div className="border-t border-[#454542] pt-1 text-[#d8d2c8]">
                  {printReadinessBlockers.length - 12} more blocking issue
                  {printReadinessBlockers.length - 12 === 1 ? "" : "s"}
                </div>
              ) : null}
            </div>
            <div className="mt-4 flex justify-end">
              <button
                type="button"
                onClick={() => setPrintReadinessBlockers([])}
                className="inline-flex min-h-9 items-center justify-center rounded-md bg-[#3a3a38] px-3 py-2 text-[9pt] font-semibold text-white shadow-sm transition hover:bg-[#454542]"
              >
                Close
              </button>
            </div>
          </section>
        </div>
      ) : null}

      {printOperatorPromptOpen ? (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/55 p-4">
          <section className="w-full max-w-[320px] rounded-md border border-[#454542] bg-[#292928] p-4 text-white shadow-2xl">
            <div className="text-center text-[12pt] font-semibold leading-tight">
              Print Report
            </div>
            <div className="relative mt-4">
              <div
                className={cn(
                  "flex h-9 rounded border bg-[#1f1f1e] focus-within:ring-2 focus-within:ring-cyan-200/25",
                  operatorNameError ? "border-[#d42c1a]" : "border-[#454542] focus-within:border-[#1f74ae]",
                )}
              >
                <input
                  aria-controls="print-report-operator-menu"
                  aria-expanded={operatorDropdownOpen}
                  aria-haspopup="listbox"
                  aria-label="Operator name"
                  autoFocus
                  value={operatorNameDraft}
                  placeholder="Operator name..."
                  onChange={(event) => {
                    const value = event.target.value;

                    setOperatorNameDraft(value);
                    setOperatorFilterText(value);
                    setOperatorNameError("");
                    setPrintReadinessBlockers([]);
                    setOperatorDropdownOpen(true);
                  }}
                  onKeyDown={(event) => {
                    if (event.key === "Enter") {
                      event.preventDefault();
                      void handleConfirmPrintReport();
                    } else if (event.key === "ArrowDown") {
                      event.preventDefault();
                      setOperatorFilterText(operatorNameDraft);
                      setOperatorDropdownOpen(true);
                    } else if (event.key === "Escape") {
                      setOperatorDropdownOpen(false);
                    }
                  }}
                  className="min-w-0 flex-1 rounded-l bg-transparent px-2 text-[9pt] text-white placeholder:text-[#b7b1a8] outline-none"
                />
                <button
                  type="button"
                  aria-label="Show operator names"
                  aria-expanded={operatorDropdownOpen}
                  aria-controls="print-report-operator-menu"
                  onClick={() => {
                    setOperatorFilterText("");
                    setOperatorDropdownOpen((open) => !open);
                  }}
                  className="inline-flex w-9 shrink-0 items-center justify-center rounded-r border-l border-[#454542] text-[#d8d2c8] transition hover:bg-[#353534] hover:text-white"
                >
                  <ChevronDown className="h-4 w-4" aria-hidden="true" />
                </button>
              </div>
              {operatorDropdownOpen ? (
                <div
                  id="print-report-operator-menu"
                  role="listbox"
                  aria-label="Saved operators"
                  className="absolute left-0 right-0 top-full z-10 mt-1 max-h-36 overflow-y-auto rounded border border-[#454542] bg-[#242423] p-1 shadow-xl [scrollbar-width:thin]"
                >
                  {visibleOperatorNames.length ? (
                    visibleOperatorNames.map((name) => (
                      <div key={name} className="flex min-h-8 items-center gap-1 rounded hover:bg-[#30302f]">
                        <button
                          type="button"
                          role="option"
                          aria-selected={operatorNameKey(operatorNameDraft) === operatorNameKey(name)}
                          onClick={() => handleSelectOperatorName(name)}
                          className="min-w-0 flex-1 truncate px-2 text-left text-[8.5pt] font-medium text-white"
                        >
                          {name}
                        </button>
                        <button
                          type="button"
                          aria-label={`Remove ${name}`}
                          title={`Remove ${name}`}
                          onClick={() => handleRemoveOperatorName(name)}
                          className="inline-flex h-7 w-7 shrink-0 items-center justify-center rounded text-[#d8d2c8] transition hover:bg-[#454542] hover:text-white"
                        >
                          <Trash2 className="h-3.5 w-3.5" aria-hidden="true" />
                        </button>
                      </div>
                    ))
                  ) : (
                    <div className="px-2 py-1.5 text-[8pt] text-[#b7b1a8]">
                      {operatorNames.length ? "No matching operators" : "No saved operators"}
                    </div>
                  )}
                </div>
              ) : null}
              {operatorNameError ? (
                <div role="alert" className="mt-1.5 text-[7.5pt] leading-tight text-[#f4b1a9]">
                  {operatorNameError}
                </div>
              ) : null}
              {printReadinessBlockers.length > 0 ? (
                <div className="mt-2 max-h-28 overflow-y-auto rounded border border-[#59342e] bg-[#241f1e] p-2 text-[7.5pt] leading-tight text-[#f4d0cb] [scrollbar-width:thin]">
                  {printReadinessBlockers.slice(0, 6).map((blocker, index) => (
                    <div key={`${blocker.task_id ?? blocker.code}-${index}`} className="py-0.5">
                      <span className="font-semibold text-white">
                        {blocker.label ?? blocker.task_id ?? "Report"}
                      </span>
                      {": "}
                      <span>{blocker.reason}</span>
                    </div>
                  ))}
                </div>
              ) : null}
            </div>
            <div className="mt-4 grid grid-cols-2 gap-2">
              <button
                type="button"
                onClick={() => {
                  setPrintOperatorPromptOpen(false);
                  setPrintReadinessBlockers([]);
                  setOperatorDropdownOpen(false);
                  setOperatorFilterText("");
                }}
                disabled={isOpeningPrintDialog}
                className="inline-flex min-h-9 items-center justify-center rounded-md bg-[#3a3a38] px-3 py-2 text-[9pt] font-semibold text-white shadow-sm transition hover:bg-[#454542] disabled:cursor-not-allowed disabled:opacity-65"
              >
                Cancel
              </button>
              <button
                type="button"
                onClick={() => void handleConfirmPrintReport()}
                disabled={isOpeningPrintDialog}
                className="inline-flex min-h-9 items-center justify-center rounded-md bg-[#1d7f47] px-3 py-2 text-[9pt] font-semibold text-white shadow-sm transition hover:bg-[#1d7f46] disabled:cursor-not-allowed disabled:opacity-65"
              >
                {isOpeningPrintDialog ? "Opening..." : "Confirm & Print"}
              </button>
            </div>
          </section>
        </div>
      ) : null}

      {backlogPrompt ? (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/55 p-4">
          <section className="w-full max-w-[320px] rounded-md border border-[#454542] bg-[#292928] p-4 text-white shadow-2xl">
            <div className="text-center text-[12pt] font-semibold leading-tight">
              Previous Tests Detected
            </div>
            <p className="mt-3 text-center text-[9pt] leading-snug text-[#d8d2c8]">
              Found {backlogPrompt.count} previously completed test
              {backlogPrompt.count === 1 ? "" : "s"}.
            </p>
            <p className="mt-2 text-center text-[8.5pt] leading-snug text-[#d8d2c8]">
              Process those report steps first, or skip ahead to the current unfinished step.
            </p>
            <div className="mt-4 grid gap-2">
              <button
                type="button"
                onClick={() => resolveBacklogPrompt(true)}
                className="inline-flex min-h-9 items-center justify-center rounded-md bg-[#1d7f47] px-3 py-2 text-[9pt] font-semibold text-white shadow-sm transition hover:bg-[#1d7f46]"
              >
                Batch Run Previous Tests
              </button>
              <button
                type="button"
                onClick={() => resolveBacklogPrompt(false)}
                className="inline-flex min-h-9 items-center justify-center rounded-md bg-[#3a3a38] px-3 py-2 text-[9pt] font-semibold text-white shadow-sm transition hover:bg-[#454542]"
              >
                Skip to Current Test
              </button>
              <button
                type="button"
                onClick={() => resolveBacklogPrompt(null)}
                className="inline-flex min-h-9 items-center justify-center rounded-md bg-[#2f2f2e] px-3 py-2 text-[9pt] font-semibold text-white shadow-sm transition hover:bg-[#3a3a38]"
              >
                Cancel
              </button>
            </div>
          </section>
        </div>
      ) : null}

    </main>
  );
}
