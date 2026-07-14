import { describe, expect, it } from "vitest";

import type { BackendTaskStatus } from "@/integrations/tauri/backend";
import {
  readinessMessage,
  readyDetectedBacklogTaskIds,
  taskSecondsRemaining,
} from "@/features/test-panel/panelLogic";
import type { TaskItem } from "@/features/test-panel/types";

function backendTask(overrides: Partial<BackendTaskStatus> = {}): BackendTaskStatus {
  return {
    accepted: false,
    csv_fingerprint: null,
    detected_steps: [14],
    label: "208V Transformer Check",
    latest_csv: "STEP14.csv",
    latest_csv_created_ms: 1_000,
    latest_csv_readable: true,
    match_reason: "matched",
    nominal_duration_seconds: 60,
    pending_duration_seconds: 0,
    phase_deadline_ms: 61_000,
    process_ready: false,
    processable: true,
    processed_at: null,
    result: null,
    source_csv_path: null,
    state: "waiting",
    step: "14",
    task_id: "208v-transformer",
    timer_start_ms: 1_000,
    wait_phase: "timing",
    ...overrides,
  };
}

const taskItem: TaskItem = {
  id: "208v-transformer",
  kind: "task",
  label: "208V Transformer Check",
  state: "waiting",
  step: "14",
};

describe("backend-owned task readiness", () => {
  it("derives a monotonic countdown from the unchanged backend deadline", () => {
    const firstScan = backendTask({ phase_deadline_ms: 61_000 });
    const secondScan = backendTask({ phase_deadline_ms: 61_000 });

    expect(taskSecondsRemaining(firstScan, 55_000)).toBe(6);
    expect(taskSecondsRemaining(secondScan, 58_000)).toBe(3);
  });

  it("allows a previously waiting task to run once the backend marks it ready", () => {
    const ready = backendTask({
      process_ready: true,
      state: "waiting",
      wait_phase: "ready",
    });

    expect(
      readyDetectedBacklogTaskIds(
        [taskItem],
        { "208v-transformer": "waiting" },
        { "208v-transformer": ready },
      ),
    ).toEqual(["208v-transformer"]);
  });

  it("keeps STEP71-only burn-in out of the processing queue", () => {
    const waiting = backendTask({
      label: "System Burn-In",
      pending_duration_seconds: 60,
      phase_deadline_ms: null,
      state: "waiting",
      task_id: "system-burn-in",
      wait_phase: "waiting_step72",
    });
    const burnInTask = { ...taskItem, id: "system-burn-in", label: "System Burn-In" };

    expect(
      readyDetectedBacklogTaskIds(
        [burnInTask],
        { "system-burn-in": "waiting" },
        { "system-burn-in": waiting },
      ),
    ).toEqual([]);
    expect(readinessMessage(waiting)).toContain("STEP72");
  });
});
