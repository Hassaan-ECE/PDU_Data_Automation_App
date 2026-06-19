type StartupTimingDetail = Record<string, unknown> | string | number | null | undefined;

const STARTUP_MARK_PREFIX = "pdu-startup";

export function markStartup(label: string, detail?: StartupTimingDetail) {
  if (typeof performance === "undefined") {
    return;
  }

  const elapsedMs = Math.round(performance.now());
  const markName = `${STARTUP_MARK_PREFIX}:${label}`;

  try {
    performance.mark(markName);
  } catch {
    // Timing marks are diagnostic only; logging should never affect the app.
  }

  if (detail === undefined) {
    console.info(`[startup] ${label} ${elapsedMs}ms`);
    return;
  }

  console.info(`[startup] ${label} ${elapsedMs}ms`, detail);
}
