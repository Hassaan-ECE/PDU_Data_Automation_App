import type { ReactNode } from "react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import {
  chooseUnitFolder,
  getBackendStatus,
  loadExampleLayoutProfile,
  openReportPath,
  type BackendStatus,
  type LayoutLoadResponse,
} from "@/integrations/tauri/backend";
import { cn } from "@/shared/lib/utils";

import { stateStyles } from "./stateStyles";
import { legacyPanelItems } from "./taskModel";
import { isSectionItem, type PanelItem, type SectionItem, type TaskItem, type TaskState } from "./types";

function formatElapsed(seconds: number) {
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  const remainingSeconds = seconds % 60;

  return [hours, minutes, remainingSeconds].map((value) => value.toString().padStart(2, "0")).join(":");
}

function flattenTasks(items: PanelItem[]): TaskItem[] {
  return items.flatMap((item) => (isSectionItem(item) ? flattenTasks(item.children) : [item]));
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
  const completed = tasks.filter((task) => task.state === "pass").length;

  return `${completed}/${tasks.length}`;
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
  const depthWidth =
    depth <= 0
      ? "w-full"
      : depth === 1
        ? "mx-auto w-[92%]"
        : "mx-auto w-[78%]";

  return (
    <button
      type="button"
      aria-label={label}
      onClick={onClick}
      className={cn(
        "group relative flex min-h-9 items-center justify-center overflow-hidden rounded-md px-8 py-2 text-center shadow-sm transition",
        "focus:outline-none focus:ring-2 focus:ring-cyan-200/25",
        depthWidth,
        current && "ring-2 ring-cyan-200/65 ring-offset-2 ring-offset-[#20201f]",
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

function TaskRow({
  task,
  currentTaskId,
  depth = 0,
}: {
  task: TaskItem;
  currentTaskId: string | null;
  depth?: number;
}) {
  return (
    <div>
      <PanelButton
        label={task.label}
        state={task.state}
        depth={depth}
        current={task.id === currentTaskId}
      />
    </div>
  );
}

function SectionBlock({
  section,
  expanded,
  onToggle,
  isExpanded,
  currentTaskId,
  depth = 0,
}: {
  section: SectionItem;
  expanded: boolean;
  onToggle: (id: string) => void;
  isExpanded: (id: string) => boolean;
  currentTaskId: string | null;
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
                depth={depth + 1}
              />
            ) : (
              <TaskRow key={item.id} task={item} currentTaskId={currentTaskId} depth={depth + 1} />
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
  const allTasks = useMemo(() => flattenTasks(legacyPanelItems), []);
  const [unitFolder, setUnitFolder] = useState("");
  const [serialNumber, setSerialNumber] = useState("");
  const [elapsedSeconds, setElapsedSeconds] = useState(0);
  const [isRunning, setIsRunning] = useState(false);
  const [currentTaskId, setCurrentTaskId] = useState<string | null>(null);
  const [expandedIds, setExpandedIds] = useState<Set<string>>(() => new Set());
  const [backendStatus, setBackendStatus] = useState<BackendStatus | null>(null);
  const [layoutProfile, setLayoutProfile] = useState<LayoutLoadResponse | null>(null);
  const [scrollCue, setScrollCue] = useState({ top: false, bottom: false });

  const statusText = useMemo(() => {
    if (!unitFolder) {
      return "No unit folder selected";
    }

    if (isRunning) {
      return "Sequence running";
    }

    return "Ready to start";
  }, [isRunning, unitFolder]);

  useEffect(() => {
    void getBackendStatus().then(setBackendStatus);
    void loadExampleLayoutProfile().then(setLayoutProfile);
  }, []);

  useEffect(() => {
    if (!isRunning) {
      return;
    }

    const handle = window.setInterval(() => setElapsedSeconds((value) => value + 1), 1000);

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
  }, [expandedIds, updateScrollCue]);

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

  useEffect(() => {
    if (!isRunning || allTasks.length === 0) {
      return;
    }

    const handle = window.setInterval(() => {
      const currentId = currentTaskIdRef.current;
      const currentIndex = Math.max(
        0,
        allTasks.findIndex((task) => task.id === currentId),
      );
      const nextIndex = Math.min(currentIndex + 1, allTasks.length - 1);
      activateTask(allTasks[nextIndex].id);
    }, 3000);

    return () => window.clearInterval(handle);
  }, [activateTask, allTasks, isRunning]);

  async function handleChooseFolder() {
    const selected = await chooseUnitFolder();

    if (!selected) {
      return;
    }

    setUnitFolder(selected);
    const folderName = selected.split(/[\\/]/).filter(Boolean).at(-1) ?? "";
    setSerialNumber(folderName.match(/\d{6,}/)?.[0] ?? "");
  }

  function handleRunClick() {
    const nextRunning = !isRunning;

    if (nextRunning) {
      activateTask(currentTaskIdRef.current ?? allTasks[0]?.id ?? null);
    }

    setIsRunning(nextRunning);
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

  const reportPath =
    unitFolder && serialNumber
      ? `${unitFolder}\\PDUD500442AM088_Test Report_0.2CT_Rev02_SN${serialNumber}.xlsx`
      : "";

  return (
    <main className="flex h-screen min-h-[400px] w-screen min-w-[360px] flex-col bg-[#20201f] p-3.5 text-white">
      <section className="px-1 py-2">
        <div className="text-center text-[26pt] font-bold leading-none tracking-normal text-white">
          {formatElapsed(elapsedSeconds)}
        </div>
        <div className="mt-1 truncate text-center text-[8.5pt] leading-tight text-zinc-300">{statusText}</div>
      </section>

      <section className="mt-1 rounded-md border border-white/10 bg-[#292928] p-1.5">
        <div className="flex gap-1.5">
          <input
            readOnly
            value={unitFolder}
            placeholder="Select a unit test folder..."
            className="h-7 min-w-0 flex-1 rounded bg-[#1f1f1e] px-2 text-[7.5pt] text-white placeholder:text-zinc-500 outline-none"
          />
          <button
            type="button"
            onClick={handleChooseFolder}
            className="inline-flex h-7 items-center justify-center rounded bg-[#3a3a38] px-2 text-[7.5pt] font-medium text-white shadow-sm hover:bg-[#454542]"
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
            <div className="space-y-1.5 px-0.5 pb-1">
            {legacyPanelItems.map((item) =>
              isSectionItem(item) ? (
                <SectionBlock
                  key={item.id}
                section={item}
                expanded={expandedIds.has(item.id)}
                onToggle={toggleSection}
                isExpanded={(id) => expandedIds.has(id)}
                currentTaskId={currentTaskId}
              />
            ) : (
                <TaskRow key={item.id} task={item} currentTaskId={currentTaskId} />
              ),
            )}
            <PanelButton
              label="Open Report"
              state="off"
              onClick={() => void openReportPath(reportPath)}
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

      <div className="px-1 py-1.5 text-[8.5pt] leading-tight text-zinc-400">
        {layoutProfile?.validation.warnings.length
          ? `${layoutProfile.display_name} - ${layoutProfile.validation.warnings.length} config warning`
          : backendStatus
            ? `Ready. v${backendStatus.version}`
            : "Ready."}
      </div>

      <div className="grid grid-cols-2 gap-2">
        <button
          type="button"
          onClick={handleRunClick}
          className={cn(
            "inline-flex min-h-9 items-center justify-center gap-1.5 rounded-md px-3 py-2 text-[9pt] font-semibold shadow-sm transition",
            isRunning
              ? "bg-[#51452b] text-amber-50 hover:bg-[#604f2f]"
              : "bg-[#254939] text-emerald-50 hover:bg-[#2b5844]",
          )}
        >
          {isRunning ? "Pause" : "Start"}
        </button>
        <button
          type="button"
          onClick={() => {
            setIsRunning(false);
            setElapsedSeconds(0);
            activateTask(null);
          }}
          className="inline-flex min-h-9 items-center justify-center gap-1.5 rounded-md bg-[#3a3a38] px-3 py-2 text-[9pt] font-semibold text-white shadow-sm transition hover:bg-[#454542]"
        >
          Reset Panel
        </button>
      </div>
    </main>
  );
}
