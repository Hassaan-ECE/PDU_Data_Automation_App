import type { ShiftWindow } from "./settingsTypes";

const MINUTES_PER_DAY = 24 * 60;

export function parseTimeToMinutes(value: string): number {
  const match = value.trim().match(/^(\d{1,2}):(\d{2})(?::\d{2})?$/);
  if (!match) return 6 * 60;
  const hour = Number(match[1]);
  const minute = Number(match[2]);
  if (hour < 0 || hour > 23 || minute < 0 || minute > 59) return 6 * 60;
  return hour * 60 + minute;
}

export function minutesToTime(value: number): string {
  const normalized = ((value % MINUTES_PER_DAY) + MINUTES_PER_DAY) % MINUTES_PER_DAY;
  return `${String(Math.floor(normalized / 60)).padStart(2, "0")}:${String(normalized % 60).padStart(2, "0")}`;
}

export function formatDisplayTime(value: string): string {
  const total = parseTimeToMinutes(value);
  const hour24 = Math.floor(total / 60);
  const minute = total % 60;
  const hour12 = hour24 % 12 || 12;
  return `${hour12}:${String(minute).padStart(2, "0")} ${hour24 < 12 ? "AM" : "PM"}`;
}

export function shiftDurationMinutes(startTime: string, endTime: string): number {
  const start = parseTimeToMinutes(startTime);
  const end = parseTimeToMinutes(endTime);
  return end === start ? 0 : (end - start + MINUTES_PER_DAY) % MINUTES_PER_DAY;
}

export function shiftScheduleError(shifts: ShiftWindow[]): string {
  if (shifts.some((shift) => shiftDurationMinutes(shift.start_time, shift.end_time) === 0)) {
    return "Each shift needs different start and end times.";
  }
  if (shifts.length === 2 && circularRangesOverlap(shifts[0], shifts[1])) {
    return "Shift 1 and Shift 2 overlap. Adjust the ranges before saving.";
  }
  return "";
}

function circularRangesOverlap(first: ShiftWindow, second: ShiftWindow): boolean {
  const firstSegments = splitCircularRange(first.start_time, first.end_time);
  const secondSegments = splitCircularRange(second.start_time, second.end_time);
  return firstSegments.some(([firstStart, firstEnd]) =>
    secondSegments.some(
      ([secondStart, secondEnd]) =>
        Math.max(firstStart, secondStart) < Math.min(firstEnd, secondEnd),
    ),
  );
}

function splitCircularRange(startTime: string, endTime: string): [number, number][] {
  const start = parseTimeToMinutes(startTime);
  const end = parseTimeToMinutes(endTime);
  if (start === end) return [[0, MINUTES_PER_DAY]];
  return end > start ? [[start, end]] : [[start, MINUTES_PER_DAY], [0, end]];
}
