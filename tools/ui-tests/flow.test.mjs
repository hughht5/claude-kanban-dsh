// A card's flow (card #162): its chips on the card face, the Card flow box in its details (switches,
// card #178; each change sent alone), the Add card form's choice and the defaults in Settings ›
// Sessions, on the real ui/*.js files in a vm (harness.mjs).
import test from "node:test";
import assert from "node:assert/strict";
import { boot, FILES } from "./harness.mjs";

const tick = async (n = 20) => { for (let i = 0; i < n; i++) await new Promise((r) => setImmediate(r)); };
const at = new Date().toISOString();

// The defaults as the board has them: the critic off (as on the live board), skip Human test on.
const DEFAULTS = { "sessions.critic_before_build": "off", "sessions.skip_human_test": "on", "sessions.skip_planning": "off" };

function board(extra = {}) {
  const updates = [], sets = [];
  const b = boot({
    files: [...FILES, "subagents.js", "settings.js"],
    invoke: {
      get_setting: ({ key }) => ({ key, value: DEFAULTS[key] ?? (key.startsWith("sessions.skip_") ? "off" : "on"), default: key.startsWith("sessions.skip_") ? "off" : "on" }),
      set_setting: ({ key, value }) => { sets.push([key, value]); return value; },
      get_setting_log: () => [],
      update_card: (a) => { updates.push(a.patch); return { id: a.id, version: 2 }; },
      ...extra,
    },
  });
  return { b, updates, sets };
}

const card = (n, f = {}) => ({ id: "card-" + n, number: n, title: "Card " + n, column: "In progress", updatedAt: at, version: 1, ...f });
const chips = (b, n) => b.card("card-" + n).querySelectorAll(".flow-chip").map((x) => x.textContent);

test("the card face shows a chip for each override that differs from the default", async () => {
  const { b } = board();
  await tick();
  b.show([
    card(1, { skipPlanning: true, subagents: { critic: true, tester: false } }),
    card(2, { skipHumanTest: false, subagents: { critic: false, router: true } }),
    card(3, { skipHumanTest: true }),
    card(4, { skipPlanning: true, stackOf: 3 }),
    card(5),
  ]);
  assert.deepEqual(chips(b, 1), ["No plan", "Critic on", "Tester off"]);
  assert.deepEqual(chips(b, 2), ["Human test on"], "the critic off and router on are the defaults: no chip");
  assert.deepEqual(chips(b, 3), [], "skip Human test is the default");
  assert.deepEqual(chips(b, 4), [], "a stack's card never skips");
  assert.deepEqual(chips(b, 5), []);
  const c1 = b.card("card-1").querySelector('.flow-chip[data-flow-chip="skipPlanning"]');
  assert.ok(c1.classList.contains("tag"));
  assert.equal(c1.title, "Skip Planning: on for this card (the default is off)");
});

test("the Card flow box: each option is a switch showing what holds, and a click sends only that option", async () => {
  const { b, updates } = board();
  await tick();
  const c = card(7, { skipPlanning: false, subagents: { tester: false } });
  b.show([c]);
  b.ctx.__c = c;
  const host = b.document.createElement("div");
  b.document.body.appendChild(host);
  host.innerHTML = b.run(`subagentsBox(__c)`);
  const box = host.querySelector(".flow-box");
  assert.equal(box.querySelector("h3").textContent, "Card flow");
  assert.equal(box.querySelectorAll("select").length, 0, "no dropdowns any more (card #178)");
  const sw = (k) => box.querySelector(`.flow-tog[data-flow="${k}"]`);
  const ctl = (k) => sw(k).parentNode;
  const state = (k) => [sw(k).getAttribute("role"), sw(k).getAttribute("aria-checked"), sw(k).dataset.own, ctl(k).querySelector(".flow-src") ? "Default" : ctl(k).querySelector(".flow-reset").textContent];
  assert.deepEqual(state("skipPlanning"), ["switch", "false", "off", "Reset"], "the card's own off");
  assert.deepEqual(state("skipHumanTest"), ["switch", "true", "default", "Default"], "follows the default, on");
  assert.match(ctl("skipHumanTest").querySelector(".flow-src").title, /on now/);
  assert.deepEqual(state("tester"), ["switch", "false", "off", "Reset"]);
  assert.deepEqual(state("critic"), ["switch", "false", "default", "Default"], "the default here is off");
  assert.deepEqual(state("reviewer"), ["switch", "true", "default", "Default"]);
  assert.equal(box.querySelectorAll(".flow-tog[data-flow]").length, 7, "two skips and five subagents");
  assert.match(box.querySelector(".meta").textContent, /Now: Skip Human test; subagents off: card-critic, card-tester\./);
  // One click each: only that key; Reset sends null (Default).
  sw("critic").querySelector(".flow-knob").click();
  await tick();
  assert.deepEqual(state("critic"), ["switch", "true", "on", "Reset"], "a flip makes it the card's own");
  sw("skipHumanTest").click();
  await tick();
  ctl("tester").querySelector(".flow-reset").click();
  await tick();
  assert.deepEqual(state("tester"), ["switch", "true", "default", "Default"], "back on the default (the tester's is on)");
  assert.deepEqual(JSON.parse(JSON.stringify(updates)), [{ subagents: { critic: true } }, { skipHumanTest: false }, { subagents: { tester: null } }]);
  assert.equal(box.querySelector(".subs-msg").textContent, "Saved");
});

test("the Card flow box puts the switch back when the save fails", async () => {
  const { b } = board({ update_card: () => { throw new Error("refused"); } });
  await tick();
  const c = card(8);
  b.show([c]);
  b.ctx.__c = c;
  const host = b.document.createElement("div");
  b.document.body.appendChild(host);
  host.innerHTML = b.run(`subagentsBox(__c)`);
  const box = host.querySelector(".flow-box");
  box.querySelector('.flow-tog[data-flow="reviewer"]').click();
  await tick();
  const sw = box.querySelector('.flow-tog[data-flow="reviewer"]');
  assert.equal(sw.getAttribute("aria-checked"), "true", "still the default, on");
  assert.equal(sw.dataset.own, "default");
  assert.ok(!sw.disabled);
  assert.equal(box.querySelector(".subs-msg").textContent, "Not saved. Try again.");
});

test("Add card sends only what isn't Default", async () => {
  const { b, updates } = board();
  const host = b.document.createElement("div");
  b.document.body.appendChild(host);
  b.ctx.__h = host;
  await b.run(`subagentTicks(__h)`);
  assert.equal(host.querySelector("legend").textContent, "Card flow");
  assert.ok(host.querySelectorAll(".flow-tog[data-flow]").every((s) => s.dataset.own === "default"));
  assert.deepEqual(JSON.parse(JSON.stringify(b.run(`flowChoice(__h)`))), {}, "all Default: nothing sent");
  const sw = (k) => host.querySelector(`.flow-tog[data-flow="${k}"]`);
  sw("skipPlanning").click();
  sw("reviewer").click();
  sw("critic").click();
  sw("tester").click();
  sw("tester").parentNode.querySelector(".flow-reset").click();
  assert.equal(sw("skipPlanning").getAttribute("aria-checked"), "true");
  assert.deepEqual(JSON.parse(JSON.stringify(b.run(`flowChoice(__h)`))), { skipPlanning: true, subagents: { reviewer: false, critic: true } });
  assert.deepEqual(JSON.parse(JSON.stringify(b.run(`subagentChoice(__h)`))), { reviewer: false, critic: true });
  assert.equal(updates.length, 0, "the form saves nothing until Add");
});

test("Settings › Sessions: the two skip defaults sit with the subagent defaults, off unless turned on", async () => {
  const { b, sets } = board();
  b.run(`openSettings("sessions")`);
  await tick();
  const box = b.document.querySelector(".st-subagents");
  assert.equal(box.querySelector("h4").textContent, "Card flow defaults");
  const rules = box.querySelectorAll("[data-rule]").map((x) => x.dataset.rule);
  assert.deepEqual(rules.slice(0, 2), ["sessions.skip_planning", "sessions.skip_human_test"]);
  assert.equal(rules.length, 7);
  const skipP = box.querySelector('[data-rule="sessions.skip_planning"]');
  assert.match(skipP.querySelector("label").textContent, /Skip Planning/);
  assert.ok(!skipP.querySelector("input").hasAttribute("checked"), "off");
  assert.ok(box.querySelector('[data-rule="sessions.skip_human_test"] input').hasAttribute("checked"), "on here");
  const cb = skipP.querySelector("input");
  cb.checked = true;
  await cb.onchange();
  assert.deepEqual(sets, [["sessions.skip_planning", "on"]]);
});
