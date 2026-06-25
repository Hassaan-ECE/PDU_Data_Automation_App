import { spawn } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

const commands = [
  ["node", ["scripts/run-bun.mjs", "run", "lint"]],
  ["node", ["scripts/run-bun.mjs", "run", "test"]],
  ["node", ["scripts/run-bun.mjs", "run", "build"]],
  ["node", ["scripts/run-bun.mjs", "run", "check:versions"]],
  ["node", ["scripts/run-bun.mjs", "run", "validate:report-layouts"]],
  ["cargo", ["fmt", "--manifest-path", "backend/Cargo.toml", "--check"]],
  ["cargo", ["check", "--manifest-path", "backend/Cargo.toml"]],
  ["cargo", ["test", "--manifest-path", "backend/Cargo.toml"]],
];

function run(command, args) {
  return new Promise((resolve, reject) => {
    console.log(`\n> ${[command, ...args].join(" ")}`);
    const child = spawn(command, args, {
      cwd: repoRoot,
      shell: false,
      stdio: "inherit",
    });

    child.on("error", reject);
    child.on("exit", (code) => {
      if (code === 0) {
        resolve();
        return;
      }

      reject(new Error(`${command} ${args.join(" ")} exited with code ${code}`));
    });
  });
}

for (const [command, args] of commands) {
  await run(command, args);
}

console.log("\nLocal validation passed.");
