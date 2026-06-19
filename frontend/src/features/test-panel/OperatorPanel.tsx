import type { ReactNode } from "react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { ExternalLink, RotateCcw, SkipForward } from "lucide-react";

import {
  chooseUnitFolder,
  getBackendStatus,
  loadLayoutProfile,
  openReportLocation,
  openReportPath,
  processAutomationTask,
  scanUnitFolder,
  setupUnitFolder,
  type BackendStatus,
  type FailureDetail,
  type FailureLocation,
  type LayoutLoadResponse,
  type TaskProcessResult,
  type UnitFolderSummary,
} from "@/integrations/tauri/backend";
import { cn } from "@/shared/lib/utils";

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

type TaskFailureNotice = {
  taskId: string;
  title: string;
  message: string;
  reportPath: string | null;
  location: FailureLocation | null;
  fromRunner: boolean;
};

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
) {
  if (processDetectedBacklog) {
    const detectedTask = tasks.find((task) => (states[task.id] ?? task.state) === "detected");

    if (detectedTask) {
      return detectedTask;
    }
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

function remainingSecondsForStates(tasks: TaskItem[], states: Record<string, TaskState>) {
  return tasks.reduce((total, task) => {
    const state = states[task.id] ?? task.state;

    return state === "off" ? total + taskDurationSeconds(task.id) : total;
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

function panelDepthWidth(depth: number) {
  if (depth <= 0) {
    return "mx-auto w-[calc(100%_-_0.5rem)]";
  }

  if (depth === 1) {
    return "mx-auto w-[88%]";
  }

  return "mx-auto w-[76%]";
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
        "group relative flex min-h-9 max-w-full items-center justify-center rounded-md px-8 py-2 text-center shadow-sm transition",
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
  const processingTaskRef = useRef(false);
  const isRunningRef = useRef(false);
  const stopAfterCurrentTaskRef = useRef(false);
  const taskStatesRef = useRef<Record<string, TaskState>>({});
  const processDetectedBacklogRef = useRef<boolean | null>(null);
  const allTaskOrder = useMemo(() => flattenTasks(legacyPanelItems), []);
  const [unitFolder, setUnitFolder] = useState("");
  const [serialNumber, setSerialNumber] = useState("");
  const [remainingSeconds, setRemainingSeconds] = useState(0);
  const [isRunning, setIsRunning] = useState(false);
  const [currentTaskId, setCurrentTaskId] = useState<string | null>(null);
  const [expandedIds, setExpandedIds] = useState<Set<string>>(() => new Set());
  const [backendStatus, setBackendStatus] = useState<BackendStatus | null>(null);
  const [layoutProfile, setLayoutProfile] = useState<LayoutLoadResponse | null>(null);
  const [scrollCue, setScrollCue] = useState({ top: false, bottom: false });
  const [taskStates, setTaskStates] = useState<Record<string, TaskState>>({});
  const [failureNotices, setFailureNotices] = useState<Record<string, TaskFailureNotice>>({});
  const [processDetectedBacklog, setProcessDetectedBacklog] = useState<boolean | null>(null);
  const [reportPath, setReportPath] = useState("");
  const [detectedCount, setDetectedCount] = useState(0);
  const [lastMessage, setLastMessage] = useState("");
  const [setupWarnings, setSetupWarnings] = useState<string[]>([]);
  const [backlogPrompt, setBacklogPrompt] = useState<BacklogPromptState>(null);
  const [resetClearsSelectionNext, setResetClearsSelectionNext] = useState(false);
  const appVersion = backendStatus?.version ?? "0.1.0";
  const panelItems = useMemo(() => applyTaskStates(legacyPanelItems, taskStates), [taskStates]);
  const detectedTaskCount = useMemo(
    () => Object.values(taskStates).filter((state) => state === "detected").length,
    [taskStates],
  );
  const announceUpdateStatus = useCallback((message: string) => setLastMessage(message), []);
  const { handleUpdateAction, updateState } = useDesktopUpdates({
    announceStatus: announceUpdateStatus,
    currentVersion: appVersion,
  });

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
    void getBackendStatus().then(setBackendStatus);
    void loadLayoutProfile().then(setLayoutProfile);
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

  useEffect(() => {
    if (!currentTaskId) {
      return;
    }

    window.requestAnimationFrame(() => {
      const currentElement = scrollRef.current?.querySelector('[data-current-task="true"]');

      currentElement?.scrollIntoView({
        behavior: "smooth",
        block: "center",
      });
    });
  }, [currentTaskId, expandedIds]);

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

      if (taskId) {
        expandForTask(taskId);
      }
    },
    [expandForTask],
  );

  const replaceTaskStates = useCallback(
    (states: Record<string, TaskState>) => {
      taskStatesRef.current = states;
      setTaskStates(states);
      setRemainingSeconds(remainingSecondsForStates(allTaskOrder, states));
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
    setDetectedCount(summary.detected_count);
    setSetupWarnings(summary.warnings);

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

  const runTask = useCallback(
    async (taskId: string, fromRunner = false): Promise<TaskState | null> => {
      if (!unitFolder || processingTaskRef.current) {
        return null;
      }

      if (!fromRunner && isRunningRef.current) {
        setLastMessage("Pause the runner before rerunning an individual task");
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

        if (result.state !== "pass") {
          setFailureNotices((current) => ({
            ...current,
            [taskId]: failureNoticeFromResult(taskId, result, fromRunner),
          }));
          setRunnerActive(false);
        }

        return result.state;
      } catch (error) {
        const message = error instanceof Error ? error.message : String(error);

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
    [activateTask, reportPath, setRunnerActive, unitFolder, updateTaskState],
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
      );

      activateTask(nextTask?.id ?? null);
      setLastMessage(nextTask ? "Sequence running" : "Sequence complete");
      setRunnerActive(Boolean(nextTask));
    },
    [activateTask, allTaskOrder, failureNotices, setRunnerActive, updateTaskState],
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
        setLastMessage(error instanceof Error ? error.message : String(error));
      }
    },
    [unitFolder],
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
        );

        if (!nextTask) {
          setIsRunning(false);
          activateTask(null);
          setLastMessage("Sequence complete");
          return;
        }

        activateTask(nextTask.id);
        const state = taskStatesRef.current[nextTask.id] ?? nextTask.state;

        if (state === "detected") {
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
        setRemainingSeconds((current) => (current > 0 ? current : taskDurationSeconds(nextTask.id)));
        setLastMessage(`Waiting for ${nextTask.label} CSV`);

        try {
          const summary = await scanUnitFolder(unitFolder);

          if (cancelled || !summary) {
            return;
          }

          applyFolderSummary(summary);
          const latestState = summary.tasks.find((task) => task.task_id === nextTask.id)?.state;

          if (latestState === "detected") {
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
          }

          return;
        } catch (error) {
          setRunnerActive(false);
          updateTaskState(nextTask.id, "fail");
          setLastMessage(error instanceof Error ? error.message : String(error));
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
  }, [activateTask, allTaskOrder, applyFolderSummary, isRunning, runTask, setRunnerActive, unitFolder, updateTaskState]);

  async function handleChooseFolder() {
    const selected = await chooseUnitFolder();

    if (!selected) {
      return;
    }

    setUnitFolder(selected);
    setRunnerActive(false);
    stopAfterCurrentTaskRef.current = false;
    setResetClearsSelectionNext(false);
    setRemainingSeconds(0);
    setProcessDetectedBacklog(null);
    processDetectedBacklogRef.current = null;
    setFailureNotices({});
    activateTask(null);
    setLastMessage("Setting up unit folder");

    try {
      const summary = await setupUnitFolder(selected);

      if (!summary) {
        const folderName = selected.split(/[\\/]/).filter(Boolean).at(-1) ?? "";
        setSerialNumber(folderName.match(/\d{6,}/)?.[0] ?? "");
        setLastMessage("Ready to start");
        return;
      }

      applyFolderSummary(summary, true);
      setLastMessage(
        summary.detected_count > 0
          ? `${summary.detected_count} detected test${summary.detected_count === 1 ? "" : "s"} ready`
          : "Ready to start",
      );

      if (summary.detected_count > 0) {
        setLastMessage(
          `${summary.detected_count} detected test${summary.detected_count === 1 ? "" : "s"} ready. Press Start to choose how to continue.`,
        );
      }
    } catch (error) {
      setTaskStates({});
      setFailureNotices({});
      setLastMessage(error instanceof Error ? error.message : String(error));
    }
  }

  async function handleRunClick() {
    if (!unitFolder) {
      void handleChooseFolder();
      return;
    }

    const nextRunning = !isRunningRef.current;

    if (nextRunning) {
      stopAfterCurrentTaskRef.current = false;
      let shouldProcessBacklog = processDetectedBacklogRef.current;
      const promptDetectedCount = detectedTaskCount || detectedCount;

      setResetClearsSelectionNext(false);

      if (shouldProcessBacklog === null && promptDetectedCount > 0) {
        shouldProcessBacklog = await requestBacklogChoice(promptDetectedCount);
        processDetectedBacklogRef.current = shouldProcessBacklog;
        setProcessDetectedBacklog(shouldProcessBacklog);
      }

      const nextTask = findNextTaskForRunner(
        allTaskOrder,
        taskStatesRef.current,
        shouldProcessBacklog === true,
      );

      activateTask(nextTask?.id ?? null);
      setLastMessage(nextTask ? "Sequence running" : "Sequence complete");
      setRunnerActive(Boolean(nextTask));
    } else {
      if (processingTaskRef.current) {
        stopAfterCurrentTaskRef.current = true;
        setLastMessage("Pausing after current step");
      } else {
        stopAfterCurrentTaskRef.current = false;
        setRunnerActive(false);
        setLastMessage("Paused");
      }
    }
  }

  async function handleResetPanel() {
    if (!unitFolder) {
      setTaskStates({});
      setFailureNotices({});
      setRemainingSeconds(0);
      setLastMessage("");
      return;
    }

    if (resetClearsSelectionNext && !isRunningRef.current && !processingTaskRef.current) {
      setUnitFolder("");
      setSerialNumber("");
      setReportPath("");
      setDetectedCount(0);
      setSetupWarnings([]);
      setFailureNotices({});
      replaceTaskStates({});
      setProcessDetectedBacklog(null);
      processDetectedBacklogRef.current = null;
      setResetClearsSelectionNext(false);
      setLastMessage("");
      return;
    }

    stopAfterCurrentTaskRef.current = false;
    setRunnerActive(false);
    setRemainingSeconds(0);
    setProcessDetectedBacklog(null);
    processDetectedBacklogRef.current = null;
    setFailureNotices({});
    activateTask(null);
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
      setLastMessage(error instanceof Error ? error.message : String(error));
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
      setLastMessage(error instanceof Error ? error.message : String(error));
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
  }

  const footerText = setupWarnings.length
    ? setupWarnings[0]
    : (detectedTaskCount || detectedCount) > 0 && !isRunning
      ? `${detectedTaskCount || detectedCount} detected task${(detectedTaskCount || detectedCount) === 1 ? "" : "s"}`
      : layoutProfile?.validation.warnings.length
        ? `${layoutProfile.display_name} - ${layoutProfile.validation.warnings.length} config warning`
        : backendStatus
          ? "Ready."
          : "Ready.";
  const appVersionText = `v${appVersion}`;

  return (
    <main className="flex h-screen min-h-[400px] w-full min-w-[360px] max-w-full flex-col overflow-hidden bg-[#20201f] p-3.5 text-white">
      <section className="px-1 py-2">
        <div className="text-center text-[26pt] font-bold leading-none tracking-normal text-white">
          {formatTime(remainingSeconds)}
        </div>
        <div className="mt-1 truncate text-center text-[8.5pt] leading-tight text-[#d8d2c8]">{statusText}</div>
      </section>

      <section className="mt-1 rounded-md border border-[#454542] bg-[#292928] p-1.5">
        <div className="flex gap-1.5">
          <input
            readOnly
            value={unitFolder}
            placeholder="Select a unit test folder..."
            className="h-7 min-w-0 flex-1 basis-0 rounded border border-[#454542] bg-[#1f1f1e] px-2 text-[7.5pt] text-white placeholder:text-[#b7b1a8] outline-none"
          />
          <button
            type="button"
            onClick={handleChooseFolder}
            className="inline-flex h-7 shrink-0 items-center justify-center rounded bg-[#3a3a38] px-2 text-[7.5pt] font-medium text-white shadow-sm hover:bg-[#454542]"
          >
            Browse...
          </button>
        </div>
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
            onScroll={updateScrollCue}
            className="h-full overflow-y-auto [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
          >
            <div className="space-y-1.5 px-2 py-2">
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
              <PanelButton
                label="Open Report"
                state="off"
                onClick={() => void handleOpenReport()}
              />
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
        {serialNumber ? `SN ${serialNumber} | ${footerText}` : footerText}
      </div>

      <div className="grid grid-cols-2 gap-2">
        <button
          type="button"
          onClick={() => void handleRunClick()}
          className={cn(
            "inline-flex min-h-9 items-center justify-center gap-1.5 rounded-md px-3 py-2 text-[9pt] font-semibold shadow-sm transition",
            isRunning
              ? "bg-[#9b630c] text-white hover:bg-[#9b630c]"
              : "bg-[#1d7f47] text-white hover:bg-[#1d7f46]",
          )}
        >
          {isRunning ? "Pause" : "Start"}
        </button>
        <button
          type="button"
          onClick={() => void handleResetPanel()}
          className="inline-flex min-h-9 items-center justify-center gap-1.5 rounded-md bg-[#3a3a38] px-3 py-2 text-[9pt] font-semibold text-white shadow-sm transition hover:bg-[#454542]"
        >
          Reset Panel
        </button>
      </div>

      <footer className="mt-2 border-t border-[#454542] pt-2 text-[7.5pt] leading-tight text-[#d8d2c8]">
        <div className="flex items-center justify-between gap-3">
          <span>{appVersionText}</span>
          <span className="font-medium">Built by Syed Hassaan Shah</span>
        </div>
      </footer>

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
