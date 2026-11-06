// Load test for the app's local API (card #122): many concurrent callers, as card sessions and
// a polling router make, and a report of how long each call took. A call over the limit, or one
// that never answers, means a lock is held too long: look in locks.log beside the board.
//
//   node tools/stress-api.mjs [--channel test|live] [--seconds 60] [--workers 12] [--card N] [--limit 5]
//
// Default channel test (%USERPROFILE%\.claude-kanban-dsh-test\api.json). It only reads: list_cards,
// get_card, and with --card the control tools kanban_session_status and kanban_session_read.
// The token is read from api.json and never printed.
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import http from "node:http";

const args = process.argv.slice(2);
const opt = (name, dflt) => {
  const i = args.indexOf(`--${name}`);
  return i >= 0 && args[i + 1] ? args[i + 1] : dflt;
};
const channel = opt("channel", "test");
const seconds = Number(opt("seconds", "60"));
const workers = Number(opt("workers", "12"));
const card = opt("card", "");
const limitMs = Number(opt("limit", "5")) * 1000;

const dir = path.join(os.homedir(), channel === "live" ? ".claude-kanban-dsh" : ".claude-kanban-dsh-test");
const api = JSON.parse(fs.readFileSync(path.join(dir, "api.json"), "utf8"));

const calls = [
  ["kanban_list_cards", {}],
  ["kanban_get_card", card ? { card: Number(card) } : {}],
];
if (card) {
  calls.push(["kanban_session_status", { card: Number(card) }], ["kanban_session_read", { card: Number(card), since: 0 }]);
}

function call(name, body) {
  return new Promise((resolve) => {
    const data = JSON.stringify(body);
    const started = Date.now();
    const req = http.request(
      { host: "127.0.0.1", port: api.port, method: "POST", path: `/tool/${api.token}/${name}`, headers: { "content-type": "application/json", "content-length": Buffer.byteLength(data) }, timeout: limitMs * 4 },
      (res) => {
        res.resume();
        res.on("end", () => resolve({ name, ms: Date.now() - started, ok: res.statusCode === 200 }));
      },
    );
    req.on("timeout", () => req.destroy(new Error("timeout")));
    req.on("error", (e) => resolve({ name, ms: Date.now() - started, ok: false, error: e.message }));
    req.end(data);
  });
}

const results = [];
const until = Date.now() + seconds * 1000;
async function worker(n) {
  let i = n;
  while (Date.now() < until) {
    const [name, body] = calls[i++ % calls.length];
    const r = await call(name, body);
    results.push(r);
    if (!r.ok || r.ms > limitMs) console.log(`slow or failed: ${r.name} ${r.ms} ms ${r.error ?? ""}`);
  }
}
await Promise.all(Array.from({ length: workers }, (_, n) => worker(n)));

const ms = results.map((r) => r.ms).sort((a, b) => a - b);
const pick = (q) => ms[Math.min(ms.length - 1, Math.floor(q * ms.length))] ?? 0;
const bad = results.filter((r) => !r.ok || r.ms > limitMs);
console.log(`${results.length} calls by ${workers} workers in ${seconds}s on ${channel}: p50 ${pick(0.5)} ms, p99 ${pick(0.99)} ms, max ${ms.at(-1) ?? 0} ms; ${bad.length} slow or failed`);
process.exit(bad.length ? 1 : 0);
