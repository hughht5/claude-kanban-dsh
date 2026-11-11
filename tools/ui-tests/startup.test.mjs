// The Agents drawer and the Router panel open by themselves at start (card #189):
// node --test tools/ui-tests/startup.test.mjs
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const read = (f) => readFileSync(fileURLToPath(new URL(`../../ui/${f}`, import.meta.url)), "utf8");

// startup.js alone, with stub panes: what it opens, and how.
function boot({ agentsOpen = false, paneShows = null, readyState = "complete", noRouter = false } = {}) {
  const log = [], ls = {};
  const ctx = { console };
  ctx.window = ctx;
  ctx.document = { readyState, addEventListener: (t, f) => { (ls[t] ||= []).push(f); } };
  ctx.agentsPane = { isOpen: () => agentsOpen, open: (o) => { log.push(["agents", o]); agentsOpen = true; } };
  if (!noRouter) ctx.openRouterPanel = () => log.push(["router"]);
  ctx.sessionPaneShows = () => paneShows;
  vm.createContext(ctx);
  vm.runInContext(read("startup.js"), ctx, { filename: "startup.js" });
  return { log, fire: (t) => (ls[t] || []).forEach((f) => f()) };
}

test("both open at start, the drawer without taking the focus", () => {
  const b = boot();
  assert.equal(b.log.length, 2);
  assert.equal(b.log[0][0], "agents");
  assert.equal(b.log[0][1].focus, false);
  assert.deepEqual(b.log[1], ["router"]);
});

test("a page still loading opens them once it has", () => {
  const b = boot({ readyState: "loading" });
  assert.equal(b.log.length, 0);
  b.fire("DOMContentLoaded");
  assert.deepEqual(b.log.map((x) => x[0]), ["agents", "router"]);
});

test("nothing already open is opened again or swapped out", () => {
  const b = boot({ agentsOpen: true, paneShows: { kind: "card", cardId: "card-1" } });
  assert.equal(b.log.length, 0);
});

test("a missing Router doesn't stop the drawer", () => {
  const b = boot({ noRouter: true });
  assert.deepEqual(b.log.map((x) => x[0]), ["agents"]);
});

test("index.html loads startup.js after agents-feed.js and router.js", () => {
  const html = read("index.html");
  const at = (f) => html.indexOf(`<script src="${f}">`);
  assert.ok(at("startup.js") > at("router.js") && at("router.js") > at("agents-feed.js") && at("agents-feed.js") > 0);
});

test("each panel keeps its close button, and the drawer's open takes a no-focus option", () => {
  const feed = read("agents-feed.js"), router = read("router.js");
  assert.match(feed, /id="ap-x"/);
  assert.match(feed, /#ap-x"\)\.onclick=\(\)=>closePane\(\)/);
  assert.match(feed, /opts&&opts\.focus===false/);
  assert.match(router, /#r-close"\)\.onclick=\(\)=>closePane\(\)/);
});
