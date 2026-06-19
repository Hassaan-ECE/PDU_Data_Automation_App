import { CheckCircle2Icon, DownloadIcon, LoaderCircleIcon } from "lucide-react";

import type { UpdateState } from "./updateTypes";

interface UpdateActionButtonProps {
  onClick: () => void;
  state: UpdateState;
}

export function UpdateActionButton({ onClick, state }: UpdateActionButtonProps) {
  if (!state.available && state.status !== "ready" && state.status !== "error") {
    return null;
  }

  const label = getUpdateActionLabel(state);
  if (!label) {
    return null;
  }

  const progress = getUpdateProgress(state);
  const isBusy = state.status === "downloading" || state.status === "checking" || state.status === "installing";

  return (
    <button
      type="button"
      onClick={onClick}
      disabled={isBusy}
      className="relative mb-1.5 inline-flex min-h-7 w-full items-center justify-center overflow-hidden rounded-md border border-sky-400/80 bg-sky-950/55 px-2.5 text-[8pt] font-semibold text-sky-100 shadow-sm transition hover:bg-sky-900/70 disabled:cursor-default disabled:opacity-90"
    >
      {progress !== null ? (
        <span
          aria-hidden="true"
          className="absolute inset-y-0 left-0 bg-sky-500/30 transition-[width] duration-200"
          style={{ width: `${progress}%` }}
        />
      ) : null}
      <span className="relative z-10 inline-flex min-w-0 items-center gap-1.5">
        {renderUpdateActionIcon(state)}
        <span className="truncate">{label}</span>
      </span>
    </button>
  );
}

function getUpdateActionLabel(state: UpdateState): string {
  switch (state.status) {
    case "available":
      return state.latestVersion ? `Update ${state.latestVersion}` : "Update available";
    case "downloading":
      return typeof state.downloadProgress === "number" ? `Downloading ${state.downloadProgress}%` : "Downloading update...";
    case "ready":
      return "Install update";
    case "installing":
      return "Installing update...";
    case "error":
      return "Retry update";
    default:
      return "";
  }
}

function renderUpdateActionIcon(state: UpdateState) {
  switch (state.status) {
    case "ready":
      return <CheckCircle2Icon className="h-3.5 w-3.5" aria-hidden="true" />;
    case "downloading":
    case "installing":
      return <LoaderCircleIcon className="h-3.5 w-3.5 animate-spin" aria-hidden="true" />;
    default:
      return <DownloadIcon className="h-3.5 w-3.5" aria-hidden="true" />;
  }
}

function getUpdateProgress(state: UpdateState): number | null {
  if (state.status === "installing") {
    return 100;
  }

  if (state.status !== "downloading") {
    return null;
  }

  if (typeof state.downloadProgress !== "number" || !Number.isFinite(state.downloadProgress)) {
    return 10;
  }

  return Math.max(3, Math.min(100, state.downloadProgress));
}
