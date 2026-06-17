export type TaskState = "off" | "detected" | "waiting" | "processing" | "pass" | "warning" | "fail";

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
