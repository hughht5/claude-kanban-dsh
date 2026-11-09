// Column widths (card #130), on the real ui/colwidths.js in a vm: the drag rule, the weights read and
// saved, --w on every column, the edge's press, double press, dead zone and clean-up.
import test from "node:test";
import assert from "node:assert/strict";
import { boot } from "./harness.mjs";
import { Event } from "./dom.mjs";

const MIN = 160;
const close = (a, b, m) => assert.ok(Math.abs(a - b) < 1e-9, `${m || ""} ${a} != ${b}`);
const pe = (type, o = {}) => new Event(type, { bubbles: true, ...o });
const cols = (b) => [...b.document.querySelectorAll("#board > .col")];
const wOf = (el) => Number(el.style.getPropertyValue("--w"));
const grips = (b) => [...b.document.querySelectorAll("#board .colgrip")];
const drawn = () => { const b = boot(); b.show([]); return b; };
// Gives each column a measured width (the fake DOM has no layout).
const measure = (b, px) => cols(b).forEach((c, i) => { c._w = px[i] ?? MIN; });
const press = (g, x, t) => g.dispatchEvent(pe("pointerdown", { clientX: x, timeStamp: t }));

function rule(b) { return (widths, i, dx) => b.ctx.colWeightsAfterDrag(widths, i, dx); }

test("drag rule: takes from the neighbour, keeps the total, smallest weight 1", () => {
  const f = rule(drawn());
  const out = f([300, 300, 300], 0, 100);   // 400, 200, 300
  close(out[0], 2); close(out[1], 1); close(out[2], 1.5);
});

test("drag rule: a neighbour at the minimum stays there and the column still widens", () => {
  const f = rule(drawn());
  const out = f([MIN, MIN, MIN], 1, 80);    // 240, and the neighbour stays at 160
  close(out[1], 240 / MIN); close(out[2], 1); close(out[0], 1);
});

test("drag rule: never under the minimum; narrowing gives the width to the neighbour", () => {
  const f = rule(drawn());
  const out = f([400, 200], 0, -500);       // 160, then 200 + 240 = 440
  close(out[0], 1); close(out[1], 440 / MIN);
});

test("drag rule: the last column has no right edge", () => {
  const f = rule(drawn());
  const out = f([200, 400], 1, 50);
  close(out[0], 1); close(out[1], 2);
});

test("weights: missing, bad, zero, negative and huge values; the smallest becomes 1", () => {
  const b = drawn();
  const n = (m, k) => b.ctx.colNormalise(m, k);
  assert.deepEqual({ ...n({}, ["a", "b"]) }, { a: 1, b: 1 });
  assert.deepEqual({ ...n({ a: "x", b: 0, c: -3 }, ["a", "b", "c"]) }, { a: 1, b: 1, c: 1 });
  const x = n({ a: 4, b: 2 }, ["a", "b"]);
  close(x.a, 2); close(x.b, 1);
  const y = n({ a: 2, b: 3 }, ["a", "b", "c"]);        // a missing key counts as 1, so it is the smallest
  close(y.a, 2); close(y.b, 3); close(y.c, 1);
  close(n({ a: 1e9, b: 1 }, ["a", "b"]).a, 1e9);       // no cap
});

test("render sets --w on every column, the last one included, and an edge on all but the last", () => {
  const b = drawn();
  const cs = cols(b);
  assert.ok(cs.length >= 8);
  for (const c of cs) assert.equal(wOf(c), 1, c.dataset.col);
  assert.ok(cs.some((c) => c.dataset.col === "Archived"));
  assert.equal(grips(b).length, cs.length - 1);
  assert.equal(cs[cs.length - 1].querySelector(".colgrip"), null);
  assert.equal(cs[cs.length - 2].querySelector(".colgrip") !== null, true);
});

test("stored weights are read on render, with bad values as 1", () => {
  const b = boot();
  b.store.set("wb.colWeights", JSON.stringify({ Planning: 3, Backlog: 1.5, Done: "x" }));
  b.show([]);
  const by = Object.fromEntries(cols(b).map((c) => [c.dataset.col, wOf(c)]));
  close(by.Planning, 3); close(by.Backlog, 1.5); close(by.Blocked, 1);
  b.store.set("wb.colWeights", "not json");
  b.show([]);
  for (const c of cols(b)) assert.equal(wOf(c), 1);
});

test("weights saved in a wide window keep their ratios when the board is narrower", () => {
  const b = drawn();
  // A wide board: every column 300px, then column 0 dragged 150px wider (taking from column 1).
  measure(b, [300, 300, 300, 300, 300, 300, 300, 300]);
  const g = grips(b)[0];
  press(g, 100, 1000);
  b.fireWindow("pointermove", pe("pointermove", { clientX: 250 }));
  b.fireWindow("pointerup", pe("pointerup"));
  const saved = JSON.parse(b.store.get("wb.colWeights"));
  const vals = Object.values(saved);
  close(Math.min(...vals), 1);
  close(saved.Backlog / saved.Blocked, 450 / 160);   // the neighbour stops at the minimum
  close(saved.Blocked, 1);
});

test("a press and a release writes nothing; a wobble under the dead zone writes nothing", () => {
  const b = drawn();
  measure(b, new Array(9).fill(300));
  const g = grips(b)[1];
  press(g, 100, 1000);
  b.fireWindow("pointermove", pe("pointermove", { clientX: 102 }));
  b.fireWindow("pointerup", pe("pointerup"));
  assert.equal(b.store.get("wb.colWeights"), undefined);
});

test("a drag paints --w live, saves on release, and removes its window listeners", () => {
  const b = drawn();
  measure(b, new Array(9).fill(300));
  const g = grips(b)[0];
  press(g, 100, 1000);
  assert.equal(b.winListeners("pointermove"), 1);
  assert.ok(b.document.body.classList.contains("resizing"));
  b.fireWindow("pointermove", pe("pointermove", { clientX: 200 }));   // +100: 400, 200
  const [c0, c1] = cols(b);
  close(wOf(c0) / wOf(c1), 2);
  assert.equal(b.store.get("wb.colWeights"), undefined, "nothing saved until the release");
  b.fireWindow("pointerup", pe("pointerup"));
  assert.ok(b.store.get("wb.colWeights"));
  assert.equal(b.winListeners("pointermove"), 0);
  assert.equal(b.winListeners("pointerup"), 0);
  assert.equal(b.winListeners("pointercancel"), 0);
  assert.ok(!b.document.body.classList.contains("resizing"));
});

test("a redraw in the middle of a drag keeps the dragged widths on the new columns", () => {
  const b = drawn();
  measure(b, new Array(9).fill(300));
  press(grips(b)[0], 100, 1000);
  b.fireWindow("pointermove", pe("pointermove", { clientX: 200 }));
  b.show([]);                                                          // cards-changed: the columns are new elements
  const [c0, c1] = cols(b);
  close(wOf(c0) / wOf(c1), 2);
  b.fireWindow("pointermove", pe("pointermove", { clientX: 250 }));   // +150: 450, 150 -> clamped at 160
  close(wOf(cols(b)[0]) / wOf(cols(b)[1]), 450 / 160);   // the neighbour stops at the minimum
  b.fireWindow("pointerup", pe("pointerup"));
  assert.ok(b.store.get("wb.colWeights"));
});

test("pressing an edge twice within 400ms resets every width, even after a redraw between", () => {
  const b = boot();
  b.store.set("wb.colWeights", JSON.stringify({ Planning: 3 }));
  b.show([]);
  assert.equal(wOf(cols(b).find((c) => c.dataset.col === "Planning")), 3);
  press(grips(b)[2], 50, 1000);
  b.fireWindow("pointerup", pe("pointerup"));
  b.show([]);                                                          // a redraw between the two presses
  press(grips(b)[2], 50, 1250);
  assert.equal(b.store.get("wb.colWeights"), undefined);
  for (const c of cols(b)) assert.equal(wOf(c), 1);
});

test("two presses more than 400ms apart do not reset", () => {
  const b = boot();
  b.store.set("wb.colWeights", JSON.stringify({ Planning: 3 }));
  b.show([]);
  press(grips(b)[2], 50, 1000);
  b.fireWindow("pointerup", pe("pointerup"));
  press(grips(b)[2], 50, 1600);
  b.fireWindow("pointerup", pe("pointerup"));
  assert.ok(b.store.get("wb.colWeights"));
});

const pileCard = { id: "c110", number: 110, title: "Search sessions", column: "Deploy complete", version: 21, archiveAsk: { reason: "its work is live", at: "2026-11-08T10:00:00Z", by: "the router", column: "Deploy complete" } };
const isPile = (c) => c.className.includes("c-DonePile");

test("the Done pile column gets --w and an edge too, so Archive can be resized", () => {
  const b = boot();
  b.show([pileCard]);
  const cs = cols(b);
  assert.ok(cs.some(isPile), "the pile is drawn");
  for (const c of cs) assert.notEqual(c.style.getPropertyValue("--w"), "", c.dataset.col);
  assert.equal(grips(b).length, cs.length - 1);
  assert.equal(cs[cs.length - 1].dataset.col, "Archived");
  assert.equal(cs[cs.length - 1].querySelector(".colgrip"), null);
  assert.ok(cs[cs.length - 2].querySelector(".colgrip"), "the pile has an edge");
  b.store.set("wb.colWeights", JSON.stringify({ "Done pile": 2.5 }));
  b.show([pileCard]);
  close(wOf(cols(b).find(isPile)), 2.5);
});

test("index.html loads colwidths.js after every script that adds a column, as the harness does", async () => {
  const fs = await import("node:fs"), path = await import("node:path");
  const { UI, FILES } = await import("./harness.mjs");
  const order = [...fs.readFileSync(path.join(UI, "index.html"), "utf8").matchAll(/<script src="([\w.-]+\.js)"/g)].map((m) => m[1]);
  assert.ok(order.indexOf("colwidths.js") > order.indexOf("archive-ask.js"), "colwidths.js comes after archive-ask.js in index.html");
  assert.equal(FILES.indexOf("colwidths.js"), FILES.length - 1);
  const inHtml = FILES.filter((f) => order.includes(f)).sort((a, b) => order.indexOf(a) - order.indexOf(b));
  assert.deepEqual(FILES, inHtml, "the harness's FILES follow index.html's order");
});
