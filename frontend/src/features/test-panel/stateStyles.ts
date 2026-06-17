import type { TaskState } from "./types";

export interface StateStyle {
  button: string;
}

export const stateStyles: Record<TaskState, StateStyle> = {
  off: {
    button: "bg-[#343434] text-zinc-100 hover:bg-[#3d3d3d]",
  },
  detected: {
    button: "bg-[#463455] text-violet-50 hover:bg-[#523b63]",
  },
  waiting: {
    button: "bg-[#51452b] text-amber-50 hover:bg-[#604f2f]",
  },
  processing: {
    button: "bg-[#51452b] text-amber-50 hover:bg-[#604f2f]",
  },
  pass: {
    button: "bg-[#254939] text-emerald-50 hover:bg-[#2b5844]",
  },
  warning: {
    button: "bg-[#274554] text-sky-50 hover:bg-[#2f5264]",
  },
  fail: {
    button: "bg-[#5a2d32] text-rose-50 hover:bg-[#69343a]",
  },
};
