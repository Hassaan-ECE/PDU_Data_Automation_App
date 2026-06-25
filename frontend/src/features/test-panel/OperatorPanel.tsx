import type { ReactNode } from "react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { ChevronDown, ExternalLink, RotateCcw, Save, SkipForward, Trash2 } from "lucide-react";

import {
  chooseUnitFolder,
  getBackendStatus,
  loadLayoutProfile,
  openPrintReportDialog,
  openReportLocation,
  openReportPath,
  processAutomationTask,
  saveFinalOperatorName,
  saveTransformerSn,
  scanUnitFolder,
  setupUnitFolder,
  validateReadyForPrint,
  type BackendTaskStatus,
  type BackendStatus,
  type FailureDetail,
  type FailureLocation,
  type LayoutLoadResponse,
  type PrintReadinessBlocker,
  type TaskProcessResult,
  type UnitFolderSummary,
} from "@/integrations/tauri/backend";
import { markStartup } from "@/shared/lib/startupTiming";
import { cn } from "@/shared/lib/utils";

import {
  addOperatorName,
  loadOperatorNames,
  matchingOperatorNames,
  operatorNameKey,
  storeOperatorNames,
} from "./operatorNames";
import { stateStyles } from "./stateStyles";
import { legacyPanelItems } from "./taskModel";
import { isSectionItem, type PanelItem, type SectionItem, type TaskItem, type TaskState } from "./types";
import { UpdateActionButton } from "./UpdateActionButton";
import { useDesktopUpdates } from "./useDesktopUpdates";

const DEFAULT_LOAD_DURATION_SECONDS = 3 * 60;
const TRANSFORMER_DURATION_SECONDS = 60;
const SYSTEM_BURN_IN_DURATION_SECONDS = 2 * 60 * 60;

type BacklogPromptState = {
  count: number;
  resolve: (processBacklog: boolean) => void;
} | null;

type TransformerSnSaveStatus = "idle" | "dirty" | "saving" | "saved" | "error";

type TaskFailureNotice = {
  taskId: string;
  title: string;
  message: string;
  reportPath: string | null;
  location: FailureLocation | null;
  fromRunner: boolean;
};

type BackendTaskStatusMap = Record<string, BackendTaskStatus | undefined>;

function printReadinessMessage(blockers: PrintReadinessBlocker[]) {
  if (blockers.length === 0) {
    return "";
  }

  return blockers
    .slice(0, 6)
    .map((blocker) => {
      const label = blocker.label ?? blocker.task_id ?? "Report";

      return `${label}: ${blocker.reason}`;
    })
    .join("\n");
}

function formatTime(seconds: number) {
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  const remainingSeconds = seconds % 60;

  return [hours, minutes, remainingSeconds].map((value) => value.toString().padStart(2, "0")).join(":");
}

function flattenTasks(items: PanelItem[]): TaskItem[] {
  return items.flatMap((item) => (isSectionItem(item) ? flattenTasks(item.children) : [item]));
}

function applyTaskStates(items: PanelItem[], states: Record<string, TaskState>): PanelItem[] {
  return items.map((item) => {
    if (isSectionItem(item)) {
      return {
        ...item,
        children: applyTaskStates(item.children, states),
      };
    }

    return {
      ...item,
      state: states[item.id] ?? item.state,
    };
  });
}

function findTaskPath(items: PanelItem[], taskId: string, path: string[] = []): string[] | null {
  for (const item of items) {
    if (isSectionItem(item)) {
      const childPath = findTaskPath(item.children, taskId, [...path, item.id]);

      if (childPath) {
        return childPath;
      }
    } else if (item.id === taskId) {
      return path;
    }
  }

  return null;
}

function isTerminalState(state: TaskState | undefined) {
  return state === "pass" || state === "fail" || state === "skipped";
}

function getSectionState(items: PanelItem[]): TaskState {
  const tasks = flattenTasks(items);

  if (tasks.some((task) => task.state === "processing" || task.state === "waiting")) {
    return "processing";
  }

  if (tasks.some((task) => task.state === "fail")) {
    return "fail";
  }

  if (tasks.some((task) => task.state === "warning")) {
    return "warning";
  }

  if (tasks.some((task) => task.state === "skipped")) {
    return "warning";
  }

  if (tasks.some((task) => task.state === "detected")) {
    return "detected";
  }

  if (tasks.length > 0 && tasks.every((task) => task.state === "pass")) {
    return "pass";
  }

  return "off";
}

function sectionProgress(items: PanelItem[]) {
  const tasks = flattenTasks(items);
  const completed = tasks.filter((task) => task.state === "pass" || task.state === "skipped").length;

  return `${completed}/${tasks.length}`;
}

function findNextTaskForRunner(
  tasks: TaskItem[],
  states: Record<string, TaskState>,
  processDetectedBacklog: boolean,
  backendTasks: BackendTaskStatusMap = {},
) {
  if (processDetectedBacklog) {
    const detectedTask = tasks.find((task) => {
      const state = states[task.id] ?? task.state;

      return state === "detected" || state === "waiting" || state === "warning";
    });

    if (detectedTask) {
      return detectedTask;
    }
  }

  const inProgressDetectedTask = tasks.find((task) => {
    const state = states[task.id] ?? task.state;

    return state === "detected" && detectedCsvStillInProgress(task.id, backendTasks[task.id]);
  });

  if (inProgressDetectedTask) {
    return inProgressDetectedTask;
  }

  return tasks.find((task) => {
    const state = states[task.id] ?? task.state;

    return state === "off" || state === "waiting" || state === "warning";
  });
}

function taskDurationSeconds(taskId: string) {
  if (taskId.endsWith("-transformer")) {
    return TRANSFORMER_DURATION_SECONDS;
  }

  if (taskId === "system-burn-in") {
    return SYSTEM_BURN_IN_DURATION_SECONDS;
  }

  return DEFAULT_LOAD_DURATION_SECONDS;
}

function csvWaitSecondsRemaining(
  taskId: string,
  task: BackendTaskStatus | undefined,
) {
  const timerStartMs = task?.timer_start_ms ?? task?.latest_csv_created_ms;

  if (!timerStartMs) {
    return 0;
  }

  const elapsedSeconds = Math.floor(Math.max(0, Date.now() - timerStartMs) / 1000);

  return Math.max(0, taskDurationSeconds(taskId) - elapsedSeconds);
}

function detectedCsvStillInProgress(taskId: string, task: BackendTaskStatus | undefined) {
  return csvWaitSecondsRemaining(taskId, task) > 0 || task?.latest_csv_readable === false;
}

function shouldProcessDetectedCsv(taskId: string, task: BackendTaskStatus | undefined) {
  return csvWaitSecondsRemaining(taskId, task) <= 0 && task?.latest_csv_readable !== false;
}

function backendTaskStatusMap(tasks: BackendTaskStatus[]) {
  return Object.fromEntries(tasks.map((task) => [task.task_id, task])) as BackendTaskStatusMap;
}

function taskRemainingSeconds(
  task: TaskItem,
  state: TaskState,
  backendTask: BackendTaskStatus | undefined,
  isCurrentTask: boolean,
) {
  if (isTerminalState(state)) {
    return 0;
  }

  const csvRemaining = csvWaitSecondsRemaining(task.id, backendTask);
  const hasDetectedCsv = state === "detected" || backendTask?.state === "detected";

  if (hasDetectedCsv) {
    return csvRemaining;
  }

  if (isCurrentTask || state === "off" || state === "waiting" || state === "warning") {
    return taskDurationSeconds(task.id);
  }

  return 0;
}

function remainingSecondsForTasks(
  tasks: TaskItem[],
  states: Record<string, TaskState>,
  backendTasks: BackendTaskStatusMap,
  currentTaskId: string | null,
) {
  return tasks.reduce((total, task) => {
    const state = states[task.id] ?? task.state;

    return total + taskRemainingSeconds(task, state, backendTasks[task.id], task.id === currentTaskId);
  }, 0);
}

function failureNoticeFromResult(
  taskId: string,
  result: TaskProcessResult,
  fromRunner: boolean,
): TaskFailureNotice {
  const failure: FailureDetail | null = result.failure;

  return {
    taskId,
    title: failure?.title ?? (result.state === "warning" ? "Step Warning" : "Step Failed"),
    message: failure?.message ?? result.message,
    reportPath: result.report_path ?? failure?.location?.workbook_path ?? null,
    location: failure?.location ?? null,
    fromRunner,
  };
}

function detectedReadyMessage(count: number) {
  return count > 0
    ? `${count} detected test${count === 1 ? "" : "s"} ready. Press Start to choose how to continue.`
    : "Ready to start";
}

function detectedTaskCountFromStates(states: Record<string, TaskState>) {
  return Object.values(states).filter((state) => state === "detected").length;
}

function serialNumberFromFolder(unitFolder: string) {
  const folderName = unitFolder.split(/[\\/]/).filter(Boolean).at(-1) ?? "";

  return folderName.match(/\d{6,}/)?.[0] ?? "";
}

function messageFromUnknownError(error: unknown) {
  if (error instanceof Error) {
    return error.message;
  }

  if (error && typeof error === "object" && "message" in error) {
    const message = (error as { message?: unknown }).message;

    if (typeof message === "string" && message.trim()) {
      return message;
    }
  }

  return String(error);
}

function detailedMessageFromUnknownError(error: unknown) {
  const message = messageFromUnknownError(error);

  if (error && typeof error === "object" && "details" in error) {
    const details = (error as { details?: unknown }).details;

    if (typeof details === "string" && details.trim() && !message.includes(details.trim())) {
      return `${message} ${details.trim()}`;
    }
  }

  return message;
}

function resetButtonLabel({
  expandedCount,
  hasUnitFolder,
  isRunning,
  clearsSelectionNext,
}: {
  expandedCount: number;
  hasUnitFolder: boolean;
  isRunning: boolean;
  clearsSelectionNext: boolean;
}) {
  if (!hasUnitFolder) {
    return "Reset Panel";
  }

  if (clearsSelectionNext) {
    return "Clear SN";
  }

  if (!isRunning && expandedCount > 0) {
    return "Collapse Tests";
  }

  return "Reset Current SN";
}

type PrimaryControlAction = "current-step" | "pause" | "resume" | "start";
type SecondaryControlAction = "follow-step" | "reset";

type PanelControlState = {
  kind: "idle" | "paused-away" | "paused-current" | "running-following" | "running-unfollowed";
  primaryAction: PrimaryControlAction;
  primaryLabel: string;
  secondaryAction: SecondaryControlAction;
  secondaryDisabled: boolean;
  secondaryLabel: string;
};

function panelControlState({
  hasActiveSequence,
  isFollowingCurrentStep,
  isRunning,
  resetLabel,
}: {
  hasActiveSequence: boolean;
  isFollowingCurrentStep: boolean;
  isRunning: boolean;
  resetLabel: string;
}): PanelControlState {
  if (!hasActiveSequence) {
    return {
      kind: "idle",
      primaryAction: "start",
      primaryLabel: "Start",
      secondaryAction: "reset",
      secondaryDisabled: false,
      secondaryLabel: resetLabel,
    };
  }

  if (isRunning) {
    if (!isFollowingCurrentStep) {
      return {
        kind: "running-unfollowed",
        primaryAction: "pause",
        primaryLabel: "Pause",
        secondaryAction: "follow-step",
        secondaryDisabled: false,
        secondaryLabel: "Follow Step",
      };
    }

    return {
      kind: "running-following",
      primaryAction: "pause",
      primaryLabel: "Pause",
      secondaryAction: "reset",
      secondaryDisabled: true,
      secondaryLabel: resetLabel,
    };
  }

  if (!isFollowingCurrentStep) {
    return {
      kind: "paused-away",
      primaryAction: "current-step",
      primaryLabel: "Current Step",
      secondaryAction: "reset",
      secondaryDisabled: false,
      secondaryLabel: resetLabel,
    };
  }

  return {
    kind: "paused-current",
    primaryAction: "resume",
    primaryLabel: "Resume",
    secondaryAction: "reset",
    secondaryDisabled: false,
    secondaryLabel: resetLabel,
  };
}

function panelDepthWidth(depth: number) {
  if (depth <= 0) {
    return "w-full";
  }

  if (depth === 1) {
    return "mx-auto w-[92%]";
  }

  return "mx-auto w-[84%]";
}

function PanelButton({
  label,
  state,
  onClick,
  trailing,
  section = false,
  depth = 0,
  current = false,
}: {
  label: string;
  state: TaskState;
  onClick?: () => void;
  trailing?: ReactNode;
  section?: boolean;
  depth?: number;
  current?: boolean;
}) {
  const styles = stateStyles[state];

  return (
    <button
      type="button"
      aria-label={label}
      onClick={onClick}
      className={cn(
        "group relative flex min-h-9 max-w-full items-center justify-center rounded-md px-4 py-2 text-center shadow-sm transition",
        "focus:outline-none focus-visible:z-10 focus-visible:ring-2 focus-visible:ring-cyan-200/25 focus-visible:ring-offset-2 focus-visible:ring-offset-[#20201f]",
        panelDepthWidth(depth),
        current && "z-10 ring-2 ring-cyan-200/65 ring-offset-2 ring-offset-[#20201f]",
        styles.button,
      )}
      data-current-task={current ? "true" : undefined}
    >
      <span
        className={cn(
          "min-w-0 max-w-full truncate text-[9pt] leading-tight",
          section ? "font-semibold" : "font-medium",
        )}
      >
        {label}
      </span>
      {trailing ? (
        <span className="absolute right-2.5 top-1/2 flex -translate-y-1/2 items-center">
          {trailing}
        </span>
      ) : null}
    </button>
  );
}

function TaskFailureDialog({
  notice,
  depth,
  onRerun,
  onSkip,
  onOpenLocation,
}: {
  notice: TaskFailureNotice;
  depth: number;
  onRerun: () => void;
  onSkip: () => void;
  onOpenLocation: () => void;
}) {
  const locationLabel = notice.location
    ? `${notice.location.sheet}!${notice.location.cell}`
    : notice.reportPath
      ? "Report workbook"
      : "";

  return (
    <div className={cn("mt-1 rounded-md border border-[#d42c1a] bg-[#301f22] p-2.5 shadow-sm", panelDepthWidth(depth))}>
      <div className="text-[8.5pt] font-semibold leading-tight text-white">{notice.title}</div>
      <div className="mt-1 max-h-20 overflow-y-auto text-[7.5pt] leading-snug text-[#d8d2c8] [scrollbar-width:thin]">
        {notice.message}
      </div>
      {locationLabel ? (
        <div className="mt-1 truncate text-[7pt] leading-tight text-[#b7b1a8]">{locationLabel}</div>
      ) : null}
      <div className="mt-2 grid grid-cols-3 gap-1.5">
        <button
          type="button"
          onClick={onRerun}
          className="inline-flex min-h-7 items-center justify-center gap-1 rounded bg-[#ab5a13] px-1.5 text-[7.2pt] font-semibold text-white shadow-sm transition hover:bg-[#a75c19]"
        >
          <RotateCcw className="h-3 w-3" aria-hidden="true" />
          Rerun
        </button>
        <button
          type="button"
          onClick={onSkip}
          className="inline-flex min-h-7 items-center justify-center gap-1 rounded bg-[#3d4142] px-1.5 text-[7.2pt] font-semibold text-white shadow-sm transition hover:bg-[#484d4e]"
        >
          <SkipForward className="h-3 w-3" aria-hidden="true" />
          Skip
        </button>
        <button
          type="button"
          onClick={onOpenLocation}
          disabled={!notice.location && !notice.reportPath}
          className={cn(
            "inline-flex min-h-7 items-center justify-center gap-1 rounded px-1.5 text-[7.2pt] font-semibold shadow-sm transition",
            notice.location || notice.reportPath
              ? "bg-[#9752b3] text-white hover:bg-[#9a4fba]"
              : "cursor-not-allowed bg-[#353535] text-[#b7b1a8]",
          )}
        >
          <ExternalLink className="h-3 w-3" aria-hidden="true" />
          Open
        </button>
      </div>
    </div>
  );
}

function TaskRow({
  task,
  currentTaskId,
  depth = 0,
  onRunTask,
  failureNotice,
  onSkipTask,
  onOpenFailureLocation,
}: {
  task: TaskItem;
  currentTaskId: string | null;
  depth?: number;
  onRunTask: (taskId: string) => void;
  failureNotice?: TaskFailureNotice;
  onSkipTask: (taskId: string) => void;
  onOpenFailureLocation: (notice: TaskFailureNotice) => void;
}) {
  return (
    <div>
      <PanelButton
        label={task.label}
        state={task.state}
        depth={depth}
        current={task.id === currentTaskId}
        onClick={() => onRunTask(task.id)}
      />
      {failureNotice ? (
        <TaskFailureDialog
          notice={failureNotice}
          depth={depth}
          onRerun={() => onRunTask(task.id)}
          onSkip={() => onSkipTask(task.id)}
          onOpenLocation={() => onOpenFailureLocation(failureNotice)}
        />
      ) : null}
    </div>
  );
}

function SectionBlock({
  section,
  expanded,
  onToggle,
  isExpanded,
  currentTaskId,
  onRunTask,
  failureNotices,
  onSkipTask,
  onOpenFailureLocation,
  depth = 0,
}: {
  section: SectionItem;
  expanded: boolean;
  onToggle: (id: string) => void;
  isExpanded: (id: string) => boolean;
  currentTaskId: string | null;
  onRunTask: (taskId: string) => void;
  failureNotices: Record<string, TaskFailureNotice>;
  onSkipTask: (taskId: string) => void;
  onOpenFailureLocation: (notice: TaskFailureNotice) => void;
  depth?: number;
}) {
  const state = getSectionState(section.children);
  const containsNestedSections = section.children.some(isSectionItem);

  return (
    <div className="space-y-1">
      <PanelButton
        label={section.label}
        state={state}
        onClick={() => onToggle(section.id)}
        section
        depth={depth}
        trailing={
          <span className="rounded bg-black/20 px-1.5 py-0.5 text-[7.5pt] font-semibold tabular-nums text-white/70">
            {sectionProgress(section.children)}
          </span>
        }
      />
      {expanded ? (
        <div className={cn("pt-1", containsNestedSections ? "space-y-1.5" : "space-y-1")}>
          {section.children.map((item) =>
            isSectionItem(item) ? (
              <SectionBlock
                key={item.id}
                section={item}
                expanded={isExpanded(item.id)}
                onToggle={onToggle}
                isExpanded={isExpanded}
                currentTaskId={currentTaskId}
                onRunTask={onRunTask}
                failureNotices={failureNotices}
                onSkipTask={onSkipTask}
                onOpenFailureLocation={onOpenFailureLocation}
                depth={depth + 1}
              />
            ) : (
              <TaskRow
                key={item.id}
                task={item}
                currentTaskId={currentTaskId}
                onRunTask={onRunTask}
                failureNotice={failureNotices[item.id]}
                onSkipTask={onSkipTask}
                onOpenFailureLocation={onOpenFailureLocation}
                depth={depth + 1}
              />
            ),
          )}
        </div>
      ) : null}
    </div>
  );
}

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
  const allTaskOrder = useMemo(() => flattenTasks(legacyPanelItems), []);
  const [unitFolder, setUnitFolder] = useState("");
  const [serialNumber, setSerialNumber] = useState("");
  const [remainingSeconds, setRemainingSeconds] = useState(0);
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
  const appVersion = backendStatus?.version ?? "0.2.9";
  const panelItems = useMemo(() => applyTaskStates(legacyPanelItems, taskStates), [taskStates]);
  const detectedTaskCount = useMemo(
    () => detectedTaskCountFromStates(taskStates),
    [taskStates],
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
    return new Promise<boolean>((resolve) => {
      setBacklogPrompt({ count, resolve });
    });
  }, []);

  const resolveBacklogPrompt = useCallback((processBacklog: boolean) => {
    setBacklogPrompt((current) => {
      current?.resolve(processBacklog);
      return null;
    });
  }, []);

  useEffect(() => {
    if (!isRunning) {
      return;
    }

    const handle = window.setInterval(
      () => setRemainingSeconds((value) => Math.max(0, value - 1)),
      1000,
    );

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

      setRemainingSeconds(
        remainingSecondsForTasks(
          allTaskOrder,
          taskStatesRef.current,
          latestTaskStatusesRef.current,
          taskId,
        ),
      );

      if (taskId) {
        expandForTask(taskId);
      }
    },
    [allTaskOrder, expandForTask],
  );

  const replaceTaskStates = useCallback(
    (states: Record<string, TaskState>) => {
      taskStatesRef.current = states;
      setTaskStates(states);
      setRemainingSeconds(
        remainingSecondsForTasks(
          allTaskOrder,
          states,
          latestTaskStatusesRef.current,
          currentTaskIdRef.current,
        ),
      );
    },
    [allTaskOrder],
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
    latestTaskStatusesRef.current = backendTaskStatusMap(summary.tasks);

    if (replace) {
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

      const setupPromise = setupUnitFolder(selected, trimmedTransformerSn, unitSerialNumber)
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
    [applyFolderSummary, setConfirmedSetupFolder],
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
      setTransformerSnSaveStatus("dirty");
      setTransformerSnError("");
      setLastMessage("Transformer SN will be saved during setup.");
      return true;
    }

    if (transformerSn === savedTransformerSnRef.current) {
      setTransformerSnSaveStatus("saved");
      setTransformerSnError("");
      return true;
    }

    setTransformerSnSaveStatus("saving");
    setTransformerSnError("");

    try {
      await saveTransformerSn(selected, transformerSn);
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
  }, [transformerSnDraft]);

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
        setLastMessage("Press Start to set up reports before running steps.");
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
      return "Sequence complete. Transformer SN is missing before report opening.";
    }

    if (transformerSn !== savedTransformerSnRef.current) {
      return "Sequence complete. Transformer SN is unsaved before report opening.";
    }

    return "Sequence complete";
  }, []);

  const runTask = useCallback(
    async (taskId: string, fromRunner = false): Promise<TaskState | null> => {
      if (!unitFolder || processingTaskRef.current) {
        return null;
      }

      if (!fromRunner && isRunningRef.current) {
        setLastMessage("Pause the runner before rerunning an individual task");
        return null;
      }

      if (!(await ensureReportSetupReady(unitFolder))) {
        return null;
      }

      processingTaskRef.current = true;
      activateTask(taskId);
      setFailureNotices((current) => {
        if (!current[taskId]) {
          return current;
        }

        const next = { ...current };
        delete next[taskId];
        return next;
      });
      updateTaskState(taskId, "processing");
      setLastMessage("Processing report data");

      try {
        const result: TaskProcessResult | null = await processAutomationTask(unitFolder, taskId);

        if (!result) {
          updateTaskState(taskId, "pass");
          setLastMessage("Mock task processed");
          return "pass";
        }

        updateTaskState(taskId, result.state);
        setLastMessage(result.message);

        if (result.report_path) {
          setReportPath(result.report_path);
        }

        if (result.print_report_path) {
          setPrintReportPath(result.print_report_path);
        }

        if (result.state === "waiting") {
          return "waiting";
        }

        if (result.state !== "pass") {
          setFailureNotices((current) => ({
            ...current,
            [taskId]: failureNoticeFromResult(taskId, result, fromRunner),
          }));
          setRunnerActive(false);
        }

        return result.state;
      } catch (error) {
        const message = messageFromUnknownError(error);

        updateTaskState(taskId, "fail");
        setFailureNotices((current) => ({
          ...current,
          [taskId]: {
            taskId,
            title: "Processing Error",
            message,
            reportPath: reportPath || null,
            location: null,
            fromRunner,
          },
        }));
        setLastMessage(message);
        setRunnerActive(false);
        return "fail";
      } finally {
        processingTaskRef.current = false;
      }
    },
    [activateTask, ensureReportSetupReady, reportPath, setRunnerActive, unitFolder, updateTaskState],
  );

  const handleSkipTask = useCallback(
    (taskId: string) => {
      const notice = failureNotices[taskId];
      const task = allTaskOrder.find((item) => item.id === taskId);

      updateTaskState(taskId, "skipped");
      setFailureNotices((current) => {
        if (!current[taskId]) {
          return current;
        }

        const next = { ...current };
        delete next[taskId];
        return next;
      });

      if (!notice?.fromRunner) {
        setLastMessage(`${task?.label ?? "Step"} skipped`);
        return;
      }

      const nextTask = findNextTaskForRunner(
        allTaskOrder,
        taskStatesRef.current,
        processDetectedBacklogRef.current === true,
        latestTaskStatusesRef.current,
      );

      activateTask(nextTask?.id ?? null);
      setLastMessage(nextTask ? "Sequence running" : sequenceCompleteMessage());
      setRunnerActive(Boolean(nextTask));
    },
    [activateTask, allTaskOrder, failureNotices, sequenceCompleteMessage, setRunnerActive, updateTaskState],
  );

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
        setLastMessage("Press Start to set up reports before opening the report.");
        return;
      }

      const transformerSn = transformerSnDraftRef.current.trim();

      if (!transformerSn) {
        setLastMessage("Transformer SN is missing. Enter and save it before opening the report.");
        setTransformerSnSaveStatus("error");
        setTransformerSnError("Transformer SN is missing.");
        return;
      }

      if (transformerSn !== savedTransformerSnRef.current) {
        const saved = await saveTransformerSnDraft();

        if (!saved) {
          return;
        }
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
    [saveTransformerSnDraft, unitFolder],
  );

  useEffect(() => {
    if (!isRunning || !unitFolder) {
      return;
    }

    let cancelled = false;

    async function tick() {
      if (cancelled || processingTaskRef.current) {
        return;
      }

      while (!cancelled && isRunningRef.current) {
        const nextTask = findNextTaskForRunner(
          allTaskOrder,
          taskStatesRef.current,
          processDetectedBacklogRef.current === true,
          latestTaskStatusesRef.current,
        );

        if (!nextTask) {
          setIsRunning(false);
          activateTask(null);
          setLastMessage(sequenceCompleteMessage());
          return;
        }

        activateTask(nextTask.id);
        const state = taskStatesRef.current[nextTask.id] ?? nextTask.state;

        if (state === "detected" && shouldProcessDetectedCsv(nextTask.id, latestTaskStatusesRef.current[nextTask.id])) {
          const resultState = await runTask(nextTask.id, true);

          if (
            resultState === "pass" &&
            processDetectedBacklogRef.current === true &&
            !stopAfterCurrentTaskRef.current
          ) {
            continue;
          }

          if (stopAfterCurrentTaskRef.current) {
            stopAfterCurrentTaskRef.current = false;
            setRunnerActive(false);
            setLastMessage("Paused");
          }

          return;
        }

        updateTaskState(nextTask.id, "waiting");
        setLastMessage(`Waiting for ${nextTask.label} CSV`);

        try {
          const summary = await scanUnitFolder(unitFolder);

          if (cancelled || !summary) {
            return;
          }

          applyFolderSummary(summary);
          const latestTask = summary.tasks.find((task) => task.task_id === nextTask.id);
          const latestState = latestTask?.state;

          if (latestState === "detected" && shouldProcessDetectedCsv(nextTask.id, latestTask)) {
            const resultState = await runTask(nextTask.id, true);

            if (
              resultState === "pass" &&
              processDetectedBacklogRef.current === true &&
              !stopAfterCurrentTaskRef.current
            ) {
              continue;
            }

            if (stopAfterCurrentTaskRef.current) {
              stopAfterCurrentTaskRef.current = false;
              setRunnerActive(false);
              setLastMessage("Paused");
            }
          } else {
            updateTaskState(nextTask.id, "waiting");

            if (latestState === "detected") {
              setLastMessage(`Waiting for ${nextTask.label} CSV to finish`);
            }
          }

          return;
        } catch (error) {
          setRunnerActive(false);
          updateTaskState(nextTask.id, "fail");
          setLastMessage(messageFromUnknownError(error));
          return;
        }
      }
    }

    const handle = window.setInterval(() => void tick(), 3000);
    void tick();

    return () => {
      cancelled = true;
      window.clearInterval(handle);
    };
  }, [
    activateTask,
    allTaskOrder,
    applyFolderSummary,
    isRunning,
    runTask,
    sequenceCompleteMessage,
    setRunnerActive,
    unitFolder,
    updateTaskState,
  ]);

  const startSequence = useCallback(
    async (folder: string) => {
      stopAfterCurrentTaskRef.current = false;
      let shouldProcessBacklog = processDetectedBacklogRef.current;
      const promptDetectedCount =
        detectedTaskCountFromStates(taskStatesRef.current) || detectedCountRef.current;

      setResetClearsSelectionNext(false);

      if (!(await ensureReportSetupReady(folder))) {
        return;
      }

      if (shouldProcessBacklog === null && promptDetectedCount > 0) {
        shouldProcessBacklog = await requestBacklogChoice(promptDetectedCount);
        processDetectedBacklogRef.current = shouldProcessBacklog;
        setProcessDetectedBacklog(shouldProcessBacklog);
      }

      const nextTask = findNextTaskForRunner(
        allTaskOrder,
        taskStatesRef.current,
        shouldProcessBacklog === true,
        latestTaskStatusesRef.current,
      );

      activateTask(nextTask?.id ?? null);
      if (nextTask) {
        enableCurrentStepFollow();
      }
      setLastMessage(nextTask ? "Sequence running" : sequenceCompleteMessage());
      setRunnerActive(Boolean(nextTask));
    },
    [
      activateTask,
      allTaskOrder,
      enableCurrentStepFollow,
      ensureReportSetupReady,
      requestBacklogChoice,
      sequenceCompleteMessage,
      setRunnerActive,
    ],
  );

  async function handleChooseFolder() {
    setIsChoosingFolder(true);
    const selected = await chooseUnitFolder().finally(() => setIsChoosingFolder(false));

    if (!selected) {
      return;
    }

    setUnitFolder(selected);
    setSerialNumber(serialNumberFromFolder(selected));
    setRunnerActive(false);
    stopAfterCurrentTaskRef.current = false;
    setResetClearsSelectionNext(false);
    setRemainingSeconds(0);
    setReportPath("");
    setPrintReportPath("");
    setProcessDetectedBacklog(null);
    processDetectedBacklogRef.current = null;
    latestTaskStatusesRef.current = {};
    setFailureNotices({});
    setPrintReadinessBlockers([]);
    resetTransformerSnState();
    activateTask(null);
    selectedFolderRef.current = selected;
    setupPromiseRef.current = null;
    setupErrorRef.current = null;
    setConfirmedSetupFolder("");
    folderScanPendingRef.current = true;
    setIsScanningFolder(true);
    setIsSettingUpReports(false);
    setLastMessage("Scanning unit folder");

    try {
      const summary = await scanUnitFolder(selected);

      if (selectedFolderRef.current !== selected) {
        return;
      }

      folderScanPendingRef.current = false;
      setIsScanningFolder(false);

      if (!summary) {
        const folderName = selected.split(/[\\/]/).filter(Boolean).at(-1) ?? "";
        setSerialNumber(folderName.match(/\d{6,}/)?.[0] ?? "");
        setLastMessage("Ready to start");
        return;
      }

      applyFolderSummary(summary, true);
      setLastMessage(detectedReadyMessage(summary.detected_count));
    } catch (error) {
      if (selectedFolderRef.current !== selected) {
        return;
      }

      folderScanPendingRef.current = false;
      setIsScanningFolder(false);
      setTaskStates({});
      setFailureNotices({});
      setLastMessage(messageFromUnknownError(error));
    }
  }

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

    if (setupConfirmedFolderRef.current !== unitFolder) {
      const setupOk = await beginReportSetup(
        unitFolder,
        transformerSnDraft.trim(),
        serialNumber || serialNumberFromFolder(unitFolder),
        true,
      );

      if (!setupOk) {
        return;
      }
    } else if (transformerSnDraft.trim() && transformerSnDraft.trim() !== savedTransformerSnRef.current) {
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

  async function ensureTransformerSnReadyForReport(action: "opening" | "printing" = "opening") {
    const actionText = action === "printing" ? "printing the report" : "opening the report";

    if (!unitFolder || setupConfirmedFolderRef.current !== unitFolder) {
      setLastMessage(`Press Start to set up reports before ${actionText}.`);
      return false;
    }

    if (!transformerSnDraft.trim()) {
      setLastMessage(`Transformer SN is missing. Enter and save it before ${actionText}.`);
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
      setTaskStates({});
      setFailureNotices({});
      setExpandedIds(new Set());
      setRemainingSeconds(0);
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
    setRemainingSeconds(0);
    setProcessDetectedBacklog(null);
    processDetectedBacklogRef.current = null;
    setConfirmedSetupFolder("");
    if (transformerSnDraftRef.current.trim()) {
      setTransformerSnSaveStatus("dirty");
    }
    setCurrentStepFollowMode(false);
    latestTaskStatusesRef.current = {};
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

    if (!(await ensureTransformerSnReadyForReport())) {
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
      setLastMessage("Press Start to set up reports before printing the report.");
      return;
    }

    if (!printReportPath) {
      setLastMessage("No print report is available yet.");
      return;
    }

    if (!(await ensureTransformerSnReadyForReport("printing"))) {
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
      const message = "Press Start to set up reports before printing the report.";
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

      await saveFinalOperatorName(unitFolder, operatorName);
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
            : "Saves on Start"
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

  return (
    <main className="flex h-screen min-h-[400px] w-full min-w-[360px] max-w-full flex-col overflow-hidden bg-[#20201f] p-3.5 text-white">
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

      <section className="mt-2 min-h-0 flex-1 overflow-hidden">
        <div className="relative h-full">
          {scrollCue.top ? (
            <div className="pointer-events-none absolute inset-x-0 top-0 z-10 flex h-8 items-start justify-center bg-gradient-to-b from-[#20201f] to-transparent pt-1">
              <span className="h-0 w-0 border-x-[4px] border-b-[6px] border-x-transparent border-b-white/45" />
            </div>
          ) : null}
          <div
            ref={scrollRef}
            aria-label="Workflow steps"
            onScroll={handleWorkflowScroll}
            onTouchStart={handleWorkflowUserScrollIntent}
            onWheel={handleWorkflowUserScrollIntent}
            className="h-full overflow-y-auto [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
          >
            <div className="space-y-1.5 px-1 py-2">
              {panelItems.map((item) =>
                isSectionItem(item) ? (
                  <SectionBlock
                    key={item.id}
                    section={item}
                    expanded={expandedIds.has(item.id)}
                    onToggle={toggleSection}
                    isExpanded={(id) => expandedIds.has(id)}
                    currentTaskId={currentTaskId}
                    onRunTask={(taskId) => void runTask(taskId)}
                    failureNotices={failureNotices}
                    onSkipTask={handleSkipTask}
                    onOpenFailureLocation={(notice) => void handleOpenFailureLocation(notice)}
                  />
                ) : (
                  <TaskRow
                    key={item.id}
                    task={item}
                    currentTaskId={currentTaskId}
                    onRunTask={(taskId) => void runTask(taskId)}
                    failureNotice={failureNotices[item.id]}
                    onSkipTask={handleSkipTask}
                    onOpenFailureLocation={(notice) => void handleOpenFailureLocation(notice)}
                  />
                ),
              )}
              <div aria-label="Report actions" className="grid grid-cols-2 gap-1.5">
                <PanelButton
                  label="Open Report"
                  state="off"
                  onClick={() => void handleOpenReport()}
                />
                <PanelButton
                  label="Print Report"
                  state="off"
                  onClick={() => void handlePrintReportClick()}
                />
              </div>
            </div>
          </div>
          {scrollCue.bottom ? (
            <div className="pointer-events-none absolute inset-x-0 bottom-0 z-10 flex h-8 items-end justify-center bg-gradient-to-t from-[#20201f] to-transparent pb-1">
              <span className="h-0 w-0 border-x-[4px] border-t-[6px] border-x-transparent border-t-white/45" />
            </div>
          ) : null}
        </div>
      </section>

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
                Run Previous Tests
              </button>
              <button
                type="button"
                onClick={() => resolveBacklogPrompt(false)}
                className="inline-flex min-h-9 items-center justify-center rounded-md bg-[#3a3a38] px-3 py-2 text-[9pt] font-semibold text-white shadow-sm transition hover:bg-[#454542]"
              >
                Skip to Current Test
              </button>
            </div>
          </section>
        </div>
      ) : null}
    </main>
  );
}
