// Card #161: with Hugh's "Size cards before their session starts" off, no card is sized, so the
// panel's Route row says so and offers no Route again.
import test from "node:test";
import assert from "node:assert/strict";
import { boot } from "./harness.mjs";

const flush = () => new Promise((r) => setTimeout(r, 0));
const planning = { id: "c1", number: 1, title: "T", column: "Planning" };

test("sizing off: the Route row says so and has no Route again", async () => {
  const b = boot({ invoke: { get_setting: ({ key }) => ({ key, value: key === "sessions.size_before_start" ? "off" : "on", default: "on" }) } });
  await flush(); await flush();
  assert.equal(b.run("sizeFirstOff"), true);
  b.ctx.__c = planning;
  const row = b.run("routeRow(__c)");
  assert.match(row, /Not sized: sizing is off \(Settings › Sessions\)/);
  assert.doesNotMatch(row, /route-again/);
  // A route made while it was on still shows, still without Route again.
  b.ctx.__c = { ...planning, route: { size: "small", model: "claude-sonnet-5-5", effort: "low" } };
  const routed = b.run("routeRow(__c)");
  assert.match(routed, /Small/);
  assert.doesNotMatch(routed, /route-again|Not sized/);
  // Not in other columns, nor on a stack's child.
  b.ctx.__c = { ...planning, column: "Backlog" };
  assert.doesNotMatch(b.run("routeRow(__c)"), /Not sized/);
  b.ctx.__c = { ...planning, stackOf: 4 };
  assert.doesNotMatch(b.run("routeRow(__c)"), /Not sized/);
});

test("sizing on: Route again as before", async () => {
  const b = boot({ invoke: { get_setting: ({ key }) => ({ key, value: "on", default: "on" }) } });
  await flush(); await flush();
  assert.equal(b.run("sizeFirstOff"), false);
  b.ctx.__c = planning;
  const row = b.run("routeRow(__c)");
  assert.match(row, /id="route-again"/);
  assert.doesNotMatch(row, /Not sized/);
});
