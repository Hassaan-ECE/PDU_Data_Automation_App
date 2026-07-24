import { describe, expect, it } from "vitest";

import {
  DEMO_UNIT_FOLDER,
  FloorSimulation,
} from "@/integrations/tauri/floorSimulation";

describe("FloorSimulation", () => {
  it("creates the complete 65-step floor workflow without touching files", () => {
    const simulation = new FloorSimulation(() => 1_000, 2_000);
    const summary = simulation.setup(DEMO_UNIT_FOLDER);

    expect(summary.tasks).toHaveLength(65);
    expect(summary.tasks[0]).toMatchObject({
      task_id: "208v-transformer",
      state: "off",
      step: "14",
      wait_phase: "awaiting_csv",
    });
    expect(summary.tasks.at(-1)).toMatchObject({
      task_id: "breaker-burn-in-8",
      step: "80",
    });
    expect(summary.warnings[0]).toContain("no CSV or Excel files are changed");
  });

  it("times tasks, flags the demo accuracy failure, and starts the next step", () => {
    let nowMs = 1_000;
    const simulation = new FloorSimulation(() => nowMs, 2_000);
    simulation.setup(DEMO_UNIT_FOLDER);

    for (const taskId of [
      "208v-transformer",
      "208v-system-100% Load",
      "208v-system-50% Load",
    ]) {
      const timing = simulation.scan(DEMO_UNIT_FOLDER);
      expect(timing.tasks.find((task) => task.task_id === taskId)).toMatchObject({
        state: "detected",
        process_ready: false,
        wait_phase: "timing",
      });

      nowMs += 2_001;
      const ready = simulation.scan(DEMO_UNIT_FOLDER);
      expect(ready.tasks.find((task) => task.task_id === taskId)?.process_ready).toBe(true);
      expect(simulation.processTask(DEMO_UNIT_FOLDER, taskId).state).toBe("pass");
    }

    const failureTiming = simulation.scan(DEMO_UNIT_FOLDER);
    expect(
      failureTiming.tasks.find((task) => task.task_id === "208v-system-20% Load")?.wait_phase,
    ).toBe("timing");

    nowMs += 2_001;
    simulation.scan(DEMO_UNIT_FOLDER);
    const failure = simulation.processTask(DEMO_UNIT_FOLDER, "208v-system-20% Load");

    expect(failure).toMatchObject({
      state: "fail",
      continue_sequence: true,
      failure: {
        title: "Accuracy Check Failed",
        location: { sheet: "System Test - 480_208", cell: "G57" },
      },
    });

    const continued = simulation.scan(DEMO_UNIT_FOLDER);
    expect(continued.tasks.find((task) => task.task_id === "208v-breaker-1-100% Load")).toMatchObject({
      state: "detected",
      wait_phase: "timing",
    });
    expect(continued.tasks.find((task) => task.task_id === "208v-system-20% Load")?.state).toBe(
      "fail",
    );
  });
});
