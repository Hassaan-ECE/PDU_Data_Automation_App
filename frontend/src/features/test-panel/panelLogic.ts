import type {
  BackendTaskStatus,
  FailureDetail,
  PrintReadinessBlocker,
  TaskProcessResult,
} from "@/integrations/tauri/backend";

import {
  isSectionItem,
  type BackendTaskStatusMap,
  type PanelItem,
  type TaskFailureNotice,
  type TaskItem,
  type TaskState,
} from "./types";

const DEFAULT_LOAD_DURATION_SECONDS = 3 * 60;
const TRANSFORMER_DURATION_SECONDS = 60;
const SYSTEM_BURN_IN_DURATION_SECONDS = 2 * 60 * 60;

export function printReadinessMessage(blockers: PrintReadinessBlocker[]) {
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

export function formatTime(seconds: number) {
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  const remainingSeconds = seconds % 60;

  return [hours, minutes, remainingSeconds].map((value) => value.toString().padStart(2, "0")).join(":");
}

export function flattenTasks(items: PanelItem[]): TaskItem[] {
  return items.flatMap((item) => (isSectionItem(item) ? flattenTasks(item.children) : [item]));
}

export function applyTaskStates(items: PanelItem[], states: Record<string, TaskState>): PanelItem[] {
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

export function findTaskPath(items: PanelItem[], taskId: string, path: string[] = []): string[] | null {
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

export function isTerminalState(state: TaskState | undefined) {
  return state === "pass" || state === "fail" || state === "skipped";
}

export function getSectionState(items: PanelItem[]): TaskState {
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

export function sectionProgress(items: PanelItem[]) {
  const tasks = flattenTasks(items);
  const completed = tasks.filter((task) => task.state === "pass" || task.state === "skipped").length;

  return `${completed}/${tasks.length}`;
}

export function findNextTaskForRunner(
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

    return state === "detected" && detectedCsvStillInProgress(backendTasks[task.id]);
  });

  if (inProgressDetectedTask) {
    return inProgressDetectedTask;
  }

  return tasks.find((task) => {
    const state = states[task.id] ?? task.state;

    return state === "off" || state === "waiting" || state === "warning";
  });
}

export function taskDurationSeconds(taskId: string) {
  if (taskId.endsWith("-transformer")) {
    return TRANSFORMER_DURATION_SECONDS;
  }

  if (taskId === "system-burn-in") {
    return SYSTEM_BURN_IN_DURATION_SECONDS;
  }

  return DEFAULT_LOAD_DURATION_SECONDS;
}

export function taskSecondsRemaining(task: BackendTaskStatus | undefined, nowMs: number) {
  if (!task || task.process_ready || task.wait_phase === "ready") {
    return 0;
  }

  if (task.phase_deadline_ms === null) {
    return task.pending_duration_seconds;
  }

  const deadlineSeconds = Math.max(0, Math.ceil((task.phase_deadline_ms - nowMs) / 1000));

  return deadlineSeconds + task.pending_duration_seconds;
}

export function readinessMessage(task: BackendTaskStatus) {
  switch (task.wait_phase) {
    case "soaking":
      return "System Burn-In STEP71 soak in progress";
    case "waiting_step72":
      return "Waiting for STEP72 burn-in capture";
    case "capturing":
      return "STEP72 burn-in capture stabilizing";
    case "waiting_unlock":
      return `Waiting for ${task.label} CSV to unlock`;
    case "timing":
      return `Waiting for ${task.label} CSV timer`;
    case "awaiting_csv":
      return `Waiting for ${task.label} CSV`;
    case "ready":
      return `${task.label} ready`;
  }
}

export function detectedCsvStillInProgress(task: BackendTaskStatus | undefined) {
  return task?.process_ready !== true;
}

export function shouldProcessDetectedCsv(task: BackendTaskStatus | undefined) {
  return task?.process_ready === true;
}

export function readyDetectedBacklogTaskIds(
  tasks: TaskItem[],
  states: Record<string, TaskState>,
  backendTasks: BackendTaskStatusMap,
) {
  return tasks
    .filter((task) => {
      const state = states[task.id] ?? task.state;

      return (
        (state === "detected" || state === "waiting") &&
        shouldProcessDetectedCsv(backendTasks[task.id])
      );
    })
    .map((task) => task.id);
}

export function backendTaskStatusMap(tasks: BackendTaskStatus[]) {
  return Object.fromEntries(tasks.map((task) => [task.task_id, task])) as BackendTaskStatusMap;
}

function taskRemainingSeconds(
  task: TaskItem,
  state: TaskState,
  backendTask: BackendTaskStatus | undefined,
  isCurrentTask: boolean,
  nowMs: number,
) {
  if (isTerminalState(state)) {
    return 0;
  }

  if (backendTask) {
    return taskSecondsRemaining(backendTask, nowMs);
  }

  if (isCurrentTask || state === "off" || state === "waiting" || state === "warning") {
    return taskDurationSeconds(task.id);
  }

  return 0;
}

export function remainingSecondsForTasks(
  tasks: TaskItem[],
  states: Record<string, TaskState>,
  backendTasks: BackendTaskStatusMap,
  currentTaskId: string | null,
  nowMs: number,
) {
  return tasks.reduce((total, task) => {
    const state = states[task.id] ?? task.state;

    return total + taskRemainingSeconds(
      task,
      state,
      backendTasks[task.id],
      task.id === currentTaskId,
      nowMs,
    );
  }, 0);
}

export function failureNoticeFromResult(
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

export function detectedReadyMessage(count: number) {
  return count > 0
    ? `${count} detected test${count === 1 ? "" : "s"} ready. Press Start to choose how to continue.`
    : "Ready to start";
}

export function detectedTaskCountFromStates(states: Record<string, TaskState>) {
  return Object.values(states).filter((state) => state === "detected").length;
}

export function serialNumberFromFolder(unitFolder: string) {
  const folderName = unitFolder.split(/[\\/]/).filter(Boolean).at(-1) ?? "";

  return folderName.match(/\d{6,}/)?.[0] ?? "";
}

export function messageFromUnknownError(error: unknown) {
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

export function detailedMessageFromUnknownError(error: unknown) {
  const message = messageFromUnknownError(error);

  if (error && typeof error === "object" && "details" in error) {
    const details = (error as { details?: unknown }).details;

    if (typeof details === "string" && details.trim() && !message.includes(details.trim())) {
      return `${message} ${details.trim()}`;
    }
  }

  return message;
}

export function resetButtonLabel({
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

export type PanelControlState = {
  kind: "idle" | "paused-away" | "paused-current" | "running-following" | "running-unfollowed";
  primaryAction: PrimaryControlAction;
  primaryLabel: string;
  secondaryAction: SecondaryControlAction;
  secondaryDisabled: boolean;
  secondaryLabel: string;
};

export function panelControlState({
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
