import { fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { describe, expect, it } from "vitest";

import {
  ShiftRangePicker,
} from "@/features/settings/ShiftRangePicker";
import { minutesToTime, shiftScheduleError } from "@/features/settings/shiftTime";

function PickerHarness({ start = "11:55", end = "15:00" }: { start?: string; end?: string }) {
  const [startTime, setStartTime] = useState(start);
  const [endTime, setEndTime] = useState(end);
  return (
    <ShiftRangePicker
      startTime={startTime}
      endTime={endTime}
      onStartChange={setStartTime}
      onEndChange={setEndTime}
    />
  );
}

describe("ShiftRangePicker", () => {
  it("scrolls minutes in five-minute increments and rolls AM into PM", () => {
    render(<PickerHarness />);

    expect(screen.getByRole("button", { name: "Edit Start time, 11:55 AM" })).toBeInTheDocument();
    expect(screen.getByText("AM")).toBeInTheDocument();
    fireEvent.wheel(screen.getByRole("group", { name: "Minute wheel" }), { deltaY: 100 });

    expect(screen.getByRole("button", { name: "Edit Start time, 12:00 PM" })).toBeInTheDocument();
    expect(screen.getByText("PM")).toBeInTheDocument();
  });

  it("supports arrow keys and editing the end time independently", () => {
    render(<PickerHarness start="06:00" end="15:00" />);

    fireEvent.click(screen.getByRole("button", { name: "Edit End time, 3:00 PM" }));
    fireEvent.keyDown(screen.getByRole("group", { name: "Hour wheel" }), { key: "ArrowDown" });

    expect(screen.getByRole("button", { name: "Edit End time, 4:00 PM" })).toBeInTheDocument();
    expect(screen.getByText("10 hours")).toBeInTheDocument();
  });

  it("shows overnight ranges and their duration", () => {
    render(<PickerHarness start="23:00" end="07:00" />);

    expect(screen.getByText("8 hours · overnight")).toBeInTheDocument();
    expect(
      screen.getByRole("img", { name: "11:00 PM to 7:00 AM, ending the following day" }),
    ).toBeInTheDocument();
  });

  it("detects overlapping double shifts while allowing touching boundaries", () => {
    expect(
      shiftScheduleError([
        { label: "Day", start_time: "06:00", end_time: "15:00" },
        { label: "Night", start_time: "14:00", end_time: "23:00" },
      ]),
    ).toMatch(/overlap/i);
    expect(
      shiftScheduleError([
        { label: "Day", start_time: "06:00", end_time: "15:00" },
        { label: "Night", start_time: "15:00", end_time: "23:00" },
      ]),
    ).toBe("");
  });

  it("wraps stored times across midnight", () => {
    expect(minutesToTime(24 * 60)).toBe("00:00");
    expect(minutesToTime(-5)).toBe("23:55");
  });
});
