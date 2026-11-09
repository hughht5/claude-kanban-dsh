// Tests for ui/agents-feed.js (card #120): node --test tools/ui-tests/feed.test.mjs
// No npm: the module runs in a vm with a tiny element stub, and its pure helpers are tested on
// window.AgentsFeed.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const SRC = readFileSync(fileURLToPath(new URL("../../ui/agents-feed.js", import.meta.url)), "utf8");

function load(extra = {}) {
  const ctx = { console, setTimeout, clearTimeout, Date, ...extra };
  ctx.window = ctx;
  vm.createContext(ctx);
  vm.runInContext(SRC, ctx, { filename: "agents-feed.js" });
  return ctx;
}
// An element with what mountAgentsFeed uses: classList, innerHTML, click listeners.
function stubEl() {
  const classes = new Set(), listeners = [];
  return {
    nodeType: 1, innerHTML: "", parentElement: null,
    classList: { add: (...c) => c.forEach((x) => classes.add(x)), has: (c) => classes.has(c) },
    addEventListener: (t, f) => listeners.push([t, f]),
    removeEventListener: (t, f) => { const i = listeners.findIndex((l) => l[0] === t && l[1] === f); if (i >= 0) listeners.splice(i, 1); },
    contains: () => false,
    classes, listeners,
  };
}
const rowsOf = (html) => [...html.matchAll(/<li class="fe ag-(\w+)" data-key="([^"]*)"/g)].map((m) => ({ agent: m[1], key: m[2] }));
const now = new Date(2026, 10, 6, 12, 0);
const at = (h, m) => new Date(2026, 10, 6, h, m).toISOString();
const ITEMS = [
  { seq: 9, at: at(1, 55), card: 133, agent: "the merger", kind: "log", verdict: null, text: "passed: merged into stack #132's branch. #135 unblocked." },
  { seq: 8, at: at(1, 52), card: 122, agent: "monitor", kind: "log", verdict: null, text: "#122 still stuck after the nudge. Told Hugh." },
  { seq: 7, at: at(1, 21), card: 121, agent: "card-reviewer", kind: "verdict", verdict: "PASS", text: "small, scoped diff" },
  { seq: 6, at: at(1, 9), card: 0, agent: "the router", kind: "log", verdict: null, text: "#128 small, Sonnet low." },
];

test("rows keep the order given (newest first) and name the agent in words", () => {
  const { AgentsFeed } = load();
  const html = AgentsFeed.rowsHtml(ITEMS, { now });
  const rows = rowsOf(html);
  assert.deepEqual(rows.map((r) => r.agent), ["merger", "monitor", "reviewer", "router"]);
  for (const w of ["Merger", "Monitor", "Reviewer", "Router"]) assert.match(html, new RegExp(`<span class="ag \\w+">${w}</span>`));
  assert.ok(html.indexOf("01:55") < html.indexOf("01:52"));
});

test("#n is a bold mono button that carries the card number; the item's card goes first", () => {
  const { AgentsFeed } = load();
  const h = AgentsFeed.textHtml(ITEMS[0]);
  assert.match(h, /^<button type="button" class="fe-card" data-card="133"[^>]*>#133<\/button> passed/);
  assert.match(h, /data-card="132"[^>]*>#132<\/button>&#39;s branch/);
  assert.match(h, /data-card="135"/);
  // Already named in the text: not added twice.
  assert.equal((AgentsFeed.textHtml(ITEMS[1]).match(/data-card="122"/g) || []).length, 1);
  // Card #0 (the router's own card) is not prefixed.
  assert.doesNotMatch(AgentsFeed.textHtml(ITEMS[3]), /data-card="0"/);
});

test("text is escaped, and &#39; is never read as a card link", () => {
  const { AgentsFeed } = load();
  const h = AgentsFeed.textHtml({ card: 5, text: `<img src=x> it's "#7"` });
  assert.doesNotMatch(h, /<img/);
  assert.match(h, /&lt;img src=x&gt; it&#39;s &quot;<button[^>]*data-card="7"/);
  assert.doesNotMatch(h, /data-card="39"/);
});

test("time is HH:MM today and the date on another day, with the full time in the title", () => {
  const { AgentsFeed } = load();
  assert.equal(AgentsFeed.timeParts(at(9, 5), now).text, "09:05");
  assert.equal(AgentsFeed.timeParts(new Date(2026, 10, 4, 9, 5).toISOString(), now).text, "4 Nov");
  assert.ok(AgentsFeed.timeParts(at(9, 5), now).title.length > 5);
  assert.deepEqual({ ...AgentsFeed.timeParts("", now) }, { text: "", title: "" });
});

test("agent names map to the seven agents", () => {
  const { AgentsFeed } = load();
  assert.equal(AgentsFeed.agentOf("the monitor"), "monitor");
  assert.equal(AgentsFeed.agentOf("card-tester"), "tester");
  assert.equal(AgentsFeed.agentOf("Reproducer"), "reproducer");
  // Hugh's own messages to the Router are his (card #119, final-test F4); a stranger is "other".
  assert.equal(AgentsFeed.agentOf("hugh"), "hugh");
  assert.equal(AgentsFeed.agentWord("hugh"), "Hugh");
  assert.equal(AgentsFeed.agentOf("somebody"), "other");
});

test("the verdict word shows unless the text already says it", () => {
  const { AgentsFeed } = load();
  assert.match(AgentsFeed.rowHtml(ITEMS[2], { now }), /<span class="fe-v ok">PASS<\/span>/);
  assert.doesNotMatch(AgentsFeed.rowHtml({ ...ITEMS[2], text: "PASS. small diff" }, { now }), /fe-v/);
});

test("compact gives the compact list class", () => {
  const { AgentsFeed } = load();
  assert.match(AgentsFeed.rowsHtml(ITEMS, { compact: true }), /<ol class="feed compact"/);
  assert.match(AgentsFeed.rowsHtml(ITEMS, {}), /<ol class="feed"/);
});

test("paging appends older items without duplicates; a refresh puts new ones on top", async () => {
  const ctx = load();
  const pages = {
    first: { items: ITEMS.slice(0, 3), next: "c1" },
    c1: { items: [ITEMS[2], ITEMS[3]], next: null }, // overlaps by one
  };
  const asked = [];
  const source = async (q) => { asked.push({ ...q }); return q.before ? pages[q.before] : pages.first; };
  const el = stubEl();
  const f = ctx.mountAgentsFeed(el, { filter: { cards: [121], limit: 3 }, source, compact: true });
  await f.ready;
  assert.ok(el.classes.has("af") && el.classes.has("af-compact"));
  assert.equal(rowsOf(el.innerHTML).length, 3);
  assert.match(el.innerHTML, /data-older/);
  assert.deepEqual(asked[0], { cards: [121], limit: 3 });
  await f.older();
  assert.deepEqual(asked[1], { cards: [121], limit: 3, before: "c1" });
  assert.deepEqual(rowsOf(el.innerHTML).map((r) => r.agent), ["merger", "monitor", "reviewer", "router"]);
  assert.doesNotMatch(el.innerHTML, /data-older/, "no Show older once next is null");
  // A refresh: one new item on top, the rest kept once, older pages kept.
  pages.first = { items: [{ seq: 10, at: at(2, 0), card: 140, agent: "critic", kind: "log", text: "2 findings" }, ...ITEMS.slice(0, 2)], next: "c0" };
  await f.refresh();
  assert.deepEqual(rowsOf(el.innerHTML).map((r) => r.agent), ["critic", "merger", "monitor", "reviewer", "router"]);
  f.destroy();
});

test("a click on #n calls onCard with the number", async () => {
  const ctx = load();
  const el = stubEl();
  const got = [];
  const f = ctx.mountAgentsFeed(el, { source: async () => ({ items: ITEMS, next: null }), onCard: (n) => got.push(n) });
  await f.ready;
  const click = el.listeners.find((l) => l[0] === "click")[1];
  const btn = { dataset: { card: "132" }, closest: (s) => (s === ".fe-card" ? btn : null) };
  el.contains = (n) => n === btn;
  let prevented = false;
  click({ target: btn, preventDefault: () => { prevented = true; } });
  assert.deepEqual(got, [132]);
  assert.ok(prevented);
  f.destroy();
  assert.equal(el.listeners.length, 0);
});

test("the default source asks the app's agents_feed with the filter as its query", async () => {
  const calls = [];
  const ctx = load({ invoke: async (cmd, args) => { calls.push([cmd, args]); return { items: [], next: null }; } });
  const el = stubEl();
  const f = ctx.mountAgentsFeed(el, { filter: { agents: ["critic"] } });
  await f.ready;
  assert.equal(calls[0][0], "agents_feed");
  assert.deepEqual({ ...calls[0][1].query }, { agents: ["critic"] });
  assert.match(el.innerHTML, /Nothing from the agents yet/);
  f.destroy();
});

test("cards-changed refreshes at most once every 2 s", async () => {
  let t = 0;
  const { AgentsFeed } = load();
  const fired = [];
  const timers = [];
  // A fake clock: the throttle takes its time from `clock`, and its trailing call from setTimeout.
  const ctx2 = load({ setTimeout: (f, w) => { timers.push([f, w]); return timers.length; }, clearTimeout: () => {} });
  const th = ctx2.AgentsFeed.throttle(() => fired.push(t), 2000, () => t);
  th(); t = 500; th(); t = 900; th();
  assert.deepEqual(fired, [0]);
  assert.equal(timers.length, 1, "one trailing call, however many events");
  assert.equal(timers[0][1], 1500);
  t = 2000; timers[0][0]();
  assert.deepEqual(fired, [0, 2000]);
  assert.equal(AgentsFeed.THROTTLE_MS, 2000);
});

test("a verdict row names its check only when it is not its own agent's; card #0 shows as Router, never #0", () => {
  const { AgentsFeed } = load();
  const v = AgentsFeed.rowHtml({ seq: 1, at: at(3, 0), card: 121, agent: "card-tester", kind: "verdict", verdict: "passed", field: "test", text: "sent 2 messages" }, { now });
  // Its own check: the verdict only, the agent name once (final test: no "TESTER TESTER passed").
  assert.match(v, /<span class="fe-v ok">PASSED<\/span>/);
  assert.equal((v.match(/Tester|TESTER/g) || []).length, 1);
  const m = AgentsFeed.rowHtml({ seq: 2, at: at(3, 1), card: 122, agent: "monitor", kind: "verdict", verdict: "repeating", field: "stuck", text: "same test 6 times" }, { now });
  assert.match(m, /REPEATING/);
  assert.doesNotMatch(m, /MONITOR repeating/);
  // Another agent's field written by this one is named: the tester setting repro to needed.
  const o = AgentsFeed.rowHtml({ seq: 4, at: at(3, 3), card: 121, agent: "tester", kind: "verdict", verdict: "needed", field: "repro", text: "" }, { now });
  assert.match(o, /REPRODUCER needed/);
  const r = AgentsFeed.rowHtml({ seq: 3, at: at(3, 2), card: 0, agent: "system", kind: "log", verdict: null, field: null, text: "#0 split #129 into 3 cards" }, { now });
  assert.match(r, /<span class="ag router">Router<\/span>/);
  assert.doesNotMatch(r, /#0|data-card="0"/);
  assert.match(r, /data-card="129"/);
});

// Card #129: what a row's agent opens, and the Running now list.
test("a row's agent opens the Router or the subagent that wrote it", () => {
  const F = load().AgentsFeed;
  assert.deepEqual({ ...F.sessionTarget({ card: 0, agent: "the router", kind: "log", text: "x" }) }, { kind: "router" });
  assert.deepEqual({ ...F.sessionTarget({ card: 12, agent: "the router", kind: "log", text: "moved" }) }, { kind: "router" }, "the Router's move on a card is the Router's");
  const route = F.sessionTarget({ card: 12, agent: "router", kind: "verdict", field: "route", agent_id: "abc", at: "2026-11-06T10:00:00Z" });
  assert.equal(route.kind, "sub"); assert.equal(route.type, "card-router"); assert.equal(route.agentId, "abc"); assert.equal(route.card, 12);
  const critic = F.sessionTarget({ card: 129, agent: "critic", kind: "verdict", field: "critique", agent_id: "a2028e5c88100b872", at: "t" });
  assert.equal(critic.type, "card-critic"); assert.equal(critic.agentId, "a2028e5c88100b872");
  const log = F.sessionTarget({ card: 5, agent: "card-tester", kind: "log", agent_id: "ignored", at: "t2" });
  assert.equal(log.type, "card-tester"); assert.equal(log.agentId, null, "a log row is matched by type and time"); assert.equal(log.before, "t2");
  assert.equal(F.sessionTarget({ card: 5, agent: "the merge lane", kind: "log" }), null, "the merge lane is opened from Running now");
  assert.equal(F.sessionTarget({ card: 5, agent: "monitor", kind: "log" }), null);
  assert.equal(F.sessionTarget({ card: 0, agent: "card-critic", kind: "verdict" }), null, "no card, no subagent");
});

test("only a feed with onSession draws the agent as a button", () => {
  const F = load().AgentsFeed;
  const item = { seq: 1, at: at(1, 0), card: 3, agent: "card-critic", kind: "log", text: "x" };
  assert.match(F.rowHtml(item, { now, open: true }), /<button type="button" class="ag critic ag-open" data-open/);
  assert.match(F.rowHtml(item, { now }), /<span class="ag critic">/);
  assert.match(F.rowHtml({ ...item, agent: "monitor" }, { now, open: true }), /<span class="ag monitor">/, "nothing to open: plain");
});

const RUN = {
  router: { state: "idle" },
  lanes: [{ key: "lane:owner/repo", repo: "owner/repo", state: "working" }],
  lanesKnown: [{ key: "lane:owner/old", repo: "owner/old", state: "off" }, { key: "lane:owner/repo", repo: "owner/repo", state: "off" }],
  cards: [{ cardId: "card-12", number: 12, title: "Fix <it>", column: "In progress", state: "permission", session: "s-12",
    subagents: [{ id: "a1", agentType: "card-tester", description: "Test card 12" }] }],
  outside: [{ id: "o-1", account: "main", cwd: "C:/work/repo", title: null, lastAt: new Date(now - 3 * 60000).toISOString(), cards: [7, 9], readable: true }],
  count: 5,
};

test("the agents sit along the top, then what runs now (Hugh, 9 Nov)", () => {
  const F = load().AgentsFeed;
  const h = F.runningHtml(RUN, { now });
  const opens = [...h.matchAll(/data-open="([^"]+)"/g)].map((m) => m[1]);
  assert.deepEqual(opens, ["router", "lane:0", "lane:1", "agent:critic", "agent:reviewer", "agent:tester", "agent:reproducer", "monitor", "card:0", "sub:0:0", "out:0"]);
  assert.ok(h.indexOf('aria-label="The agents"') < h.indexOf("Running now"), "the agents first");
  assert.match(h, /<span class="rn-n">5<\/span>/);
  assert.match(h, /Fix &lt;it&gt;/, "titles are escaped");
  assert.match(h, /data-open="card:0"[^>]*><span class="rdot r-asks"/, "a session at a question shows the asks dot");
  assert.match(h, /data-open="agent:tester"[^>]*><span class="rdot r-working"/, "a kind with one running shows it");
  assert.match(h, /data-open="agent:critic"[^>]*>Critic</, "a kind with none running: just its name");
  assert.match(h, /Outside <span class="rn-tt">repo<\/span>/, "no title: the folder's name");
  assert.match(h, /#7 #9 · 3 min ago/);
  const quiet = F.runningHtml({ count: 0 });
  assert.match(quiet, /No card session is running\./);
  assert.match(quiet, /data-open="router"[^>]*><span class="rdot r-off"/, "the Router tab is there when it is off");
  assert.match(quiet, /data-open="lane:none"/, "no lane yet: a Merger tab that says so");
  assert.match(F.runningHtml({}, { monitor: true }), /class="rn-tab ag-monitor on"[^>]*aria-pressed="true"/);
});

test("a tab turns back into the session it opens", () => {
  const F = load().AgentsFeed;
  assert.deepEqual({ ...F.runningSource(RUN, "router") }, { kind: "router" });
  assert.deepEqual({ ...F.runningSource({}, "router") }, { kind: "router" }, "the Router opens when it is off too");
  assert.deepEqual({ ...F.runningSource(RUN, "lane:0") }, { kind: "lane", key: "lane:owner/repo", repo: "owner/repo", state: "working" }, "a running lane keeps its state");
  assert.deepEqual({ ...F.runningSource(RUN, "lane:1") }, { kind: "lane", key: "lane:owner/old", repo: "owner/old", state: "off" });
  assert.equal(F.lanesOf(RUN).length, 2, "a lane once");
  assert.deepEqual({ ...F.runningSource(RUN, "card:0") }, { kind: "card", cardId: "card-12", num: 12, title: "Fix <it>" });
  const sub = F.runningSource(RUN, "sub:0:0");
  assert.equal(sub.kind, "sub"); assert.equal(sub.session, "s-12"); assert.equal(sub.id, "a1"); assert.equal(sub.cardId, "card-12");
  const out = F.runningSource(RUN, "out:0");
  assert.equal(out.kind, "outside"); assert.equal(out.id, "o-1"); assert.deepEqual([...out.cards], [7, 9]);
  assert.equal(F.runningSource(RUN, "card:4"), null, "gone");
  assert.equal(F.runningSource(RUN, "agent:critic"), null, "an agent tab is the drawer's to open");
  assert.equal(F.runningKind(RUN, "tester").id, "a1", "the running tester");
  assert.equal(F.runningKind(RUN, "critic"), null, "none running: the drawer opens the latest from the feed");
});
