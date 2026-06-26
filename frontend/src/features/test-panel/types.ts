import type { BackendTaskStatus, FailureLocation } from "@/integrations/tauri/backend";

export type TaskState =
  | "off"
  | "detected"
  | "waiting"
  | "processing"
  | "pass"
  | "warning"
  | "fail"
  | "skipped";

export interface TaskItem {
  kind: "task";
  id: string;
  label: string;
  step: string;
  state: TaskState;
}

export interface SectionItem {
  kind: "section";
  id: string;
  label: string;
  children: PanelItem[];
}

export type PanelItem = TaskItem | SectionItem;

export function isSectionItem(item: PanelItem): item is SectionItem {
  return item.kind === "section";
}

export type BacklogPromptState = {
  count: number;
  resolve: (processBacklog: boolean | null) => void;
} | null;

export type TransformerSnSaveStatus = "idle" | "dirty" | "saving" | "saved" | "error";

export type TaskFailureNotice = {
  taskId: string;
  title: string;
  message: string;
  reportPath: string | null;
  location: FailureLocation | null;
  fromRunner: boolean;
};

export type BackendTaskStatusMap = Record<string, BackendTaskStatus | undefined>;
