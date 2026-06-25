import Ajv from "ajv";
import { readdir, readFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const schemaPath = path.join(repoRoot, "shared", "schemas", "report-layout.schema.json");
const layoutDir = path.join(repoRoot, "config", "report-layouts");

function formatAjvError(error) {
  const location = error.dataPath || "(root)";
  const detail = error.message || "failed schema validation";

  return `${location} ${detail}`;
}

const schema = JSON.parse(await readFile(schemaPath, "utf8"));
const ajv = new Ajv({ allErrors: true, schemaId: "auto" });
const validate = ajv.compile(schema);
const entries = await readdir(layoutDir, { withFileTypes: true });
const layoutFiles = entries
  .filter(
    (entry) =>
      entry.isFile() &&
      (entry.name.endsWith(".layout.json") || entry.name.endsWith(".layout.example.json")),
  )
  .map((entry) => entry.name)
  .sort();

if (layoutFiles.length === 0) {
  throw new Error(`No report layout files found under ${layoutDir}`);
}

const failures = [];

for (const fileName of layoutFiles) {
  const filePath = path.join(layoutDir, fileName);
  const profile = JSON.parse(await readFile(filePath, "utf8"));

  if (!validate(profile)) {
    failures.push(
      `${path.relative(repoRoot, filePath)}:\n${(validate.errors ?? [])
        .map((error) => `  - ${formatAjvError(error)}`)
        .join("\n")}`,
    );
  }
}

if (failures.length > 0) {
  throw new Error(`Report layout schema validation failed:\n${failures.join("\n\n")}`);
}

console.log(`Validated ${layoutFiles.length} report layout file${layoutFiles.length === 1 ? "" : "s"}.`);
