// Loads the board's own files (ui/*.js) into a node vm context with the tiny DOM (dom.mjs), stub
// Tauri calls and no timers, so a test can set cards and call render() (card #120).
import vm from "node:vm";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { makeDocument, Event } from "./dom.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
export const UI = path.resolve(here, "..", "..", "ui");

// The files a board face needs, in index.html's order (panel.js, stacks.js and the rest are left out).
export const FILES = ["labels.js", "board.js", "small.js", "needs.js", "check-size.js", "steps.js", "done-when.js", "waits.js", "doing.js", "changes.js", "filterbar.js", "face.js", "archive-ask.js", "colwidths.js"];

// `storage`: a localStorage for the page (card #132's held places), else an in-memory one, `store` (card #130).
export function boot({ files = FILES, invoke = {}, storage = null } = {}) {
  const html = fs.readFileSync(path.join(UI, "index.html"), "utf8");
  const at = html.indexOf("<body>") + 6, body = html.slice(at, html.indexOf("<script", at));
  const document = makeDocument(body);
  const calls = [];
  const store = new Map(), winLs = {};
  const ctx = {
    document, Event, console,
    location: { hash: "" },
    localStorage: storage || { getItem: (k) => (store.has(k) ? store.get(k) : null), setItem: (k, v) => { store.set(k, String(v)); }, removeItem: (k) => { store.delete(k); } },
    CSS: { escape: (s) => String(s) },
    navigator: { clipboard: { writeText: async () => {} } },
    getSelection: () => ({ removeAllRanges() {}, addRange() {} }),
    setTimeout: () => 0, clearTimeout() {}, setInterval: () => 0, clearInterval() {},
    queueMicrotask,
  };
  ctx.window = ctx; ctx.globalThis = ctx; ctx.self = ctx;
  ctx.addEventListener = (t, f) => { (winLs[t] ||= []).push(f); };
  ctx.removeEventListener = (t, f) => { const a = winLs[t]; if (a && a.includes(f)) a.splice(a.indexOf(f), 1); };
  ctx.__TAURI__ = {
    core: { invoke: async (cmd, args) => { calls.push([cmd, args]); if (cmd in invoke) return typeof invoke[cmd] === "function" ? invoke[cmd](args) : invoke[cmd]; if (cmd === "list_cards") return []; if (cmd === "needs_you") return {}; return null; } },
    event: { listen: async () => () => {} },
  };
  vm.createContext(ctx);
  for (const f of files) vm.runInContext(fs.readFileSync(path.join(UI, f), "utf8"), ctx, { filename: f });
  const run = (code) => vm.runInContext(code, ctx);
  return {
    ctx, document, calls, run, store,
    // Window listeners the page added (pointermove and the like), and a way to fire them.
    winListeners: (t) => (winLs[t] || []).length,
    fireWindow: (t, e) => { for (const f of (winLs[t] || []).slice()) f(e); },
    // Sets the board's cards (and optionally needs.js's `needs` and the filter text), then renders.
    show(cards, { needs, q } = {}) {
      ctx.__cards = cards; ctx.__needs = needs || {}; ctx.__q = q || "";
      run(`cards=__cards;loaded=true;if(typeof needs!=="undefined")needs=__needs;q=__q;document.getElementById("q").value=__q;render()`);
    },
    card: (id) => document.querySelector(`.card[data-id="${id}"]`),
  };
}

// The kinds of a card's chip row, in order (face.js's own names).
export const rowKinds = (b, el) => [...el.querySelector(":scope > .tags").children].map((x) => x.classList.contains("timein") ? "time" : b.ctx.Face.chipKind(x));
