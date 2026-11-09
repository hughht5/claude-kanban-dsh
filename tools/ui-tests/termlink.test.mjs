// The terminal link (card #95), on the real ui/termlink.js in a vm with a fake xterm and a fake
// app: it replays a session once per run, never on every refresh; it skips live chunks its
// snapshot holds; it drops xterm's answers to old output during a replay; and a key that can't
// go through says so.
import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import vm from "node:vm";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const src = fs.readFileSync(path.join(here, "..", "..", "ui", "termlink.js"), "utf8");
const b64 = (s) => Buffer.from(s, "utf8").toString("base64");
const flush = () => new Promise((r) => setImmediate(r));

function boot({ run = "row1", ring = "", end = ring.length } = {}) {
  const app = { run, ring, end, calls: [], inputs: [], refuse: null, ptyHandler: null };
  const invoke = async (cmd, args) => {
    app.calls.push(cmd);
    if (cmd === "session_run") return app.run;
    if (cmd === "session_snapshot") return app.run ? { run: app.run, end: app.end, data: b64(app.ring) } : null;
    if (cmd === "session_resize") return null;
    if (cmd === "session_input") { if (app.refuse) throw new Error(app.refuse); app.inputs.push(args.data); return null; }
    throw new Error("unexpected " + cmd);
  };
  const ctx = {
    atob: (s) => Buffer.from(s, "base64").toString("latin1"),
    Uint8Array, Date, Set, Promise, String, RegExp,
    window: { __TAURI__: { core: { invoke }, event: { listen: (name, f) => { if (name === "pty") app.ptyHandler = f; return Promise.resolve(() => {}); } } } },
  };
  vm.createContext(ctx);
  vm.runInContext(src, ctx);
  // A fake xterm: keeps what is written as text, and fires onData like a key press.
  const term = {
    cols: 80, rows: 24, text: "", resets: 0, onDataF: null, doneF: null,
    onData(f) { this.onDataF = f; },
    reset() { this.resets++; this.text = ""; },
    write(d, done) { this.text += typeof d === "string" ? d : Buffer.from(d).toString("utf8"); if (done) this.doneF = done; },
    key(d) { this.onDataF(d); },
  };
  const L = ctx.termLink(term, "card1", "No session is running.");
  const pty = (data, end, r = app.run) => app.ptyHandler({ payload: { card: "card1", run: r, end, data: b64(data) } });
  return { ctx, app, term, L, pty };
}

test("it replays once per run, not on every refresh", async () => {
  const { ctx, app, term, L } = boot({ ring: "screen" });
  await ctx.termAttach(L);
  assert.equal(term.text, "screen");
  assert.equal(term.resets, 1);
  for (let i = 0; i < 5; i++) await ctx.termAttach(L); // refreshInfo on each hook and send
  assert.equal(term.resets, 1, "no reset while the run is the same");
  assert.equal(app.calls.filter((c) => c === "session_snapshot").length, 1);
  app.run = "row2"; app.ring = "new run"; app.end = 7;
  await ctx.termAttach(L);
  assert.equal(term.text, "new run");
  assert.equal(term.resets, 2);
});

test("live output the snapshot already holds is skipped; what came after is drawn", async () => {
  const { ctx, term, L, pty } = boot({ ring: "abc", end: 3 });
  await ctx.termAttach(L);
  pty("c", 3); // already in the snapshot
  pty("d", 4);
  assert.equal(term.text, "abcd");
});

test("output that arrives while the snapshot loads waits, and nothing is lost or doubled", async () => {
  const { ctx, term, L, pty, app } = boot({ ring: "ab", end: 2 });
  const p = ctx.termAttach(L);
  await flush(); // session_run answered; the snapshot is on its way
  app.ring = "abc"; app.end = 3; // the session wrote "c" before the snapshot was taken
  pty("c", 3);
  pty("d", 4); // and "d" after it
  await p;
  assert.equal(term.text, "abcd");
});

test("xterm's answers to old output during a replay are not typed into the session", async () => {
  const { ctx, term, L, app } = boot({ ring: "\x1b[6n" });
  await ctx.termAttach(L);
  term.key("\x1b[24;1R"); // xterm answering the replayed cursor-position query
  term.key("\x1b]11;rgb:0f0f/1111/1717\x1b\\"); // and a colour query
  assert.deepEqual(app.inputs, []);
  term.key("h"); // Hugh's own key, while the replay still draws
  await flush();
  assert.deepEqual(app.inputs, ["h"]);
  term.doneF(); // the replay has drawn: now a report is a real answer to the live session
  term.key("\x1b[5;1R");
  await flush();
  assert.deepEqual(app.inputs, ["h", "\x1b[5;1R"]);
});

test("keys typed as the terminal opens wait for it to attach, then go in, in order", async () => {
  const { ctx, term, L, app } = boot({ ring: "" });
  const p = ctx.termAttach(L);
  term.key("a"); term.key("b");
  assert.deepEqual(app.inputs, []);
  assert.doesNotMatch(term.text, /No session is running/);
  await p; await flush();
  assert.deepEqual(app.inputs, ["a", "b"]);
});

test("a new run's first output during an attach that found no session is not lost", async () => {
  const { ctx, term, L, app, pty } = boot({ run: null });
  const p = ctx.termAttach(L);
  await flush(); // session_run said: no session
  app.run = "row9"; app.ring = "hello"; app.end = 5;
  pty("hello", 5, "row9");
  await p; await flush(); await flush(); await flush();
  assert.equal(term.text, "hello");
});

test("a key with no running session, or one the app refuses, shows a note", async () => {
  const { ctx, term, L, app } = boot({ run: null });
  await ctx.termAttach(L);
  assert.match(term.text, /No session is running/);
  term.text = "";
  term.key("x");
  assert.match(term.text, /No session is running/);
  assert.deepEqual(app.inputs, []);

  const b = boot({ ring: "" });
  await b.ctx.termAttach(b.L);
  b.app.refuse = "the pipe is closed";
  b.term.key("y");
  await flush();
  assert.match(b.term.text, /Not sent to the session: the pipe is closed/);
});

test("a chunk from a new run replays that run instead of mixing it in", async () => {
  const { ctx, term, L, pty, app } = boot({ ring: "old", end: 3 });
  await ctx.termAttach(L);
  app.run = "row2"; app.ring = "fresh"; app.end = 5;
  pty("fresh", 5, "row2");
  await flush(); await flush(); await flush();
  assert.equal(term.text, "fresh");
});
