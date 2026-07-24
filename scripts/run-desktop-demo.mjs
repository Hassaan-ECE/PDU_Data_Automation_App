import { spawn } from "node:child_process";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const packageJson = JSON.parse(readFileSync(path.join(repoRoot, "package.json"), "utf8"));

console.log("Starting accelerated PDU floor simulation.");
console.log("Demo mode does not read CSV files or modify Excel reports.");
console.log("Use `bun run desktop` for the real floor backend.\n");

const child = spawn("cargo", ["tauri", "dev", "--config", "backend/tauri.conf.json"], {
  cwd: repoRoot,
  env: {
    ...process.env,
    VITE_PDU_SIMULATION_MODE: "true",
    VITE_PDU_SIMULATION_VERSION: packageJson.version,
  },
  shell: false,
  stdio: "inherit",
});

for (const signal of ["SIGINT", "SIGTERM"]) {
  process.on(signal, () => child.kill(signal));
}

child.on("error", (error) => {
  console.error(`Unable to start the desktop demo: ${error.message}`);
  process.exitCode = 1;
});

child.on("exit", (code) => {
  process.exitCode = code ?? 1;
});
