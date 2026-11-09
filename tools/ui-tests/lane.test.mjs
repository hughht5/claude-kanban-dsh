// The merge lanes (card #160): the Merger chips for a lane job (waiting, merging, cancelled) and
// Settings › Sessions' merge-lane list with each lane's state and Stop, on the real ui/*.js files
// in a vm (harness.mjs). The own-worktree box shares the list editor, so it is checked here too.
import test from "node:test";
import assert from "node:assert/strict";
import { boot, FILES } from "./harness.mjs";

const tick = async (n = 20) => { for (let i = 0; i < n; i++) await new Promise((r) => setImmediate(r)); };
const at = new Date().toISOString();

test("the Merger chip says Waiting for the lane, Merging and Merge cancelled, titled with the lane's repo", () => {
  const b = boot();
  const card = (n, merge, column = "Ship it") => ({ id: "card-" + n, number: n, title: "Card " + n, column, repo: "acme/web", merge, updatedAt: at });
  b.show([
    card(1, { state: "queued", lane: "lane:acme/web", at }),
    card(2, { state: "running", lane: "lane:acme/web", at, startedAt: at }),
    card(3, { state: "cancelled", lane: "lane:acme/web", at, detail: "the card left Ship it" }, "Deploy complete"),
    card(4, { state: "passed", at, detail: "rebased card-4" }),
    card(5, { state: "running", lane: "lane:acme/web", at }, "In progress"),
  ]);
  const chip = (n) => b.card("card-" + n).querySelector('[data-check="merge"]');
  assert.equal(chip(1).textContent, "Waiting for the lane");
  assert.ok(chip(1).classList.contains("test-queued"));
  assert.equal(chip(1).title, "In the merge lane for acme/web");
  assert.equal(chip(2).textContent, "Merging");
  assert.ok(chip(2).classList.contains("test-running"));
  assert.equal(chip(3).textContent, "Merge cancelled");
  assert.equal(chip(3).title, "In the merge lane for acme/web: the card left Ship it");
  // The card-merger's verdict, with no lane, is as it was.
  assert.equal(chip(4).textContent, "Merged");
  assert.equal(chip(4).title, "rebased card-4");
  // Not outside Ship it and Deploy complete.
  assert.equal(chip(5), null);
});

const lanes = [
  { repo: "acme/web", checkout: "C:/co/web", worktree: "C:/co/web/.claude/worktrees/merge-lane", state: "merging", card: 42, model: "claude-opus-5-5", effort: "high", live: true },
  { repo: "acme/api", checkout: "C:/co/api", worktree: "C:/co/api/.claude/worktrees/merge-lane", state: "off", card: null, model: "claude-opus-5-5", effort: "high", live: false },
];

function settings(extra = {}) {
  const sets = [], stops = [];
  const values = {
    "sessions.merge_lane_repos": JSON.stringify(lanes.map(({ repo, checkout }) => ({ repo, checkout }))),
    "sessions.own_worktree_repos": JSON.stringify([{ repo: "acme/web", checkout: "C:/co/web" }]),
    "lanes.model": "claude-sonnet-5-5", "lanes.effort": "medium",
    "sessions.small_goes_on_repos": "[]",
  };
  const b = boot({
    files: [...FILES, "settings.js"],
    invoke: {
      get_setting: ({ key }) => ({ key, value: values[key] ?? "3", default: key === "lanes.model" ? "claude-opus-5-5" : key === "lanes.effort" ? "high" : "3" }),
      set_setting: ({ key, value }) => { sets.push([key, value]); return value; },
      get_setting_log: ({ key }) => key === "sessions.merge_lane_repos" ? [{ at, by: "Hugh (settings)", value: values[key] }] : [],
      lane_status: () => lanes,
      lane_stop: ({ repo }) => { stops.push(repo); return lanes.map((l) => l.repo === repo ? { ...l, state: "idle", live: false, card: null } : l); },
      ...extra,
    },
  });
  b.ctx.confirm = () => true;
  return { b, sets, stops };
}

test("settings, Sessions: the merge-lane list, the lanes' model and effort, each lane's state and Stop", async () => {
  const { b, sets, stops } = settings();
  b.run(`openSettings("sessions")`);
  await tick();
  const box = b.document.querySelector(".st-lanes");
  assert.ok(box, "the box is there");
  // Straight after the own-worktree box.
  const boxes = b.document.querySelectorAll(".st-box");
  assert.equal(boxes[boxes.indexOf(box) - 1], b.document.querySelector(".st-ownwt"));
  assert.equal(box.querySelector("b").textContent, "Ship through one merge lane per repo");
  assert.match(box.querySelector(".meta").textContent, /ships through one merge lane.*never pushes.*Repos not listed keep the card-merger subagent/);
  assert.deepEqual(box.querySelectorAll(".st-lr").map((x) => x.value), ["acme/web", "acme/api"]);
  assert.match(box.querySelector(".log").textContent, /Hugh \(settings\): acme\/web, acme\/api/);
  // The model and effort, as set.
  assert.equal(box.querySelector('#st-lmodel option[selected]').getAttribute("value"), "claude-sonnet-5-5");
  assert.equal(box.querySelector('#st-leffort option[selected]').getAttribute("value"), "medium");
  // Each lane: its state, model and effort, and Stop only while it runs.
  const rows = box.querySelectorAll(".st-lane");
  assert.deepEqual(rows.map((r) => r.dataset.repo), ["acme/web", "acme/api"]);
  assert.equal(rows[0].querySelector("[data-lane]").textContent, "Merging #42");
  assert.ok(rows[0].querySelector(".chip").classList.contains("lane-merging"));
  assert.equal(rows[0].querySelector(".meta").textContent, "Opus 5.5 · effort high");
  assert.equal(rows[1].querySelector("[data-lane]").textContent, "Off");
  assert.ok(rows[1].querySelector(".st-lstop").disabled);
  assert.ok(!rows[0].querySelector(".st-lstop").disabled);
  rows[0].querySelector(".st-lstop").click();
  await tick();
  assert.deepEqual(stops, ["acme/web"]);
  assert.equal(box.querySelector('.st-lane[data-repo="acme/web"] [data-lane]').textContent, "Idle");
  // A model change is saved at once.
  const sel = box.querySelector("#st-lmodel");
  sel.value = "claude-opus-5-5";
  sel.onchange();
  await tick();
  assert.deepEqual(sets, [["lanes.model", "claude-opus-5-5"]]);
  // Saving the list saves its rows, and the lanes are drawn again.
  box.querySelector(".st-lx").click();
  await tick();
  box.querySelector("#st-lsave").click();
  await tick();
  assert.deepEqual(sets[1], ["sessions.merge_lane_repos", JSON.stringify([{ repo: "acme/api", checkout: "C:/co/api" }])]);
  assert.equal(box.querySelector("#st-lmsg").textContent, "Saved");
  assert.ok(box.querySelector(".st-lanelist"));
});

test("settings, Sessions: the own-worktree box works as before through the shared list editor", async () => {
  const { b, sets } = settings();
  b.run(`openSettings("sessions")`);
  await tick();
  const box = b.document.querySelector(".st-ownwt");
  assert.equal(box.querySelector("b").textContent, "Build each card in its own worktree");
  assert.deepEqual(box.querySelectorAll(".st-wrow").map((r) => r.querySelector(".st-wr").value), ["acme/web"]);
  assert.equal(box.querySelector(".st-lanebox"), null, "no lanes in it");
  box.querySelector("#st-wadd").click();
  await tick();
  assert.equal(box.querySelectorAll(".st-wrow").length, 2);
  box.querySelector("#st-wsave").click();
  await tick();
  assert.deepEqual(sets, [["sessions.own_worktree_repos", JSON.stringify([{ repo: "acme/web", checkout: "C:/co/web" }, { repo: "", checkout: "" }])]]);
  assert.equal(box.querySelector("#st-wmsg").textContent, "Saved");
});

test("settings, Sessions: a backend with no lanes says it couldn't read them", async () => {
  const { b } = settings({ lane_status: () => { throw new Error("unknown command lane_status"); } });
  b.run(`openSettings("sessions")`);
  await tick();
  assert.match(b.document.querySelector(".st-lanelist").textContent, /^Couldn't read the lanes: /);
});
