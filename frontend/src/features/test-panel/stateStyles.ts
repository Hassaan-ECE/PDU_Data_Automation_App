import type { TaskState } from "./types";

export interface StateStyle {
  button: string;
}

export const knownGoodColorPalette = {
  shell: {
    background: "#20201f",
    panel: "#292928",
    input: "#1f1f1e",
  },
  states: {
    off: "#343434",
    detected: "#463455",
    waiting: "#51452b",
    processing: "#51452b",
    pass: "#254939",
    warning: "#274554",
    skipped: "#3d4142",
    fail: "#5a2d32",
  },
  controls: {
    browseReset: "#3a3a38",
    browseResetHover: "#454542",
  },
  error: {
    panel: "#301f22",
    rerun: "#493135",
    rerunHover: "#563940",
    open: "#263f48",
    openHover: "#2f4d58",
  },
  ring: {
    current: "cyan-200/65",
    focus: "cyan-200/25",
  },
} as const;

export const stateStyles: Record<TaskState, StateStyle> = {
  off: {
    button: "bg-[#343434] text-white hover:bg-[#3d3d3d]",
  },
  detected: {
    button: "bg-[#1f74ae] text-white hover:bg-[#2874a8]",
  },
  waiting: {
    button: "bg-[#9b630c] text-white hover:bg-[#9b630c]",
  },
  processing: {
    button: "bg-[#9b630c] text-white hover:bg-[#9b630c]",
  },
  pass: {
    button: "bg-[#1d7f47] text-white hover:bg-[#1d7f46]",
  },
  warning: {
    button: "bg-[#9c6308] text-white hover:bg-[#9b630c]",
  },
  skipped: {
    button: "bg-[#3d4142] text-white hover:bg-[#484d4e]",
  },
  fail: {
    button: "bg-[#d42c1a] text-white hover:bg-[#ca3c2d]",
  },
};
