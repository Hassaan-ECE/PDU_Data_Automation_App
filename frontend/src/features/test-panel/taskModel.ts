import type { PanelItem } from "./types";

const loads = ["100% Load", "50% Load", "20% Load"] as const;

function loadTasks(prefix: string, firstStep: number): PanelItem[] {
  return loads.map((load, index) => {
    const step = firstStep + index;

    return {
      kind: "task",
      id: `${prefix}-${load}`,
      label: load,
      step: String(step),
      state: "off",
    };
  });
}

function breakerLoadGroups(prefix: "208v" | "415v", firstStep: number): PanelItem[] {
  return Array.from({ length: 8 }, (_, breakerIndex) => {
    const breakerNumber = breakerIndex + 1;

    return {
      kind: "section",
      id: `${prefix}-breaker-${breakerNumber}`,
      label: `Breaker ${breakerNumber}`,
      children: loadTasks(
        `${prefix}-breaker-${breakerNumber}`,
        firstStep + breakerIndex * loads.length,
      ),
    };
  });
}

function burnInBreakerTasks(): PanelItem[] {
  return Array.from({ length: 8 }, (_, index) => {
    const step = 73 + index;
    const breakerNumber = index + 1;

    return {
      kind: "task",
      id: `breaker-burn-in-${breakerNumber}`,
      label: `Breaker ${breakerNumber}`,
      step: String(step),
      state: "off",
    };
  });
}

export const legacyPanelItems: PanelItem[] = [
  {
    kind: "task",
    id: "208v-transformer",
    label: "208V Transformer Check",
    step: "14",
    state: "off",
  },
  {
    kind: "section",
    id: "208v-system",
    label: "208V System",
    children: loadTasks("208v-system", 15),
  },
  {
    kind: "section",
    id: "208v-breaker",
    label: "208V Breaker",
    children: breakerLoadGroups("208v", 18),
  },
  {
    kind: "task",
    id: "415v-transformer",
    label: "415V Transformer Check",
    step: "43",
    state: "off",
  },
  {
    kind: "section",
    id: "415v-system",
    label: "415V System",
    children: loadTasks("415v-system", 44),
  },
  {
    kind: "section",
    id: "415v-breaker",
    label: "415V Breaker",
    children: breakerLoadGroups("415v", 47),
  },
  {
    kind: "task",
    id: "system-burn-in",
    label: "System Burn-In",
    step: "71/72",
    state: "off",
  },
  {
    kind: "section",
    id: "breaker-burn-in",
    label: "Breaker Burn-In",
    children: burnInBreakerTasks(),
  },
];
