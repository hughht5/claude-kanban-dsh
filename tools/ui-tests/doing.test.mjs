// What each card is doing (card #150): the one status chip, its order, labels and tooltips, on
// the real ui/*.js files in a vm.
import test from "node:test";
import assert from "node:assert/strict";
import { boot, rowKinds } from "./harness.mjs";

const NOW = Date.now(); // the tiles read the real clock
const iso = (minsAgo) => new Date(NOW - minsAgo * 60000).toISOString();
const card = (o) => ({ id: "c" + o.number, title: "Card " + o.number, column: "In progress", repo: "hughht5/claude-kanban-dsh", updatedAt: iso(1), ...o });
const idle = (m) => ({ state: "idle", since: iso(m), turnSince: null, running: [], testing: false });
const working = (m) => ({ state: "working", since: iso(1), turnSince: iso(m), running: [], testing: false });

// doingOf for one card and context, from the page's own function.
function of(b, c, ctx) {
  b.ctx.__c = c; b.ctx.__x = { now: NOW, ...ctx };
  return b.run("Doing.doingOf(__c,__x)");
}

test("each status, and the first that holds wins", () => {
  const b = boot();
  const c = card({ number: 150 });
  const tester = { agent: "card-tester", description: "Test card #150", since: iso(4), background: true };
  const tapp = { holder: { card: 150, givenAt: iso(6), expiresAt: new Date(NOW + 24 * 60000).toISOString() }, line: [] };
  // Lowest first: no session, idle, shipped, waits, starting, working, the test app line, its turn, a subagent, install, stuck, needs.
  assert.equal(of(b, c, {}).label, "No session");
  assert.equal(of(b, card({ number: 150, column: "Planning" }), {}), null, "only an In progress card says it has none");
  assert.equal(of(b, c, { queued: true }), null, "a start waiting under the cap has its own badge");
  assert.equal(of(b, c, { d: idle(25) }).label, "Idle 25m");
  assert.equal(of(b, card({ number: 150, column: "Deploy complete", ship: { state: "shipped" } }), { d: idle(3) }).label, "Shipped, session open");
  const un = [{ n: 12, tip: "#12 Store: Planning (ready from Human test)" }];
  assert.equal(of(b, c, { d: idle(3), unmet: un }).label, "Waiting on #12");
  assert.equal(of(b, c, { unmet: un }).label, "Waiting on #12", "In progress with no session");
  assert.equal(of(b, card({ number: 150, column: "Backlog" }), { unmet: un }), null, "Backlog keeps waits.js's own chip");
  assert.equal(of(b, c, { unmet: [{ n: 1 }, { n: 2 }, { n: 3 }], d: idle(1) }).label, "Waiting on #1 +2");
  assert.equal(of(b, c, { d: { state: "starting", since: iso(0) }, unmet: un }).label, "Starting");
  assert.equal(of(b, c, { d: working(72), unmet: un }).label, "Working 1h 12m");
  // Its test app (testapp.js's KBTestApp.tileChip, card #147, passed in as tchip): its place in a line, its turn, or just its name.
  const line2 = { t: "Test app: 2nd in line", c: "tap-wait", title: "Every test app is taken: this card waits for one to come free, 2nd in line." };
  assert.equal(of(b, c, { d: working(12), tchip: line2 }).label, "Test app: 2nd in line");
  assert.equal(of(b, c, { d: idle(1), tchip: { t: "Test 3 · 1st in line", c: "tap-paused", title: "x" } }).label, "Test 3 · 1st in line");
  const hold = { t: "Test 2 · testing", c: "tap-hold", title: "This card's build goes in Claude Kanban DSH Test 2 (CDP port 9335). Its session has the turn now." };
  assert.equal(of(b, c, { d: working(12), tapp, tchip: hold }).label, "Test 2 · testing");
  const own = { t: "Test 2", c: "tap-own", title: "This card's build goes in Claude Kanban DSH Test 2 (CDP port 9335)." };
  assert.equal(of(b, c, { d: idle(7), tchip: own }).label, "Idle 7m · Test 2", "its own test app, named after what it does");
  assert.equal(of(b, card({ number: 150, column: "Human test" }), { tchip: own }).label, "Test 2", "no session: just where to test it");
  assert.equal(of(b, c, { d: { ...idle(1), running: [tester] }, tapp, tchip: hold }).label, "Tester running", "after its turn, while it runs in the background");
  assert.equal(of(b, c, { d: { ...working(5), testing: true }, tapp, tchip: hold }).label, "Reviewing");
  assert.equal(of(b, c, { d: { ...working(5), running: [tester] }, tapp: { ...tapp, install: { card: 150, phase: "installing" } } }).label, "Installing");
  assert.equal(of(b, c, { d: working(5), tapp: { install: { card: 99, phase: "installing" } } }).label, "Working 5m", "another card's install isn't this card's");
  assert.equal(of(b, c, { d: working(5), tapp: { slots: [{ slot: 2, install: { card: "150", phase: "installing" } }] } }).label, "Installing", "an install on one of the slots (card #147)");
  const stuck = { reason: "same cargo test 6 times", kind: "repeating", since: iso(22) };
  // A stuck card has face.js's "May be stuck" box: its chip says what the session does under it, the Monitor's reason first in the tooltip.
  const sx = of(b, c, { d: { ...working(30), running: [tester] }, stuck });
  assert.equal(sx.label, "Tester running");
  assert.equal(sx.stuck, true);
  assert.match(sx.tip, /^The Monitor thinks it may be stuck \(Repeating\): same cargo test 6 times, for 22m\.\nWaiting for its subagent:/);
  assert.equal(of(b, card({ number: 150, column: "Done" }), { d: idle(1), stuck }), null, "nothing on a Done card");
  assert.equal(of(b, c, { loaded: false }), null, "no \"No session\" before session_doing has answered");
  const need = { reason: "permission", at: iso(2), ask: { kind: "question", text: "Keep the older email?" } };
  assert.equal(of(b, c, { d: { ...working(30), state: "permission" }, stuck, need }).label, "Needs you");
  assert.equal(of(b, c, { need: { reason: "failed", detail: "crashed", at: iso(1) } }).label, "Crashed");
  assert.equal(of(b, c, { need: { reason: "unread", at: iso(1) }, d: idle(4) }).label, "Idle 4m", "an unread reply is only a dot");
  assert.equal(of(b, card({ number: 150, archived: true }), { d: working(1) }), null);
});

test("the subagents: the app's by name, any other as a subagent, each in the tooltip", () => {
  const b = boot();
  const c = card({ number: 150 });
  const rev = { agent: "card-reviewer", description: "Review card #150's diff", since: iso(4), background: false };
  const exp = { agent: "Explore", description: "Find the hook code", since: iso(9), background: true };
  const crit = { agent: "card-critic", description: "Critique the plan", since: iso(2), background: true };
  let x = of(b, c, { d: { ...working(10), running: [exp, rev] }, step: "Step 4 of 7: Writing the tests" });
  assert.equal(x.label, "Reviewer running", "the app's own agent before an older Explore");
  assert.equal(x.k, "agent");
  assert.match(x.tip, /Waiting for 2 subagents:/);
  assert.match(x.tip, /- Explore: Find the hook code · 9m, in the background/);
  assert.match(x.tip, /- Reviewer \(card-reviewer\): Review card #150's diff · 4m/);
  assert.match(x.tip, /Its session is on a turn, since \d\d:\d\d \(10m\)\./);
  assert.match(x.tip, /Its to-do list: Step 4 of 7: Writing the tests\./);
  assert.equal(of(b, c, { d: { ...idle(1), running: [rev, crit] } }).label, "Reviewer running +1");
  assert.equal(of(b, c, { d: { ...idle(1), running: [exp] } }).label, "Subagent running");
  assert.equal(of(b, c, { d: { ...idle(1), running: [exp, { ...exp, since: iso(1) }] } }).label, "2 subagents running");
  // Secrets in a description are hidden, as needs.js hides them.
  x = of(b, c, { d: { ...idle(1), running: [{ agent: "Explore", description: "Use token=ghp_abcdefghijklmnop1234", since: iso(1) }] } });
  assert.doesNotMatch(x.tip, /ghp_abcdefghijklmnop1234/);
});

test("the tooltips say why", () => {
  const b = boot();
  const c = card({ number: 150 });
  const need = { reason: "permission", at: iso(2), ask: { kind: "question", text: "Keep the older email?" } };
  assert.match(of(b, c, { need }).tip, /^Needs you \(2m\)\. It asks you a question: Keep the older email\?/);
  assert.match(of(b, c, { need: { reason: "permission", at: iso(1), ask: { kind: "permission", text: "Bash: cargo test" } } }).tip, /permission prompt: Bash: cargo test/);
  assert.match(of(b, c, { d: idle(30), stuck: { reason: "quiet for 30 minutes", kind: "quiet", since: iso(30) } }).tip, /^The Monitor thinks it may be stuck \(Quiet\): quiet for 30 minutes, for 30m\.\nIts session is idle/);
  const x = of(b, c, { d: idle(1), tchip: { t: "Test 2 · 1st in line", c: "tap-wait", title: "This card's build goes in Claude Kanban DSH Test 2 (CDP port 9335). Its session is 1st in line for a turn." } });
  assert.match(x.tip, /^This card's build goes in Claude Kanban DSH Test 2 \(CDP port 9335\)\. Its session is 1st in line for a turn\.\nIts session is idle/);
  assert.match(of(b, c, { d: idle(25) }).tip, /Its session is idle, waiting for a message, since \d\d:\d\d \(25m\)\./);
  assert.match(of(b, c, { d: working(1), tapp: { install: { card: 150, phase: "waiting", waitingFor: [{ card: 99 }] } } }).tip, /Its test app install waits for #99\./);
  assert.match(of(b, c, {}).tip, /No Claude session is running for this card/);
});

test("on the tile: one chip first in the row, with its tooltip, and no Building chip", () => {
  const b = boot();
  b.ctx.__d = { c150: working(12), c151: { ...idle(3), running: [{ agent: "card-tester", description: "Test it", since: iso(2), background: true }] } };
  b.run("doing=__d");
  const cards = [card({ number: 150, labels: ["DSH"] }), card({ number: 151 }), card({ number: 152, blockedBy: [150] }), card({ number: 153, column: "Backlog", blockedBy: [150] })];
  b.show(cards);
  const chip = (id) => b.card(id).querySelector(".chip.doing");
  assert.equal(chip("c150").textContent.startsWith("Working"), true);
  assert.equal(rowKinds(b, b.card("c150"))[0], "doing", "first, before the labels");
  assert.match(chip("c150").title, /Its session is on a turn/);
  assert.equal(chip("c151").textContent, "Tester running");
  assert.equal(chip("c151").dataset.doing, "agent");
  assert.equal(b.card("c150").querySelector(".chip.building"), null);
  // "Waiting on #150" on an In progress card takes the place of waits.js's chip; a Backlog card keeps that one.
  assert.equal(chip("c152").textContent, "Waiting on #150");
  assert.equal(b.card("c152").querySelector(".wt-chip"), null);
  assert.equal(chip("c153"), null);
  assert.ok(b.card("c153").querySelector(".wt-chip"));
});

test("the times move on in place", () => {
  const b = boot();
  b.ctx.__d = { c150: idle(5) };
  b.run("doing=__d");
  b.show([card({ number: 150 })]);
  const el = b.card("c150").querySelector(".chip.doing");
  assert.equal(el.textContent, "Idle 5m");
  // Ten minutes later, by the session's clock: the same node, new words.
  b.ctx.__d2 = { c150: idle(15) };
  b.run("doing=__d2;Doing.doingTick()");
  assert.equal(b.card("c150").querySelector(".chip.doing"), el);
  assert.equal(el.textContent, "Idle 15m");
});

test("before session_doing answers, panel.js's state stands in; a plan to start keeps its chip", () => {
  const b = boot();
  // Nothing from session_doing yet (the harness answers null): the session's state from `activity` only.
  b.run("var activity={c150:'working',c151:'idle'};doingLoaded=false;doing={}");
  b.show([card({ number: 150 }), card({ number: 151, column: "Planning", plan: { state: "requested", detail: "x" } }), card({ number: 152 })]);
  assert.equal(b.card("c150").querySelector(".chip.doing").textContent, "Working");
  assert.equal(b.card("c152").querySelector(".chip.doing"), null, "no false \"No session\" before the first answer");
  const p = b.card("c151");
  assert.equal(p.querySelector(".chip.doing").textContent, "Idle");
  assert.ok(p.querySelector(".chip.plan-requested"), "Plan requested still shows beside it");
});

test("a new answer changes the chips in place, and redraws only when one comes or goes", () => {
  const b = boot();
  b.run("doingLoaded=true;doing={c150:__i}".replace("__i", JSON.stringify(idle(5))));
  b.show([card({ number: 150 }), card({ number: 151, column: "Human test" })]);
  const el = b.card("c150").querySelector(".chip.doing");
  assert.equal(el.dataset.doing, "idle");
  assert.equal(b.card("c151").querySelector(".chip.doing"), null);
  // The session starts a turn: same node, new kind, words and colour.
  b.ctx.__d = { c150: working(1) };
  b.run("doing=__d;Doing.doingPaint()");
  assert.equal(b.card("c150").querySelector(".chip.doing"), el);
  assert.equal(el.dataset.doing, "working");
  assert.equal(el.className, "tag chip doing d-working");
  assert.equal(el.textContent, "Working 1m");
  // A session on the Human test card: a chip that wasn't there means a redraw.
  b.ctx.__d = { c150: working(1), c151: idle(2) };
  b.run("doing=__d;Doing.doingPaint()");
  assert.equal(b.card("c151").querySelector(".chip.doing").textContent, "Idle 2m");
});
