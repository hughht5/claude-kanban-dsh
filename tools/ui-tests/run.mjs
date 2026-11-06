// Runs every tools/ui-tests/*.test.mjs with node:test and exits non-zero on a failure (card #120).
// `node tools/ui-tests/run.mjs`; src-tauri/tests/ui_js.rs runs it from `cargo test` when node is on PATH.
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const tests = fs.readdirSync(here).filter((f) => f.endsWith(".test.mjs")).sort().map((f) => path.join(here, f));
if (!tests.length) { console.error("no *.test.mjs files in " + here); process.exit(1); }
const r = spawnSync(process.execPath, ["--test", ...tests], { stdio: "inherit" });
process.exit(r.status === null ? 1 : r.status);
