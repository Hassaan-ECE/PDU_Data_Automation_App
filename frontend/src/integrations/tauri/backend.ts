import { invoke } from "@tauri-apps/api/core";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";

export interface BackendStatus {
  app_name: string;
  version: string;
  backend: string;
}

export interface LayoutValidationResult {
  warnings: string[];
  errors: string[];
}

export interface LayoutLoadResponse {
  profile_id: string;
  display_name: string;
  task_count: number;
  validation: LayoutValidationResult;
}

export function isTauriRuntime() {
  return Boolean(window.__TAURI_INTERNALS__);
}

export async function getBackendStatus(): Promise<BackendStatus | null> {
  if (!isTauriRuntime()) {
    return null;
  }

  return invoke<BackendStatus>("get_app_status");
}

export async function loadExampleLayoutProfile(): Promise<LayoutLoadResponse | null> {
  if (!isTauriRuntime()) {
    return null;
  }

  return invoke<LayoutLoadResponse>("load_example_layout_profile");
}

export async function chooseUnitFolder(): Promise<string | null> {
  if (!isTauriRuntime()) {
    return "C:\\PDU500\\DEMO_20260617";
  }

  const selected = await openDialog({
    directory: true,
    multiple: false,
    title: "Select PDU unit folder",
  });

  return typeof selected === "string" ? selected : null;
}

export async function openReportPath(path: string) {
  if (!isTauriRuntime()) {
    return;
  }

  await openPath(path);
}
