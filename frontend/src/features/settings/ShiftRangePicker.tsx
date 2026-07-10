import { useRef, useState, type PointerEvent, type WheelEvent } from "react";

import { cn } from "@/shared/lib/utils";

import {
  formatDisplayTime,
  minutesToTime,
  parseTimeToMinutes,
  shiftDurationMinutes,
} from "./shiftTime";

const MINUTES_PER_DAY = 24 * 60;
const MINUTE_STEP = 5;

type Endpoint = "start" | "end";

export interface ShiftRangePickerProps {
  startTime: string;
  endTime: string;
  disabled?: boolean;
  onStartChange: (value: string) => void;
  onEndChange: (value: string) => void;
}

export function ShiftRangePicker({
  startTime,
  endTime,
  disabled,
  onStartChange,
  onEndChange,
}: ShiftRangePickerProps) {
  const [activeEndpoint, setActiveEndpoint] = useState<Endpoint>("start");
  const activeValue = activeEndpoint === "start" ? startTime : endTime;
  const activeMinutes = parseTimeToMinutes(activeValue);
  const startMinutes = parseTimeToMinutes(startTime);
  const endMinutes = parseTimeToMinutes(endTime);
  const duration = shiftDurationMinutes(startTime, endTime);
  const overnight = endMinutes < startMinutes;

  function updateActive(delta: number) {
    const next = minutesToTime(activeMinutes + delta);
    if (activeEndpoint === "start") {
      onStartChange(next);
    } else {
      onEndChange(next);
    }
  }

  return (
    <div className="space-y-3">
      <div className="grid grid-cols-2 gap-2">
        <TimeEndpointButton
          endpoint="start"
          active={activeEndpoint === "start"}
          value={startTime}
          disabled={disabled}
          onClick={() => setActiveEndpoint("start")}
        />
        <TimeEndpointButton
          endpoint="end"
          active={activeEndpoint === "end"}
          value={endTime}
          disabled={disabled}
          onClick={() => setActiveEndpoint("end")}
        />
      </div>

      <div className="rounded-lg border border-[#454542] bg-[#1d1d1c] p-3">
        <div className="mb-2 flex items-center justify-between gap-3">
          <span className="text-[7.3pt] font-semibold uppercase tracking-[0.12em] text-[#9a958c]">
            Editing {activeEndpoint}
          </span>
          <span
            aria-live="polite"
            className="rounded-full border border-[#3f5f75] bg-[#203444] px-2 py-0.5 text-[8pt] font-bold text-[#a8d8f8]"
          >
            {activeMinutes < 12 * 60 ? "AM" : "PM"}
          </span>
        </div>

        <div className="grid grid-cols-[1fr_auto_1fr] items-center gap-2">
          <WheelColumn
            label="Hour"
            previous={formatHour(activeMinutes - 60)}
            current={formatHour(activeMinutes)}
            next={formatHour(activeMinutes + 60)}
            disabled={disabled}
            onStep={(direction) => updateActive(direction * 60)}
          />
          <span className="pb-0.5 text-[16pt] font-semibold text-[#777772]" aria-hidden="true">
            :
          </span>
          <WheelColumn
            label="Minute"
            previous={formatMinute(activeMinutes - MINUTE_STEP)}
            current={formatMinute(activeMinutes)}
            next={formatMinute(activeMinutes + MINUTE_STEP)}
            disabled={disabled}
            onStep={(direction) => updateActive(direction * MINUTE_STEP)}
          />
        </div>
        <p className="mt-2 text-center text-[7pt] leading-snug text-[#777772]">
          Scroll, swipe, click, or use arrow keys · 5-minute increments
        </p>
      </div>

      <ShiftRangeBar startTime={startTime} endTime={endTime} />

      <div className="flex items-center justify-between gap-3 text-[7.5pt]">
        <span className="text-[#b7b1a8]">
          {formatDisplayTime(startTime)}–{formatDisplayTime(endTime)}
        </span>
        <span className={cn("font-semibold", duration === 0 ? "text-[#f4b1a9]" : "text-[#86efac]")}>
          {duration === 0
            ? "Choose different times"
            : `${formatDuration(duration)}${overnight ? " · overnight" : ""}`}
        </span>
      </div>
    </div>
  );
}

function TimeEndpointButton({
  endpoint,
  active,
  value,
  disabled,
  onClick,
}: {
  endpoint: Endpoint;
  active: boolean;
  value: string;
  disabled?: boolean;
  onClick: () => void;
}) {
  const label = endpoint === "start" ? "Start" : "End";
  return (
    <button
      type="button"
      disabled={disabled}
      aria-pressed={active}
      aria-label={`Edit ${label} time, ${formatDisplayTime(value)}`}
      onClick={onClick}
      className={cn(
        "rounded-lg border px-3 py-2.5 text-left outline-none transition focus-visible:ring-2 focus-visible:ring-cyan-200/30 disabled:cursor-not-allowed disabled:opacity-55",
        active
          ? "border-[#2c8ac4] bg-[#203444] shadow-[0_0_0_1px_rgba(44,138,196,0.2)]"
          : "border-[#454542] bg-[#242423] hover:border-[#5a5a56] hover:bg-[#2c2c2a]",
      )}
    >
      <span className="block text-[7pt] font-semibold uppercase tracking-[0.12em] text-[#9a958c]">
        {label}
      </span>
      <span className="mt-0.5 block text-[12pt] font-bold tabular-nums text-white">
        {formatDisplayTime(value)}
      </span>
    </button>
  );
}

function WheelColumn({
  label,
  previous,
  current,
  next,
  disabled,
  onStep,
}: {
  label: string;
  previous: string;
  current: string;
  next: string;
  disabled?: boolean;
  onStep: (direction: -1 | 1) => void;
}) {
  const pointerStartRef = useRef<number | null>(null);
  const wheelLockRef = useRef(0);

  function step(direction: -1 | 1) {
    if (!disabled) onStep(direction);
  }

  function handleWheel(event: WheelEvent<HTMLDivElement>) {
    event.preventDefault();
    event.stopPropagation();
    const now = Date.now();
    if (now - wheelLockRef.current < 90 || Math.abs(event.deltaY) < 2) return;
    wheelLockRef.current = now;
    step(event.deltaY > 0 ? 1 : -1);
  }

  function handlePointerDown(event: PointerEvent<HTMLDivElement>) {
    if (disabled) return;
    pointerStartRef.current = event.clientY;
    event.currentTarget.setPointerCapture(event.pointerId);
  }

  function handlePointerUp(event: PointerEvent<HTMLDivElement>) {
    const start = pointerStartRef.current;
    pointerStartRef.current = null;
    if (start === null) return;
    const distance = event.clientY - start;
    if (Math.abs(distance) >= 14) step(distance < 0 ? 1 : -1);
  }

  return (
    <div>
      <div className="mb-1 text-center text-[7pt] font-semibold uppercase tracking-[0.1em] text-[#777772]">
        {label}
      </div>
      <div
        role="group"
        aria-label={`${label} wheel`}
        aria-disabled={disabled}
        tabIndex={disabled ? -1 : 0}
        onWheel={handleWheel}
        onPointerDown={handlePointerDown}
        onPointerUp={handlePointerUp}
        onKeyDown={(event) => {
          if (event.key === "ArrowUp") {
            event.preventDefault();
            step(-1);
          } else if (event.key === "ArrowDown") {
            event.preventDefault();
            step(1);
          }
        }}
        className="touch-none select-none overflow-hidden rounded-lg border border-[#454542] bg-[#242423] outline-none focus-visible:border-[#2c8ac4] focus-visible:ring-2 focus-visible:ring-cyan-200/20"
      >
        <button
          type="button"
          tabIndex={-1}
          disabled={disabled}
          aria-label={`Previous ${label.toLowerCase()}`}
          onClick={() => step(-1)}
          className="flex h-7 w-full items-center justify-center text-[8pt] font-medium tabular-nums text-[#777772] transition hover:bg-[#30302f] hover:text-[#d8d2c8] disabled:opacity-50"
        >
          {previous}
        </button>
        <div
          aria-live="polite"
          className="flex h-10 items-center justify-center border-y border-[#2c8ac4]/70 bg-[#203444] text-[15pt] font-bold tabular-nums text-white shadow-[inset_0_0_12px_rgba(44,138,196,0.08)]"
        >
          {current}
        </div>
        <button
          type="button"
          tabIndex={-1}
          disabled={disabled}
          aria-label={`Next ${label.toLowerCase()}`}
          onClick={() => step(1)}
          className="flex h-7 w-full items-center justify-center text-[8pt] font-medium tabular-nums text-[#777772] transition hover:bg-[#30302f] hover:text-[#d8d2c8] disabled:opacity-50"
        >
          {next}
        </button>
      </div>
    </div>
  );
}

function ShiftRangeBar({ startTime, endTime }: { startTime: string; endTime: string }) {
  const start = parseTimeToMinutes(startTime);
  const end = parseTimeToMinutes(endTime);
  const startPercent = (start / MINUTES_PER_DAY) * 100;
  const endPercent = (end / MINUTES_PER_DAY) * 100;
  const overnight = end < start;
  const label = `${formatDisplayTime(startTime)} to ${formatDisplayTime(endTime)}${overnight ? ", ending the following day" : ""}`;

  return (
    <div role="img" aria-label={label} className="space-y-1.5">
      <div className="relative h-3 overflow-hidden rounded-full border border-[#454542] bg-[#171716]">
        {start !== end ? (
          overnight ? (
            <>
              <span
                className="absolute inset-y-0 right-0 bg-[#1f74ae]"
                style={{ left: `${startPercent}%` }}
              />
              <span
                className="absolute inset-y-0 left-0 bg-[#1f74ae]"
                style={{ width: `${endPercent}%` }}
              />
            </>
          ) : (
            <span
              className="absolute inset-y-0 bg-[#1f74ae]"
              style={{ left: `${startPercent}%`, width: `${endPercent - startPercent}%` }}
            />
          )
        ) : null}
        <span
          className="absolute top-1/2 h-3 w-1 -translate-x-1/2 -translate-y-1/2 rounded-full bg-white"
          style={{ left: `${startPercent}%` }}
        />
        <span
          className="absolute top-1/2 h-3 w-1 -translate-x-1/2 -translate-y-1/2 rounded-full bg-[#86efac]"
          style={{ left: `${endPercent}%` }}
        />
      </div>
      <div className="flex justify-between text-[6.7pt] font-medium text-[#777772]">
        <span>12 AM</span>
        <span>6 AM</span>
        <span>12 PM</span>
        <span>6 PM</span>
        <span>12 AM</span>
      </div>
    </div>
  );
}

function formatHour(value: number): string {
  const total = ((value % MINUTES_PER_DAY) + MINUTES_PER_DAY) % MINUTES_PER_DAY;
  return String(Math.floor(total / 60) % 12 || 12);
}

function formatMinute(value: number): string {
  const total = ((value % MINUTES_PER_DAY) + MINUTES_PER_DAY) % MINUTES_PER_DAY;
  return String(total % 60).padStart(2, "0");
}

function formatDuration(totalMinutes: number): string {
  const hours = Math.floor(totalMinutes / 60);
  const minutes = totalMinutes % 60;
  if (hours && minutes) return `${hours}h ${minutes}m`;
  if (hours) return `${hours} ${hours === 1 ? "hour" : "hours"}`;
  return `${minutes} minutes`;
}
