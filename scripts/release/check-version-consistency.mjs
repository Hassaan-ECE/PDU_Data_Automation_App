import { readFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");

function versionFromCargoToml(content) {
  const match = content.match(/^\s*version\s*=\s*"([^"]+)"\s*$/m);

  if (!match) {
    throw new Error("backend/Cargo.toml does not contain a package version");
  }

  return match[1];
}

const packageJson = JSON.parse(await readFile(path.join(repoRoot, "package.json"), "utf8"));
const cargoToml = await readFile(path.join(repoRoot, "backend", "Cargo.toml"), "utf8");
const tauriConfig = JSON.parse(await readFile(path.join(repoRoot, "backend", "tauri.conf.json"), "utf8"));

const versions = {
  "package.json": packageJson.version,
  "backend/Cargo.toml": versionFromCargoToml(cargoToml),
  "backend/tauri.conf.json": tauriConfig.version,
};
const uniqueVersions = new Set(Object.values(versions));

if (uniqueVersions.size !== 1) {
  throw new Error(
    `Version mismatch:\n${Object.entries(versions)
      .map(([file, version]) => `  ${file}: ${version}`)
      .join("\n")}`,
  );
}

console.log(`Version consistency check passed: ${packageJson.version}`);
