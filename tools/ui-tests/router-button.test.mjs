// The header's Router button is gone (card #186): node --test tools/ui-tests/router-button.test.mjs
// The panel still opens from the Agents drawer's Router tab (on or off), and starts from its own
// Start button.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const read = (f) => readFileSync(fileURLToPath(new URL(`../../ui/${f}`, import.meta.url)), "utf8");

test("the header has no Router button, and no script looks for one", () => {
  assert.doesNotMatch(read("index.html"), /routerBtn|class="[^"]*\brbtn\b/);
  for (const f of ["router.js", "agents-feed.js", "board.js", "filterbar.js"]) assert.doesNotMatch(read(f), /routerBtn/, f);
  assert.doesNotMatch(read("router.css"), /\.rbtn\b/);
});

test("the Agents drawer's Router tab still opens the Router, running or not", () => {
  const ctx = { console, setTimeout, clearTimeout, Date };
  ctx.window = ctx;
  vm.createContext(ctx);
  vm.runInContext(read("agents-feed.js"), ctx, { filename: "agents-feed.js" });
  const F = ctx.AgentsFeed;
  assert.match(F.runningHtml({ count: 0 }), /data-open="router"[^>]*>.*Router/s);
  assert.match(F.runningHtml({ router: null }), /Open the Router \(not running\)/);
  assert.deepEqual({ ...F.runningSource({}, "router") }, { kind: "router" });
});

test("the Router panel keeps its own Start, and the open hooks", () => {
  const js = read("router.js");
  assert.match(js, /id="r-start"/);
  assert.match(js, /invoke\("router_start"\)/);
  assert.match(js, /window\.openRouterPanel=openRouter/);
  assert.match(js, /registerPill\(\{key:"router-asks"/);
});
