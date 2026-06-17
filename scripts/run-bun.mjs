import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

const executableName = process.platform === "win32" ? "bun.exe" : "bun";
const candidates = [
  process.env.BUN_EXE,
  join(homedir(), ".bun", "bin", executableName),
  "bun",
].filter(Boolean);

const bunExecutable = candidates.find(
  (candidate) => candidate === "bun" || existsSync(candidate),
);

if (!bunExecutable) {
  console.error("Bun was not found. Install Bun or set BUN_EXE to the Bun executable path.");
  process.exit(1);
}

const result = spawnSync(bunExecutable, process.argv.slice(2), {
  env: process.env,
  shell: false,
  stdio: "inherit",
});

if (result.error) {
  console.error(result.error.message);
  process.exit(1);
}

process.exit(result.status ?? 1);
