import type { ReactNode, RefObject, UIEventHandler } from "react";
import { CircleCheck, ExternalLink, RotateCcw } from "lucide-react";

import { cn } from "@/shared/lib/utils";

import { getSectionState, sectionProgress } from "./panelLogic";
import { stateStyles } from "./stateStyles";
import {
  isSectionItem,
  type PanelItem,
  type SectionItem,
  type TaskFailureNotice,
  type TaskItem,
  type TaskState,
} from "./types";

type ScrollCue = {
  top: boolean;
  bottom: boolean;
};

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
  onPass,
  onOpenLocation,
}: {
  notice: TaskFailureNotice;
  depth: number;
  onRerun: () => void;
  onPass: () => void;
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
          onClick={onPass}
          className="inline-flex min-h-7 items-center justify-center gap-1 rounded bg-[#1d7f47] px-1.5 text-[7.2pt] font-semibold text-white shadow-sm transition hover:bg-[#248d52]"
        >
          <CircleCheck className="h-3 w-3" aria-hidden="true" />
          Pass
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
  onPassTask,
  onOpenFailureLocation,
}: {
  task: TaskItem;
  currentTaskId: string | null;
  depth?: number;
  onRunTask: (taskId: string) => void;
  failureNotice?: TaskFailureNotice;
  onPassTask: (taskId: string) => void;
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
          onPass={() => onPassTask(task.id)}
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
  onPassTask,
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
  onPassTask: (taskId: string) => void;
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
                onPassTask={onPassTask}
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
                onPassTask={onPassTask}
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

export function WorkflowSteps({
  scrollRef,
  scrollCue,
  panelItems,
  expandedIds,
  currentTaskId,
  failureNotices,
  onToggleSection,
  onRunTask,
  onPassTask,
  onOpenFailureLocation,
  onOpenReport,
  onPrintReport,
  onScroll,
  onTouchStart,
  onWheel,
}: {
  scrollRef: RefObject<HTMLDivElement | null>;
  scrollCue: ScrollCue;
  panelItems: PanelItem[];
  expandedIds: Set<string>;
  currentTaskId: string | null;
  failureNotices: Record<string, TaskFailureNotice>;
  onToggleSection: (id: string) => void;
  onRunTask: (taskId: string) => void;
  onPassTask: (taskId: string) => void;
  onOpenFailureLocation: (notice: TaskFailureNotice) => void;
  onOpenReport: () => void;
  onPrintReport: () => void;
  onScroll: UIEventHandler<HTMLDivElement>;
  onTouchStart: () => void;
  onWheel: () => void;
}) {
  return (
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
          onScroll={onScroll}
          onTouchStart={onTouchStart}
          onWheel={onWheel}
          className="h-full overflow-y-auto [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
        >
          <div className="space-y-1.5 px-1 py-2">
            {panelItems.map((item) =>
              isSectionItem(item) ? (
                <SectionBlock
                  key={item.id}
                  section={item}
                  expanded={expandedIds.has(item.id)}
                  onToggle={onToggleSection}
                  isExpanded={(id) => expandedIds.has(id)}
                  currentTaskId={currentTaskId}
                  onRunTask={onRunTask}
                  failureNotices={failureNotices}
                  onPassTask={onPassTask}
                  onOpenFailureLocation={onOpenFailureLocation}
                />
              ) : (
                <TaskRow
                  key={item.id}
                  task={item}
                  currentTaskId={currentTaskId}
                  onRunTask={onRunTask}
                  failureNotice={failureNotices[item.id]}
                  onPassTask={onPassTask}
                  onOpenFailureLocation={onOpenFailureLocation}
                />
              ),
            )}
            <div aria-label="Report actions" className="grid grid-cols-2 gap-1.5">
              <PanelButton label="Open Report" state="off" onClick={onOpenReport} />
              <PanelButton label="Print Report" state="off" onClick={onPrintReport} />
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
  );
}
