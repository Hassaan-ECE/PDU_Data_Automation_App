import {
  useCallback,
  useEffect,
  type Dispatch,
  type MutableRefObject,
  type SetStateAction,
} from "react";

import {
  acceptAutomationTaskFailure,
  listenAutomationTaskBatchProgress,
  processAutomationTask,
  processAutomationTasks,
  scanUnitFolder,
  type TaskProcessResult,
  type UnitFolderSummary,
} from "@/integrations/tauri/backend";

import {
  detectedTaskCountFromStates,
  findNextTaskForRunner,
  isWorkbookLockedError,
  messageFromUnknownError,
  readyDetectedBacklogTaskIds,
  runnerWaitingMessage,
  shouldRunnerContinueAfterResult,
  shouldProcessDetectedCsv,
} from "./panelLogic";
import type {
  BackendTaskStatusMap,
  TaskFailureNotice,
  TaskItem,
  TaskState,
} from "./types";

type TaskRunnerRefs = {
  taskStatesRef: MutableRefObject<Record<string, TaskState>>;
  latestTaskStatusesRef: MutableRefObject<BackendTaskStatusMap>;
  processDetectedBacklogRef: MutableRefObject<boolean | null>;
  stopAfterCurrentTaskRef: MutableRefObject<boolean>;
  processingTaskRef: MutableRefObject<boolean>;
  isRunningRef: MutableRefObject<boolean>;
  detectedCountRef: MutableRefObject<number>;
  heldFailureTaskIdRef: MutableRefObject<string | null>;
};

type TaskRunnerActions = {
  activateTask: (taskId: string | null) => void;
  updateTaskState: (taskId: string, state: TaskState) => void;
  setRunnerActive: (active: boolean) => void;
  applyTaskProcessResult: (
    result: TaskProcessResult,
    fromRunner: boolean,
    focusFailure?: boolean,
  ) => TaskState;
  applyFolderSummary: (summary: UnitFolderSummary, replace?: boolean) => void;
  setFailureNotices: Dispatch<SetStateAction<Record<string, TaskFailureNotice>>>;
  setLastMessage: (message: string) => void;
  setProcessDetectedBacklog: (value: boolean | null) => void;
  setResetClearsSelectionNext: (value: boolean) => void;
  sequenceCompleteMessage: () => string;
  requestBacklogChoice: (count: number) => Promise<boolean | null>;
  ensureReportSetupReady: (folder: string) => Promise<boolean>;
  enableCurrentStepFollow: () => void;
  focusTaskForAttention: (taskId: string) => void;
  requestWorkbookClose: (path: string, message: string) => Promise<boolean>;
};

type UseTaskRunnerProps = {
  unitFolder: string;
  reportPath: string;
  allTaskOrder: TaskItem[];
  failureNotices: Record<string, TaskFailureNotice>;
  isRunning: boolean;
  refs: TaskRunnerRefs;
  actions: TaskRunnerActions;
};

export function useTaskRunner({
  unitFolder,
  reportPath,
  allTaskOrder,
  failureNotices,
  isRunning,
  refs,
  actions,
}: UseTaskRunnerProps) {
  const {
    taskStatesRef,
    latestTaskStatusesRef,
    processDetectedBacklogRef,
    stopAfterCurrentTaskRef,
    processingTaskRef,
    isRunningRef,
    detectedCountRef,
    heldFailureTaskIdRef,
  } = refs;
  const {
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
  } = actions;
  const runWithWorkbookCloseRetry = useCallback(
    async <Result,>(operation: () => Promise<Result>): Promise<Result> => {
      try {
        return await operation();
      } catch (error) {
        if (!isWorkbookLockedError(error)) {
          throw error;
        }

        const shouldRetry = await requestWorkbookClose(reportPath, messageFromUnknownError(error));
        if (!shouldRetry) {
          throw error;
        }

        return operation();
      }
    },
    [reportPath, requestWorkbookClose],
  );
  const releaseHeldFailure = useCallback(
    (taskId: string) => {
      if (heldFailureTaskIdRef.current !== taskId) {
        return;
      }

      const nextFailure = allTaskOrder.find(
        (task) => task.id !== taskId && Boolean(failureNotices[task.id]),
      );
      heldFailureTaskIdRef.current = nextFailure?.id ?? null;

      if (nextFailure) {
        focusTaskForAttention(nextFailure.id);
      }
    },
    [allTaskOrder, failureNotices, focusTaskForAttention, heldFailureTaskIdRef],
  );
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
      if (!fromRunner || !heldFailureTaskIdRef.current) {
        activateTask(taskId);
      }
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
        const result: TaskProcessResult | null = await runWithWorkbookCloseRetry(() =>
          processAutomationTask(unitFolder, taskId),
        );

        if (!result) {
          updateTaskState(taskId, "pass");
          releaseHeldFailure(taskId);
          setLastMessage("Mock task processed");
          return "pass";
        }

        const resultState = applyTaskProcessResult(result, fromRunner);
        if (result.state === "pass") {
          releaseHeldFailure(taskId);
        }

        return shouldRunnerContinueAfterResult(result, fromRunner) ? "pass" : resultState;
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
        if (!heldFailureTaskIdRef.current) {
          heldFailureTaskIdRef.current = taskId;
          focusTaskForAttention(taskId);
        }
        setLastMessage(message);
        setRunnerActive(false);
        return "fail";
      } finally {
        processingTaskRef.current = false;
      }
    },
    [
      activateTask,
      applyTaskProcessResult,
      ensureReportSetupReady,
      focusTaskForAttention,
      heldFailureTaskIdRef,
      isRunningRef,
      processingTaskRef,
      releaseHeldFailure,
      reportPath,
      runWithWorkbookCloseRetry,
      setFailureNotices,
      setLastMessage,
      setRunnerActive,
      unitFolder,
      updateTaskState,
    ],
  );

  const runTaskBatch = useCallback(
    async (taskIds: string[]): Promise<TaskState | null> => {
      if (!unitFolder || processingTaskRef.current || taskIds.length === 0) {
        return null;
      }

      if (!(await ensureReportSetupReady(unitFolder))) {
        return null;
      }

      processingTaskRef.current = true;
      const firstTaskId = taskIds[0];
      const taskIdSet = new Set(taskIds);
      const previousStates = Object.fromEntries(
        taskIds.map((taskId) => [taskId, taskStatesRef.current[taskId] ?? "detected"]),
      ) as Record<string, TaskState>;
      let unlistenBatchProgress: (() => void) | null = null;

      setFailureNotices((current) => {
        let changed = false;
        const next = { ...current };

        for (const taskId of taskIds) {
          if (next[taskId]) {
            delete next[taskId];
            changed = true;
          }
        }

        return changed ? next : current;
      });
      for (const taskId of taskIds) {
        updateTaskState(taskId, "processing");
      }
      setLastMessage(`Batch processing ${taskIds.length} previous test${taskIds.length === 1 ? "" : "s"}`);

      try {
        try {
          unlistenBatchProgress = await listenAutomationTaskBatchProgress((progress) => {
            if (progress.unit_folder !== unitFolder || !taskIdSet.has(progress.task_id)) {
              return;
            }

            updateTaskState(progress.task_id, progress.state);
            setLastMessage(
              progress.state === "processing"
                ? `${progress.index}/${progress.total}: ${progress.message}`
                : `${progress.index}/${progress.total} committed: ${progress.message}`,
            );
          });
        } catch {
          unlistenBatchProgress = null;
        }

        const batch = await runWithWorkbookCloseRetry(() =>
          processAutomationTasks(unitFolder, taskIds),
        );

        if (!batch) {
          for (const taskId of taskIds) {
            updateTaskState(taskId, "pass");
          }
          setLastMessage(`Mock batch processed ${taskIds.length} task${taskIds.length === 1 ? "" : "s"}`);
          return "pass";
        }

        let lastState: TaskState = "pass";
        const returnedTaskIds = new Set(batch.results.map((result) => result.task_id));

        for (const result of batch.results) {
          lastState = applyTaskProcessResult(result, true, false);
        }

        for (const taskId of taskIds) {
          if (!returnedTaskIds.has(taskId)) {
            updateTaskState(taskId, previousStates[taskId] ?? "detected");
          }
        }

        const lastResult = batch.results.at(-1);
        const firstFailure = batch.results.find(
          (result) => result.state !== "pass" && result.state !== "waiting",
        );
        const continuableFailure = batch.results.find((result) =>
          shouldRunnerContinueAfterResult(result, true),
        );

        if (firstFailure) {
          if (!heldFailureTaskIdRef.current) {
            heldFailureTaskIdRef.current = firstFailure.task_id;
            focusTaskForAttention(firstFailure.task_id);
          }
        }

        if (batch.stopped_task_id) {
          if (lastResult) {
            setLastMessage(lastResult.message);
          }
          return lastState;
        }

        setLastMessage(batch.message);
        return continuableFailure ? "warning" : "pass";
      } catch (error) {
        const message = messageFromUnknownError(error);

        updateTaskState(firstTaskId, "fail");
        for (const taskId of taskIds.slice(1)) {
          updateTaskState(taskId, previousStates[taskId] ?? "detected");
        }
        setFailureNotices((current) => ({
          ...current,
          [firstTaskId]: {
            taskId: firstTaskId,
            title: "Processing Error",
            message,
            reportPath: reportPath || null,
            location: null,
            fromRunner: true,
          },
        }));
        if (!heldFailureTaskIdRef.current) {
          heldFailureTaskIdRef.current = firstTaskId;
          focusTaskForAttention(firstTaskId);
        }
        setLastMessage(message);
        setRunnerActive(false);
        return "fail";
      } finally {
        unlistenBatchProgress?.();
        processingTaskRef.current = false;
      }
    },
    [
      applyTaskProcessResult,
      ensureReportSetupReady,
      focusTaskForAttention,
      heldFailureTaskIdRef,
      processingTaskRef,
      reportPath,
      runWithWorkbookCloseRetry,
      setFailureNotices,
      setLastMessage,
      setRunnerActive,
      taskStatesRef,
      unitFolder,
      updateTaskState,
    ],
  );

  const handlePassTask = useCallback(
    async (taskId: string) => {
      const task = allTaskOrder.find((item) => item.id === taskId);

      if (!unitFolder) {
        setLastMessage("No unit folder is selected");
        return;
      }

      try {
        const summary = await acceptAutomationTaskFailure(unitFolder, taskId);

        if (summary) {
          applyFolderSummary(summary);
        }

        updateTaskState(taskId, "pass");
        setFailureNotices((current) => {
          if (!current[taskId]) {
            return current;
          }

          const next = { ...current };
          delete next[taskId];
          return next;
        });
        releaseHeldFailure(taskId);
        setLastMessage(`${task?.label ?? "Step"} accepted as pass`);
      } catch (error) {
        setLastMessage(messageFromUnknownError(error));
      }
    },
    [
      allTaskOrder,
      applyFolderSummary,
      releaseHeldFailure,
      setFailureNotices,
      setLastMessage,
      unitFolder,
      updateTaskState,
    ],
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
        if (processDetectedBacklogRef.current === true) {
          const backlogTaskIds = readyDetectedBacklogTaskIds(
            allTaskOrder,
            taskStatesRef.current,
            latestTaskStatusesRef.current,
          );

          if (backlogTaskIds.length > 0) {
            const resultState = await runTaskBatch(backlogTaskIds);

            if (
              (resultState === "pass" || resultState === "warning") &&
              !stopAfterCurrentTaskRef.current
            ) {
              const hasDetectedBacklogRemaining = allTaskOrder.some((task) => {
                const state = taskStatesRef.current[task.id] ?? task.state;

                return state === "detected" || state === "waiting";
              });

              if (!hasDetectedBacklogRemaining) {
                processDetectedBacklogRef.current = false;
                setProcessDetectedBacklog(false);
              }

              if (resultState === "pass") {
                continue;
              }

              return;
            }

            if (stopAfterCurrentTaskRef.current) {
              stopAfterCurrentTaskRef.current = false;
              setRunnerActive(false);
              setLastMessage("Paused");
            }

            return;
          }
        }

        const nextTask = findNextTaskForRunner(
          allTaskOrder,
          taskStatesRef.current,
          processDetectedBacklogRef.current === true,
          latestTaskStatusesRef.current,
        );

        if (!nextTask) {
          setRunnerActive(false);
          if (!heldFailureTaskIdRef.current) {
            activateTask(null);
          }
          setLastMessage(sequenceCompleteMessage());
          return;
        }

        if (!heldFailureTaskIdRef.current) {
          activateTask(nextTask.id);
        }
        const state = taskStatesRef.current[nextTask.id] ?? nextTask.state;

        if (
          (state === "detected" || state === "waiting") &&
          shouldProcessDetectedCsv(latestTaskStatusesRef.current[nextTask.id])
        ) {
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
        setLastMessage(
          runnerWaitingMessage(latestTaskStatusesRef.current[nextTask.id], nextTask.label),
        );

        try {
          const summary = await scanUnitFolder(unitFolder);

          if (cancelled || !summary) {
            return;
          }

          applyFolderSummary(summary);
          const latestTask = summary.tasks.find((task) => task.task_id === nextTask.id);
          const latestState = latestTask?.state;

          if (
            (latestState === "detected" || latestState === "waiting") &&
            shouldProcessDetectedCsv(latestTask)
          ) {
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
            setLastMessage(runnerWaitingMessage(latestTask, nextTask.label));
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
    // The omitted refs are stable useRef objects from OperatorPanel; ticks read their current values.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [
    activateTask,
    allTaskOrder,
    applyFolderSummary,
    isRunning,
    runTaskBatch,
    runTask,
    sequenceCompleteMessage,
    setLastMessage,
    setProcessDetectedBacklog,
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
        const backlogChoice = await requestBacklogChoice(promptDetectedCount);

        if (backlogChoice === null) {
          setLastMessage("Start canceled");
          return;
        }

        shouldProcessBacklog = backlogChoice;
        processDetectedBacklogRef.current = shouldProcessBacklog;
        setProcessDetectedBacklog(shouldProcessBacklog);
      }

      const nextTask = findNextTaskForRunner(
        allTaskOrder,
        taskStatesRef.current,
        shouldProcessBacklog === true,
        latestTaskStatusesRef.current,
      );
      const batchBacklogRun = shouldProcessBacklog === true;

      if (!heldFailureTaskIdRef.current) {
        activateTask(batchBacklogRun ? null : nextTask?.id ?? null);
        if (nextTask && !batchBacklogRun) {
          enableCurrentStepFollow();
        }
      }
      setLastMessage(
        nextTask
          ? batchBacklogRun
            ? "Batch previous tests queued"
            : "Sequence running"
          : sequenceCompleteMessage(),
      );
      setRunnerActive(Boolean(nextTask));
    },
    [
      activateTask,
      allTaskOrder,
      detectedCountRef,
      enableCurrentStepFollow,
      ensureReportSetupReady,
      heldFailureTaskIdRef,
      latestTaskStatusesRef,
      processDetectedBacklogRef,
      requestBacklogChoice,
      sequenceCompleteMessage,
      setLastMessage,
      setProcessDetectedBacklog,
      setResetClearsSelectionNext,
      setRunnerActive,
      stopAfterCurrentTaskRef,
      taskStatesRef,
    ],
  );

  return {
    runTask,
    startSequence,
    handlePassTask,
  };
}
