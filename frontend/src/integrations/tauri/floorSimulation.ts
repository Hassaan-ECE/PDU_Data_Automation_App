import type {
  BackendTaskState,
  BackendTaskStatus,
  TaskBatchProcessResult,
  TaskProcessResult,
  UnitFolderSuggestion,
  UnitFolderSummary,
} from "./backend";

type SimulationClock = () => number;

type SimulatedTaskDefinition = {
  id: string;
  label: string;
  step: string;
  detectedSteps: number[];
};

type SimulatedTaskRuntime = {
  definition: SimulatedTaskDefinition;
  state: BackendTaskState;
  accepted: boolean;
  startedAtMs: number | null;
  processedAt: string | null;
  result: string | null;
};

const LOADS = ["100% Load", "50% Load", "20% Load"] as const;
const DEMO_FAILURE_TASK_ID = "208v-system-20% Load";

export const FLOOR_SIMULATION_ENABLED = import.meta.env.VITE_PDU_SIMULATION_MODE === "true";
export const DEMO_UNIT_FOLDER = "C:\\PDU500\\DEMO_262343000072";
export const DEMO_SERIAL_NUMBER = "262343000072";
export const DEMO_WARNING =
  "DEMO MODE — accelerated floor simulation; no CSV or Excel files are changed.";

function addLoadTasks(
  tasks: SimulatedTaskDefinition[],
  prefix: string,
  firstStep: number,
) {
  for (const [loadIndex, load] of LOADS.entries()) {
    const step = firstStep + loadIndex;

    tasks.push({
      id: `${prefix}-${load}`,
      label: load,
      step: String(step),
      detectedSteps: [step],
    });
  }
}

function addBreakerTasks(
  tasks: SimulatedTaskDefinition[],
  voltagePrefix: "208v" | "415v",
  firstStep: number,
) {
  for (let breakerNumber = 1; breakerNumber <= 8; breakerNumber += 1) {
    addLoadTasks(
      tasks,
      `${voltagePrefix}-breaker-${breakerNumber}`,
      firstStep + (breakerNumber - 1) * LOADS.length,
    );
  }
}

function createTaskDefinitions() {
  const tasks: SimulatedTaskDefinition[] = [
    {
      id: "208v-transformer",
      label: "208V Transformer Check",
      step: "14",
      detectedSteps: [14],
    },
  ];

  addLoadTasks(tasks, "208v-system", 15);
  addBreakerTasks(tasks, "208v", 18);
  tasks.push({
    id: "415v-transformer",
    label: "415V Transformer Check",
    step: "43",
    detectedSteps: [43],
  });
  addLoadTasks(tasks, "415v-system", 44);
  addBreakerTasks(tasks, "415v", 47);
  tasks.push({
    id: "system-burn-in",
    label: "System Burn-In",
    step: "71/72",
    detectedSteps: [71, 72],
  });

  for (let breakerNumber = 1; breakerNumber <= 8; breakerNumber += 1) {
    const step = 72 + breakerNumber;

    tasks.push({
      id: `breaker-burn-in-${breakerNumber}`,
      label: `Breaker ${breakerNumber}`,
      step: String(step),
      detectedSteps: [step],
    });
  }

  return tasks;
}

const TASK_DEFINITIONS = createTaskDefinitions();

function serialNumberFromFolder(unitFolder: string) {
  const folderName = unitFolder.split(/[\\/]/).filter(Boolean).at(-1) ?? "";

  return folderName.match(/\d{12}/)?.[0] ?? DEMO_SERIAL_NUMBER;
}

export class FloorSimulation {
  private readonly clock: SimulationClock;
  private readonly durationMs: number;
  private readonly durationSeconds: number;
  private unitFolder = DEMO_UNIT_FOLDER;
  private tasks: SimulatedTaskRuntime[] = [];

  constructor(clock: SimulationClock = () => Date.now(), durationMs = 2_500) {
    this.clock = clock;
    this.durationMs = durationMs;
    this.durationSeconds = Math.max(1, Math.ceil(durationMs / 1_000));
    this.reset(DEMO_UNIT_FOLDER);
  }

  setup(unitFolder: string): UnitFolderSummary {
    this.reset(unitFolder);
    return this.summary();
  }

  suggestion(): UnitFolderSuggestion {
    return {
      detected_count: 0,
      detection_reason: "accelerated floor simulation",
      detection_source: DEMO_UNIT_FOLDER,
      serial_label: `SN ${DEMO_SERIAL_NUMBER}`,
      serial_number: DEMO_SERIAL_NUMBER,
      unit_folder: DEMO_UNIT_FOLDER,
    };
  }

  scan(unitFolder: string): UnitFolderSummary {
    this.ensureFolder(unitFolder);

    if (!this.tasks.some((task) => this.isActive(task))) {
      const nextTask = this.tasks.find((task) => task.state === "off");

      if (nextTask) {
        nextTask.state = "detected";
        nextTask.startedAtMs = this.clock();
      }
    }

    return this.summary();
  }

  processTask(unitFolder: string, taskId: string): TaskProcessResult {
    this.ensureFolder(unitFolder);
    const task = this.tasks.find((candidate) => candidate.definition.id === taskId);

    if (!task) {
      return {
        task_id: taskId,
        state: "fail",
        code: 1,
        continue_sequence: false,
        message: `Unknown simulated task: ${taskId}`,
        log: [],
        report_path: this.reportPath(),
        print_report_path: this.printReportPath(),
        failure: {
          title: "Simulation Error",
          message: `Unknown simulated task: ${taskId}`,
          location: null,
        },
        source_csv_path: null,
        csv_fingerprint: null,
      };
    }

    if (task.accepted) {
      task.accepted = false;
      task.state = "detected";
    }

    const status = this.taskStatus(task);

    if (!status.process_ready) {
      return {
        task_id: taskId,
        state: "waiting",
        code: 2,
        continue_sequence: false,
        message: `Waiting for ${task.definition.label} test to finish`,
        log: ["Simulated CSV is still inside its accelerated timing window."],
        report_path: this.reportPath(),
        print_report_path: this.printReportPath(),
        failure: null,
        source_csv_path: status.source_csv_path,
        csv_fingerprint: status.csv_fingerprint,
      };
    }

    const processedAt = new Date(this.clock()).toISOString();
    const sourceCsvPath = this.csvPath(task);
    const csvFingerprint = this.csvFingerprint(task);

    if (taskId === DEMO_FAILURE_TASK_ID) {
      task.state = "fail";
      task.processedAt = processedAt;
      task.result = "fail";

      return {
        task_id: taskId,
        state: "fail",
        code: 1,
        continue_sequence: true,
        message: "208V System 20% Load failed accuracy; demo readings were still written.",
        log: [
          "Simulated out-of-range voltage detected.",
          "Demo report patch committed before the task was marked failed.",
          "Runner continuation approved for this accuracy failure.",
        ],
        report_path: this.reportPath(),
        print_report_path: this.printReportPath(),
        failure: {
          title: "Accuracy Check Failed",
          message:
            "The simulated reading is outside the allowed range. The bad values were written to the demo report, the step is marked red, and the sequence will continue.",
          location: {
            workbook_path: this.reportPath(),
            sheet: "System Test - 480_208",
            cell: "G57",
          },
        },
        source_csv_path: sourceCsvPath,
        csv_fingerprint: csvFingerprint,
      };
    }

    task.state = "pass";
    task.processedAt = processedAt;
    task.result = "pass";

    return {
      task_id: taskId,
      state: "pass",
      code: 0,
      continue_sequence: false,
      message: `${task.definition.label} passed; demo report updated.`,
      log: ["Simulated CSV processed.", "Demo workbook patch committed."],
      report_path: this.reportPath(),
      print_report_path: this.printReportPath(),
      failure: null,
      source_csv_path: sourceCsvPath,
      csv_fingerprint: csvFingerprint,
    };
  }

  acceptTaskFailure(unitFolder: string, taskId: string): UnitFolderSummary {
    this.ensureFolder(unitFolder);
    const task = this.tasks.find((candidate) => candidate.definition.id === taskId);

    if (!task || task.state !== "fail") {
      throw new Error("Only a persisted failed demo task can be marked as pass.");
    }

    task.accepted = true;
    return this.summary();
  }

  processTasks(unitFolder: string, taskIds: string[]): TaskBatchProcessResult {
    const results = taskIds.map((taskId) => this.processTask(unitFolder, taskId));
    const committedResults = results.filter(
      (result) => result.state === "pass" || result.continue_sequence,
    );
    const stoppedResult = results.find(
      (result) => result.state === "fail" && !result.continue_sequence,
    );
    const failedCount = results.filter((result) => result.state === "fail").length;

    return {
      results,
      committed: committedResults.length > 0,
      committed_count: committedResults.length,
      stopped_task_id: stoppedResult?.task_id ?? null,
      message: `Demo batch processed ${results.length} task${results.length === 1 ? "" : "s"} (${failedCount} failed).`,
    };
  }

  summary(): UnitFolderSummary {
    return {
      unit_folder: this.unitFolder,
      serial_number: serialNumberFromFolder(this.unitFolder),
      report_path: this.reportPath(),
      print_report_path: this.printReportPath(),
      detected_count: 0,
      tasks: this.tasks.map((task) => this.taskStatus(task)),
      warnings: [DEMO_WARNING],
    };
  }

  private reset(unitFolder: string) {
    this.unitFolder = unitFolder || DEMO_UNIT_FOLDER;
    this.tasks = TASK_DEFINITIONS.map((definition) => ({
      definition,
      state: "off",
      accepted: false,
      startedAtMs: null,
      processedAt: null,
      result: null,
    }));
  }

  private ensureFolder(unitFolder: string) {
    if (this.unitFolder !== unitFolder || this.tasks.length === 0) {
      this.reset(unitFolder);
    }
  }

  private isActive(task: SimulatedTaskRuntime) {
    return task.state === "detected" || task.state === "waiting" || task.state === "processing";
  }

  private taskStatus(task: SimulatedTaskRuntime): BackendTaskStatus {
    const startedAtMs = task.startedAtMs;
    const phaseDeadlineMs = startedAtMs === null ? null : startedAtMs + this.durationMs;
    const processReady =
      startedAtMs !== null && this.isActive(task) && this.clock() >= (phaseDeadlineMs ?? 0);
    const sourceCsvPath = startedAtMs === null ? null : this.csvPath(task);
    const effectiveState = task.accepted ? "pass" : task.state;
    const terminal = effectiveState === "pass" || effectiveState === "fail";

    return {
      task_id: task.definition.id,
      label: task.definition.label,
      step: task.definition.step,
      state: effectiveState,
      detected_steps: [...task.definition.detectedSteps],
      latest_csv: sourceCsvPath,
      latest_csv_created_ms: startedAtMs,
      latest_csv_readable: startedAtMs === null ? null : true,
      timer_start_ms: startedAtMs,
      processable: startedAtMs !== null,
      process_ready: processReady,
      wait_phase:
        startedAtMs === null ? "awaiting_csv" : terminal || processReady ? "ready" : "timing",
      phase_deadline_ms: phaseDeadlineMs,
      pending_duration_seconds: startedAtMs === null ? this.durationSeconds : 0,
      nominal_duration_seconds: this.durationSeconds,
      match_reason:
        startedAtMs === null
          ? "Waiting for simulated floor CSV"
          : "Matched accelerated simulated floor CSV",
      source_csv_path: sourceCsvPath,
      csv_fingerprint: startedAtMs === null ? null : this.csvFingerprint(task),
      processed_at: task.processedAt,
      result: task.result,
      accepted: task.accepted,
    };
  }

  private csvPath(task: SimulatedTaskRuntime) {
    const firstStep = task.definition.detectedSteps[0] ?? 0;
    const safeTaskId = task.definition.id.replace(/[^a-zA-Z0-9]+/g, "_");

    return `${this.unitFolder}\\STEP${firstStep}_${safeTaskId}_DEMO.csv`;
  }

  private csvFingerprint(task: SimulatedTaskRuntime) {
    return `demo-${task.definition.id}-${task.startedAtMs ?? 0}`;
  }

  private reportPath() {
    return `${this.unitFolder}\\PDUD500442AM088_Test Report_0.2CT_Rev02_SN${serialNumberFromFolder(this.unitFolder)}.xlsx`;
  }

  private printReportPath() {
    return `${this.unitFolder}\\PDUD500442AA088_0.2CT Test Report Print.xlsx`;
  }
}

export const floorSimulation = new FloorSimulation();
