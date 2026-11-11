// Card #185: the line between a session pane's live log and its chat (ui/split.js).
import test from "node:test";
import assert from "node:assert/strict";
import { boot } from "./harness.mjs";

// A pane's parts with the heights a stacked panel would give them: log 200, chat 400.
function pane(b, { log = 200, chat = 400 } = {}) {
  const d = b.document;
  const grid = d.createElement("div"), feed = d.createElement("div"), right = d.createElement("div"), handle = d.createElement("div");
  grid.appendChild(feed); grid.appendChild(handle); grid.appendChild(right); d.body.appendChild(grid);
  const h = { log, chat };
  feed.getBoundingClientRect = () => ({ top: 0, left: 0, width: 300, height: grid.classList.contains("split-set") ? parseFloat(grid.style.getPropertyValue("--log-h")) : h.log, bottom: 0, right: 300 });
  right.getBoundingClientRect = () => {
    const l = feed.getBoundingClientRect().height;
    // The grid's room is fixed: a log set taller than it overflows and leaves the chat nothing.
    return { top: 0, left: 0, width: 300, height: Math.max(0, h.log + h.chat - l), bottom: 0, right: 300 };
  };
  return { grid, feed, right, handle, h };
}
const fire = (el, type, props = {}) => { const e = new b0.ctx.Event(type); Object.assign(e, props); el.dispatchEvent(e); return e; };
let b0;
const start = (opts) => { const b = boot({ files: ["split.js"] }); b0 = b; b.ctx.getComputedStyle = () => ({ display: "block" }); return b; };

test("limits: neither the log nor the chat goes below its minimum", () => {
  const b = start();
  const S = b.ctx.PaneSplit;
  assert.equal(S.clamp(10, 600), S.MIN_LOG);
  assert.equal(S.clamp(1000, 600), 600 - S.MIN_CHAT);
  assert.equal(S.clamp(250.4, 600), 250);
  // A tiny pane: the log keeps its minimum.
  assert.equal(S.clamp(300, 100), S.MIN_LOG);
  assert.equal(S.keyFor("router"), "rpane.split.router");
  assert.equal(S.keyFor("card"), "rpane.split.session");
  assert.equal(S.keyFor("sub"), "rpane.split.session");
});

test("keys: up and down move the line, shift moves it further, other keys do nothing", () => {
  const S = start().ctx.PaneSplit;
  assert.equal(S.keyStep("ArrowDown", false, 200, 600), 200 + S.STEP);
  assert.equal(S.keyStep("ArrowUp", false, 200, 600), 200 - S.STEP);
  assert.equal(S.keyStep("ArrowUp", true, 200, 600), 200 - S.BIG);
  assert.equal(S.keyStep("Home", false, 200, 600), S.MIN_LOG);
  assert.equal(S.keyStep("End", false, 200, 600), 600 - S.MIN_CHAT);
  assert.equal(S.keyStep("ArrowDown", false, 600 - S.MIN_CHAT, 600), 600 - S.MIN_CHAT);
  assert.equal(S.keyStep("a", false, 200, 600), null);
});

test("drag: moves the split, keeps focus, is saved per panel type, double-click resets", () => {
  const b = start();
  const p = pane(b);
  const s = b.ctx.PaneSplit.attach({ handle: p.handle, grid: p.grid, feed: p.feed, right: p.right, kind: "card" });
  assert.equal(p.grid.classList.contains("split-set"), false); // nothing saved: the default split
  const down = fire(p.handle, "pointerdown", { button: 0, clientY: 300, pointerId: 1 });
  assert.equal(down.defaultPrevented, true); // the focus stays in the message box
  b.fireWindow("pointermove", { clientY: 380 });
  assert.equal(p.grid.style.getPropertyValue("--log-h"), "280px");
  b.fireWindow("pointermove", { clientY: 2000 }); // past the chat's minimum
  assert.equal(p.grid.style.getPropertyValue("--log-h"), (600 - b.ctx.PaneSplit.MIN_CHAT) + "px");
  b.fireWindow("pointermove", { clientY: 350 });
  b.fireWindow("pointerup", {});
  assert.equal(b.winListeners("pointermove"), 0);
  assert.equal(b.store.get("rpane.split.session"), "250");
  assert.equal(b.store.has("rpane.split.router"), false); // the Router's is its own
  assert.equal(p.handle.getAttribute("aria-valuenow"), "250");
  // A new pane of the same type opens at the saved split.
  const q = pane(b);
  b.ctx.PaneSplit.attach({ handle: q.handle, grid: q.grid, feed: q.feed, right: q.right, kind: "sub" });
  assert.equal(q.grid.style.getPropertyValue("--log-h"), "250px");
  // Double-click: back to the default, and nothing kept.
  fire(p.handle, "dblclick");
  assert.equal(p.grid.classList.contains("split-set"), false);
  assert.equal(b.store.has("rpane.split.session"), false);
  // A right-button press doesn't start a drag.
  fire(p.handle, "pointerdown", { button: 2, clientY: 300 });
  assert.equal(b.winListeners("pointermove"), 0);
  assert.equal(s.height(), 200);
});

test("keyboard on the focused handle, and a saved split refitted to a smaller pane", () => {
  const b = start();
  const p = pane(b);
  const s = b.ctx.PaneSplit.attach({ handle: p.handle, grid: p.grid, feed: p.feed, right: p.right, kind: "router" });
  const k = fire(p.handle, "keydown", { key: "ArrowDown" });
  assert.equal(k.defaultPrevented, true);
  assert.equal(p.grid.style.getPropertyValue("--log-h"), (200 + b.ctx.PaneSplit.STEP) + "px");
  assert.equal(b.store.get("rpane.split.router"), String(200 + b.ctx.PaneSplit.STEP));
  const tab = fire(p.handle, "keydown", { key: "Tab" });
  assert.equal(tab.defaultPrevented, false); // Tab still moves on
  // The window gets smaller: the saved height is cut so the chat keeps its minimum.
  // The log is still set to the old window's 500 px, so it overflows: the chat measures 0.
  b.store.set("rpane.split.router", "500");
  s.apply(500);
  p.h.log = 100; p.h.chat = 200;
  assert.equal(p.right.getBoundingClientRect().height, 0);
  s.refit();
  assert.equal(p.right.getBoundingClientRect().height, b.ctx.PaneSplit.MIN_CHAT);
  assert.equal(p.grid.style.getPropertyValue("--log-h"), (300 - b.ctx.PaneSplit.MIN_CHAT) + "px");
  // Folded or side by side (the handle hidden): the default layout, nothing forced.
  b.ctx.getComputedStyle = () => ({ display: "none" });
  s.refit();
  assert.equal(p.grid.classList.contains("split-set"), false);
});
