// Test apps per card (card #147): the tile chip's choice (KBTestApp.tileChip, pure), the header chip,
// the popover's Release and Free, the banner, the card details row and the settings box, on the real
// ui/*.js files in a vm (harness.mjs).
import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import vm from "node:vm";
import { boot, FILES, UI } from "./harness.mjs";

const SRC = fs.readFileSync(path.join(UI, "testapp.js"), "utf8");
const NOW = Date.now();
const iso = (minsAgo) => new Date(NOW - minsAgo * 60000).toISOString();
const later = (mins) => new Date(NOW + mins * 60000).toISOString();
const tick = async (n = 6) => { for (let i = 0; i < n; i++) await new Promise((r) => setImmediate(r)); };

// testapp.js alone, with no Tauri: only the pure parts run.
function pure() {
  const ctx = { console };
  ctx.window = ctx;
  vm.createContext(ctx);
  vm.runInContext(SRC, ctx, { filename: "testapp.js" });
  return ctx.KBTestApp;
}

const app = (slot, since) => ({ slot, channel: slot === 1 ? "test" : "test" + slot, name: "Claude Kanban DSH Test" + (slot === 1 ? "" : " " + slot),
  label: slot === 1 ? "Test" : "Test " + slot, port: 9333 + slot, since: since || iso(30) });
function cardsFixture() {
  return [
    { id: "card-140", number: 140, title: "Owns Test 2, testing", column: "In progress", testApp: app(2), updatedAt: iso(1) },
    { id: "card-141", number: 141, title: "Owns Test 3, in its line", column: "In progress", testApp: app(3), updatedAt: iso(1) },
    { id: "card-142", number: 142, title: "Owns Test, nothing else", column: "Human test", testApp: app(1), updatedAt: iso(1) },
    { id: "card-150", number: 150, title: "Waits for a test app", column: "In progress", updatedAt: iso(1) },
    { id: "card-151", number: 151, title: "Waits too", column: "In progress", updatedAt: iso(1) },
    { id: "card-160", number: 160, title: "Stack lead", column: "In progress", stack: [161], testApp: app(4), updatedAt: iso(1) },
    { id: "card-161", number: 161, title: "Stack child", column: "In progress", stackOf: 160, updatedAt: iso(1) },
    { id: "card-170", number: 170, title: "Nothing to do with test apps", column: "In progress", updatedAt: iso(1) },
  ];
}
const holder = (card, kind = "tester", extra = {}) => ({ kind, card, cardId: card ? "card-" + card : null, owner: card ? String(card) : "router",
  label: card ? "#" + card : "wave 3", givenAt: iso(3), renewedAt: iso(1), expiresAt: later(57), heldMinutes: 3, overdue: false, ...extra });
const slot = (n, card, more = {}) => ({ slot: n, channel: n === 1 ? "test" : "test" + n, label: n === 1 ? "Test" : "Test " + n,
  name: "Claude Kanban DSH Test" + (n === 1 ? "" : " " + n), port: 9333 + n,
  card: card ? { number: card, cardId: "card-" + card, label: "#" + card, since: iso(30) } : null,
  holder: null, line: [], install: null, pending: null, hughInputAt: null, hughInputRecent: false, otherBoard: null, ...more });
function statusFixture() {
  return {
    channel: "live", limitMinutes: 60, ceiling: 10,
    slots: [
      slot(1, 142),
      slot(2, 140, { holder: holder(140), install: { owner: "router", label: "wave 3", card: null, phase: "waiting", waitingFor: "Hugh's input", restartAt: later(5), channel: "test2" },
        pending: { label: "wave 3" }, hughInputAt: iso(2) }),
      slot(3, 141, { holder: holder(null, "install"), line: [{ kind: "tester", card: 141, cardId: "card-141", owner: "141", label: "#141", place: 1, live: true, askedAt: iso(4) }] }),
      slot(4, 160),
    ],
    slotLine: [
      { card: 151, cardId: "card-151", label: "#151", place: 1, askedAt: iso(9), live: true },
      { card: 150, cardId: "card-150", label: "#150", place: 2, askedAt: iso(5), live: true },
    ],
    pending: { label: "wave 3", slot: 2, slotLabel: "Test 2" },
    hold: null, allowUntil: null, mkdir: null,
  };
}

test("tileChip: a card that owns a test app, with nothing else, names it", () => {
  const K = pure(), all = cardsFixture(), s = statusFixture();
  const w = K.tileChip(all[2], all, s);
  assert.equal(w.t, "Test");
  assert.equal(w.c, "tap-own");
  assert.equal(w.title, "This card's build goes in Claude Kanban DSH Test (CDP port 9334).");
  // With no status yet the owner's chip still shows; the port falls back to 9333 + slot.
  const bare = K.tileChip({ id: "x", number: 9, testApp: { slot: 5 } }, [], null);
  assert.deepEqual([bare.t, bare.c, bare.title], ["Test 5", "tap-own", "This card's build goes in Claude Kanban DSH Test 5 (CDP port 9338)."]);
});

test("tileChip: the card whose session has its test app's turn says testing", () => {
  const K = pure(), all = cardsFixture(), s = statusFixture();
  const w = K.tileChip(all[0], all, s);
  assert.equal(w.t, "Test 2 · testing");
  assert.equal(w.c, "tap-hold");
  assert.equal(w.title, "This card's build goes in Claude Kanban DSH Test 2 (CDP port 9335). Its session has the turn now.");
  s.slots[1].holder.overdue = true;
  assert.equal(K.tileChip(all[0], all, s).c, "tap-hold tap-over");
});

test("tileChip: a card in its own test app's line says its place", () => {
  const K = pure(), all = cardsFixture(), s = statusFixture();
  const w = K.tileChip(all[1], all, s);
  assert.equal(w.t, "Test 3 · 1st in line");
  assert.equal(w.c, "tap-wait");
  assert.match(w.title, /^This card's build goes in Claude Kanban DSH Test 3 \(CDP port 9336\)\. Its session is 1st in line for a turn\.$/);
  s.slots[2].line[0].live = false;
  assert.equal(K.tileChip(all[1], all, s).c, "tap-paused");
  // Card #168: its turn came while its session was stopped, and the app is starting it.
  s.slots[2].line[0].waking = true;
  const k = K.tileChip(all[1], all, s);
  assert.equal(k.t, "Test 3 · starting");
  assert.equal(k.c, "tap-wait");
  assert.match(k.title, /its turn has come, and the app is starting its stopped session\.$/);
});

test("tileChip: a card waiting for a free test app says its place in that line", () => {
  const K = pure(), all = cardsFixture(), s = statusFixture();
  const w = K.tileChip(all[3], all, s);
  assert.equal(w.t, "Test app: 2nd in line");
  assert.equal(w.c, "tap-wait");
  assert.match(w.title, /Every test app is taken: this card waits for one/);
  assert.equal(K.tileChip(all[4], all, s).t, "Test app: 1st in line");
});

test("tileChip: a stack's child shows its lead's test app", () => {
  const K = pure(), all = cardsFixture(), s = statusFixture();
  const w = K.tileChip(all[6], all, s);
  assert.equal(w.t, "Test 4 · stack");
  assert.equal(w.c, "tap-stack");
  assert.equal(w.title, "Hugh tests this stack in Claude Kanban DSH Test 4, on lead #160's install.");
  // A lead with no test app: nothing.
  const noApp = all.map((c) => (c.number === 160 ? { ...c, testApp: undefined } : c));
  assert.equal(K.tileChip(noApp[6], noApp, s), null);
});

test("tileChip: a card with nothing to do with test apps gets no chip", () => {
  const K = pure(), all = cardsFixture();
  assert.equal(K.tileChip(all[7], all, statusFixture()), null);
  assert.equal(K.tileChip(all[7], all, null), null);
  assert.equal(K.tileChip(null, all, null), null);
});

test("tileChip and headText read an older status (card #115) as slot 1", () => {
  const K = pure(), c = { id: "card-140", number: 140 };
  const old = { limitMinutes: 60, holder: holder(140), line: [{ kind: "tester", card: 141, cardId: "card-141", place: 1, live: true }], install: null, pending: null };
  assert.equal(K.tileChip(c, [c], old).t, "Test · testing");
  assert.equal(K.tileChip({ id: "card-141", number: 141 }, [], old).t, "Test · 1st in line");
  assert.equal(K.headText(old), "Test apps: #140 testing");
  assert.equal(K.headText({ slots: [], slotLine: [] }), "Test apps");
});

test("headText counts the test apps with a card and the cards waiting for one", () => {
  const K = pure();
  assert.equal(K.headText(statusFixture()), "Test apps: 4 in use · 2 waiting");
  assert.equal(K.headText({ slots: [slot(2, null, { holder: holder(null, "install") })], slotLine: [] }), "Test apps: wave 3 install");
});

// The whole page: board.js and friends, then testapp.js (MutationObserver stubbed; the test runs it).
function page(status, extraInvoke = {}) {
  const invoke = { testapp_status: status, app_channel: "live", testapp_release: (a) => status, testapp_free: (a) => status, ...extraInvoke };
  const b = boot({ invoke });
  let mo = null;
  b.ctx.MutationObserver = class { constructor(f) { mo = f; } observe() {} };
  vm.runInContext(SRC, b.ctx, { filename: "testapp.js" });
  b.decorate = () => mo && mo();
  return b;
}

test("the page: header chip, tile chips, banner; Release and Free ask twice and name the slot", async () => {
  const s = statusFixture(), b = page(s);
  await tick();
  b.show(cardsFixture());
  b.decorate();
  const chip = b.document.getElementById("testappChip");
  assert.equal(chip.textContent, "Test apps: 4 in use · 2 waiting");
  assert.equal(b.document.querySelector(".tapwrap").hidden, false);
  assert.equal(chip.classList.contains("warn"), false);
  // Each tile's test app is on its status chip (doing.js, card #150): no chip of testapp.js's own.
  const tap = (id) => { const el = b.card(id).querySelector(".chip.doing"); return el && [el.textContent, el.dataset.doing]; };
  assert.deepEqual(tap("card-140"), ["Test 2 · testing", "tapp"]);
  assert.deepEqual(tap("card-141"), ["Test 3 · 1st in line", "tline"]);
  assert.deepEqual(tap("card-142"), ["Test", "own"]);
  assert.deepEqual(tap("card-150"), ["Test app: 2nd in line", "tline"]);
  assert.deepEqual(tap("card-161"), ["Test 4 · stack", "own"]);
  assert.equal(tap("card-170"), null);
  assert.equal(b.document.querySelectorAll(".tapcard").length, 0);
  assert.match(b.card("card-140").querySelector(".chip.doing").title, /Claude Kanban DSH Test 2 \(CDP port 9335\)\. Its session has the turn now\./);
  // A card with a session and its own test app: what it does, then where to test it.
  b.ctx.__d = { "card-142": { state: "idle", since: iso(4), turnSince: null, running: [], testing: false } };
  b.run("doingLoaded=true;doing=__d;Doing.doingPaint()");
  assert.deepEqual(tap("card-142"), ["Idle 4m · Test", "idle"]);
  // The banner names the slot and what its install waits for.
  const bar = b.document.getElementById("testappBar");
  assert.equal(bar.hidden, false);
  assert.equal(bar.querySelector(".ut").textContent, "Test app restart pending (wave 3) · Test 2 · waiting for Hugh's input");
  // The popover: a row per slot, then the cards waiting for a test app.
  chip.click();
  const pop = b.document.getElementById("testappPop");
  const rows = pop.querySelectorAll(".tapslot");
  assert.deepEqual(rows.map((r) => r.dataset.slot), ["1", "2", "3", "4"]);
  assert.equal(rows[1].querySelector(".tapname").textContent, "Test 2 · port 9335");
  assert.match(rows[1].textContent, /Turn: #140 · testing · held for 3 min/);
  assert.match(rows[1].textContent, /Install: wave 3 · waiting · waiting for Hugh's input/);
  assert.match(rows[1].textContent, /Your last input in it: /);
  assert.match(rows[2].textContent, /Waiting for a turn1st#141 · testing/);
  assert.equal(rows[0].querySelector('[data-act="release-1"]'), null, "no turn on Test: no Release");
  assert.ok(rows[0].querySelector('[data-act="free-1"]'));
  assert.match(pop.textContent, /Waiting for a test app1st#1512nd#150/);
  assert.match(pop.querySelector(".taphead").textContent, /60 min a turn · at most 10 test apps/);
  // Free on Test 2: the first click asks, the second frees slot 2.
  pop.querySelector('[data-act="free-2"]').click();
  assert.equal(pop.querySelector('[data-act="free-2"]').textContent, "Free Test 2?");
  assert.equal(b.calls.filter((c) => c[0] === "testapp_free").length, 0);
  pop.querySelector('[data-act="free-2"]').click();
  await tick();
  assert.deepEqual(b.calls.filter((c) => c[0] === "testapp_free").map((c) => JSON.stringify(c[1])), ['{"slot":2}']);
  // Release on Test 3 (an install turn with no card) names slot 3.
  pop.querySelector('[data-act="release-3"]').click();
  assert.equal(pop.querySelector('[data-act="release-3"]').textContent, "Release wave 3?");
  pop.querySelector('[data-act="release-3"]').click();
  await tick();
  assert.deepEqual(b.calls.filter((c) => c[0] === "testapp_release").map((c) => JSON.stringify(c[1])), ['{"slot":3}']);
  // Nothing here asks for the window's focus or a toast.
  assert.ok(!b.calls.some((c) => /focus|toast|notify/i.test(c[0])));
});

test("the page: an overdue holder turns the header chip amber; an older backend shows nothing", async () => {
  const s = statusFixture();
  s.slots[1].holder.overdue = true;
  const b = page(s);
  await tick();
  assert.equal(b.document.getElementById("testappChip").classList.contains("warn"), true);
  // No testapp_status (an older backend): the chip, the banner and the tile chips stay away.
  const old = boot({ invoke: { testapp_status: () => { throw new Error("unknown command"); }, app_channel: () => { throw new Error("unknown command"); } } });
  old.ctx.MutationObserver = class { observe() {} };
  assert.doesNotThrow(() => vm.runInContext(SRC, old.ctx, { filename: "testapp.js" }));
  await tick();
  old.show(cardsFixture().map(({ testApp, ...c }) => c));
  assert.equal(old.document.querySelector(".tapwrap").hidden, true);
  assert.equal(old.document.getElementById("testappBar").hidden, true);
  assert.equal(old.document.querySelectorAll(".tapcard").length, 0);
});

test("the page: Hugh's input is recorded in every test channel, never on live", async () => {
  for (const [ch, want] of [["live", false], ["test", true], ["test2", true], ["test10", true]]) {
    const b = boot({ invoke: { testapp_status: statusFixture(), app_channel: ch } });
    const seen = [];
    b.ctx.MutationObserver = class { observe() {} };
    b.ctx.addEventListener = (t) => seen.push(t);
    vm.runInContext(SRC, b.ctx, { filename: "testapp.js" });
    await tick();
    assert.equal(seen.includes("keydown"), want, ch);
  }
});

test("card details: a Test app row after Branch", () => {
  const b = boot();
  const all = cardsFixture();
  b.show(all);
  const el = b.document.createElement("div");
  el.innerHTML = `<div id="p-det"></div><div class="ph"><span class="ttl"></span></div>`;
  b.document.body.appendChild(el);
  b.ctx.panel = { el, id: "card-140" };
  b.run("renderDetail()");
  const dts = el.querySelectorAll("dt").map((x) => x.textContent);
  const i = dts.indexOf("Test app");
  assert.ok(i > 0 && dts[i - 1] === "Branch", dts.join(", "));
  const dd = el.querySelectorAll("dd")[el.querySelectorAll("dt").findIndex((x) => x.textContent === "Test app")];
  assert.match(dd.textContent, /^Claude Kanban DSH Test 2 · channel test2 · CDP port 9335 · since \d\d:\d\d$/);
  // No test app: no row.
  b.ctx.panel = { el, id: "card-170" };
  b.run("renderDetail()");
  assert.ok(!el.querySelectorAll("dt").some((x) => x.textContent === "Test app"));
});

test("settings, Sessions: Test apps, at most (testapp.slots, 1 to 10)", async () => {
  const sets = [];
  const b = boot({
    files: [...FILES, "settings.js"],
    invoke: {
      get_setting: ({ key }) => ({ key, value: key === "testapp.slots" ? "4" : key === "sessions.small_goes_on_repos" ? "[]" : "3", default: key === "testapp.slots" ? "10" : "3" }),
      set_setting: ({ key, value }) => { sets.push([key, value]); return value; },
      get_setting_log: [],
    },
  });
  b.run(`openSettings("sessions")`);
  await tick(20);
  const box = b.document.querySelector('[data-setting="testapp-slots"]');
  assert.ok(box, "the box is there");
  assert.match(box.querySelector("label").textContent, /^Test apps, at most/);
  assert.equal(box.querySelector("#st-tapps").value, "4");
  assert.match(box.querySelector(".meta").textContent, /^The highest-numbered test app the board gives out\. .* 1 to 10 \(default 10\)\.$/);
  // Straight after Max running sessions.
  const boxes = b.document.querySelectorAll(".st-box");
  assert.ok(boxes[0].querySelector("#st-max") && boxes[1] === box);
  box.querySelector("#st-tapps").value = "11";
  box.querySelector("#st-tasave").click();
  await tick();
  assert.equal(box.querySelector("#st-tamsg").textContent, "Give a whole number from 1 to 10.");
  box.querySelector("#st-tapps").value = "6";
  box.querySelector("#st-tasave").click();
  await tick();
  assert.deepEqual(sets, [["testapp.slots", "6"]]);
  assert.equal(box.querySelector("#st-tamsg").textContent, "Saved");
});
