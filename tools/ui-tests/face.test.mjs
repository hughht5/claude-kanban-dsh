// The card face, the pill row and the board's counts (card #120), on the real ui/*.js files in a vm.
import test from "node:test";
import assert from "node:assert/strict";
import { boot, rowKinds, FILES } from "./harness.mjs";

const NOW = Date.now();
const iso = (minsAgo) => new Date(NOW - minsAgo * 60000).toISOString();

// One card for each state the mockup shows, plus the router's card #0.
function fixture() {
  return [
    { id: "c130", number: 130, title: "Weekly usage report", column: "Backlog", labels: ["frontend"], blockedBy: [128], repo: "hughht5/kanban-app", updatedAt: iso(5) },
    { id: "c128", number: 128, title: "Search finds labels", column: "Planning", labels: ["frontend"], repo: "hughht5/kanban-app", updatedAt: iso(9),
      route: { size: "small", model: "claude-sonnet-5-5", effort: "low", reason: "one file in ui/, no data change", at: iso(8) },
      critique: { state: "passed", detail: "No findings", at: iso(2) }, autoTo: "Human test",
      issue: { number: 7, url: "https://github.com/hughht5/kanban-app/issues/7" }, pr: "https://github.com/hughht5/kanban-app/pull/9", account: "account2" },
    { id: "c129", number: 129, title: "Booking calendar", column: "Planning", route: { size: "split", children: ["a", "b", "c"], at: iso(20) },
      critique: { state: "failed", detail: "2 findings: no plan for time zones", at: iso(3) }, updatedAt: iso(3) },
    { id: "c120", number: 120, title: "Pin a card", column: "In progress", doneWhen: "a pinned card stays first", columnSince: iso(72), updatedAt: iso(1),
      summary: "The summary" },
    { id: "c122", number: 122, title: "Remember the panel width", column: "In progress", updatedAt: iso(1),
      stuck: { reason: "same cargo test 6 times", kind: "repeating", since: iso(22), nudged: true } },
    { id: "c124", number: 124, title: "Merge duplicate users", column: "In progress", updatedAt: iso(1) },
    { id: "c125", number: 125, title: "Answered question", column: "In progress", updatedAt: iso(1) },
    { id: "c121", number: 121, title: "Message times", column: "Human test", updatedAt: iso(1), checkSize: { class: "quick", why: "small" },
      test: { state: "passed", detail: "all good", at: iso(4) }, review: { state: "passed", detail: "fine", at: iso(3) },
      evidence: [{ run: "r1", at: iso(4), verdict: "passed", shots: [{ n: 2, kind: "after", path: "C:/shots/after.png" }, { n: 1, kind: "before", path: "C:/shots/before.png" }] }],
      worktreePath: "C:/wt", changesSaved: { totals: { files: 2, added: 31, removed: 9 } } },
    { id: "c126", number: 126, title: "Both stuck and waiting", column: "In progress", updatedAt: iso(1),
      stuck: { reason: "quiet for 30 minutes", kind: "quiet", since: iso(30) } },
    { id: "router", number: 0, title: "Router", column: "In progress", system: "router", updatedAt: iso(1) },
  ];
}
const NEEDS = {
  c124: { reason: "permission", at: iso(1), ask: { kind: "question", text: "Keep the older or newer email?", options: ["Keep older", "Keep newer", "Keep both"] } },
  c125: { reason: "unread", at: iso(1), ask: { kind: "reply", text: "Done." } },
  c126: { reason: "permission", at: iso(1) },
  router: { reason: "permission", at: iso(1) },
};

test("the pure helpers", () => {
  const { ctx } = boot();
  const F = ctx.Face;
  assert.equal(F.fmtDur(30 * 1000), "<1m");
  assert.equal(F.fmtDur(22 * 60000), "22m");
  assert.equal(F.fmtDur(72 * 60000), "1h 12m");
  assert.equal(F.fmtDur(185 * 60000), "3h 05m");
  assert.equal(F.fmtDur(50 * 3600000), "2d 2h");
  assert.equal(F.routeText({ size: "small", model: "claude-sonnet-5-5", effort: "low" }), "Router: small · Sonnet low");
  assert.equal(F.routeText({ size: "split", children: ["a", "b", "c"] }), "Router: split into 3");
  assert.equal(F.critiqueText({ detail: "2 findings: x" }, "Critique failed"), "Critic: 2 findings");
  assert.equal(F.critiqueText({ detail: "nothing" }, "Plan critiqued"), "Plan critiqued");
  assert.equal(F.timeIn({ column: "In progress", columnSince: iso(72) }, NOW), "In progress 1h 12m");
  assert.equal(F.timeIn({ column: "In progress", columnSince: null }, NOW), "");
  // The newest agent text wins, among the agents whose chip is on the card.
  const c = { route: { reason: "old", at: iso(10) }, critique: { detail: "new", at: iso(1) } };
  assert.deepEqual([F.agentLine(c, new Set(["route", "critique"])).agent, F.agentLine(c, new Set(["route", "critique"])).text], ["Critic", "new"]);
  assert.equal(F.agentLine(c, new Set(["route"])).text, "old");
  assert.equal(F.agentLine(c, new Set()), null);
  // A stable order: labels, waits, route, critique, test, review, repro, merge, check size, then the rest as drawn.
  const kinds = ["other1", "merge", "label", "checksize", "other2", "test", "wait", "route", "review", "critique", "repro"];
  const kindOf = (k) => (k.startsWith("other") ? "other" : k);
  assert.deepEqual(F.orderChips(kinds, kindOf), ["label", "wait", "route", "critique", "test", "review", "repro", "merge", "checksize", "other1", "other2"]);
});

test("labels come first in the one chip row, in the mockup's order", () => {
  const b = boot();
  b.show(fixture(), { needs: NEEDS });
  assert.deepEqual(rowKinds(b, b.card("c130")), ["label", "wait"]);
  const k128 = rowKinds(b, b.card("c128"));
  assert.deepEqual(k128, ["label", "route", "critique", "other"]);
  const row = b.card("c128").querySelector(":scope > .tags");
  assert.equal(row.querySelector("[data-route]").textContent, "Router: small · Sonnet low");
  assert.match(row.querySelector("[data-route]").title, /card-router/); // the hover text stays
  assert.equal(b.card("c129").querySelector("[data-check=critique]").textContent, "Critic: 2 findings");
  assert.equal(b.card("c129").querySelector("[data-route]").textContent, "Router: split into 3");
  assert.equal(row.querySelector(".chip.auto").textContent, "Goes on by itself");
  // Human test: test, review, then the dashed check size, then To test; the time last when known.
  assert.deepEqual(rowKinds(b, b.card("c121")), ["test", "review", "checksize", "other"]);
  assert.deepEqual(rowKinds(b, b.card("c120")), ["time"]);
  assert.equal(b.card("c120").querySelector(".timein").textContent, "In progress 1h 12m");
});

test("the face parts are in the mockup's order", () => {
  const b = boot();
  b.show(fixture(), { needs: NEEDS });
  const parts = (id) => b.card(id).children.filter((x) => !x.hidden).map((x) => x.className.split(" ")[0]);
  assert.deepEqual(parts("c120"), ["t", "dw-line", "tags", "s", "face-meta"]);
  assert.deepEqual(parts("c124"), ["t", "wbox", "face-meta"]); // an empty chip row hides
  assert.deepEqual(parts("c128"), ["t", "tags", "face-agent", "face-meta"]);
  assert.deepEqual(parts("c121"), ["t", "face-thumbs", "tags", "face-agent", "face-meta"]);
  // The agent line replaces the summary; the summary stays (hidden) for the panel and search.
  assert.equal(b.card("c128").querySelector(".face-agent").textContent, "Critic: No findings");
  assert.equal(b.card("c120").querySelector(":scope > .s").hidden, false);
  // The muted row: repo, issue, PR, acct 2, ago, every link and title kept.
  const meta = [...b.card("c128").querySelector(".face-meta").children].map((x) => x.dataset.meta);
  assert.deepEqual(meta, ["repo", "issue", "pr", "acct", "ago"]);
  assert.equal(b.card("c128").querySelectorAll(".face-meta a[href]").length, 2);
});

test("the wait box only for a question, asks, permission, failed or plan; To test is a chip", () => {
  const b = boot();
  b.show(fixture(), { needs: NEEDS });
  const q = b.card("c124").querySelector(".wbox");
  assert.ok(q, "a question gets the box");
  assert.equal(q.querySelector(".wb-k").textContent, "Asks you");
  const opts = q.querySelectorAll(".wb-opt");
  assert.deepEqual(opts.map((o) => o.textContent), ["Keep older", "Keep newer", "Keep both"]);
  assert.ok(opts.every((o) => o.localName === "span"), "choices are text, not buttons");
  assert.ok(b.card("c126").querySelector(".wbox.w-permission"), "a permission prompt gets the box");
  assert.equal(b.card("c125").querySelector(".wbox"), null, "a plain reply keeps the line");
  assert.ok(b.card("c125").querySelector(":scope > .wait.w-reply"));
  const ht = b.card("c121");
  assert.equal(ht.querySelector(".wbox"), null, "Human test gets no box");
  assert.equal(ht.querySelector(".chip.to-test").textContent, "To test");
});

test("a stuck card: warn border, its chip as the box's label", () => {
  const b = boot();
  b.show(fixture(), { needs: NEEDS });
  const k = b.card("c122");
  assert.ok(k.classList.contains("stuck"));
  const box = k.querySelector(":scope > .face-stuck");
  assert.ok(box);
  assert.equal(box.querySelector(".chip.stuck").textContent, "May be stuck");
  assert.match(box.textContent, /^May be stuck · same cargo test 6 times, 22m\. Nudged\.$/);
  assert.equal(k.children[1], box, "straight under the title");
});

test("no chip or link is lost: the same .tag, .chip and <a> on every card with and without face.js", () => {
  const withF = boot(), without = boot({ files: FILES.filter((f) => f !== "face.js") });
  withF.show(fixture(), { needs: NEEDS });
  without.show(fixture(), { needs: NEEDS });
  const sig = (el) => ["tag", "chip"].map((c) => el.querySelectorAll("." + c).length).concat(el.querySelectorAll("a").length, el.querySelectorAll("[title]").length);
  const ids = without.document.querySelectorAll(".card[data-id]").map((x) => x.dataset.id);
  assert.ok(ids.length >= 9);
  for (const id of ids) assert.deepEqual(sig(withF.card(id)).slice(0, 3), sig(without.card(id)).slice(0, 3), id);
  // Titles only grow (the agent line and the time add hover text), never shrink.
  for (const id of ids) assert.ok(sig(withF.card(id))[3] >= sig(without.card(id))[3], id);
});

test("card #0 is never drawn and never counted", () => {
  const b = boot();
  b.show(fixture(), { needs: NEEDS });
  assert.equal(b.card("router"), null);
  const head = b.document.querySelector('.col[data-col="In progress"] .colhead .n');
  assert.equal(head.textContent, "5"); // c120, c122, c124, c125, c126: not the router's card
  assert.equal(b.run(`colCards("In progress").length`), 5);
  assert.equal(b.run(`isSystemCard({number:0})&&isSystemCard({system:"router"})&&isSystemCard({fields:{system:"router"}})&&!isSystemCard({number:3})`), true);
  // The router's card waits too (NEEDS.router), but the pill counts 4: c124, c125 (a reply), c126, c121 (to test).
  assert.equal(b.document.getElementById("waitBtn").textContent, "Waiting on you · 4");
});

test("pills: order, counts, is:stuck AND is:waiting, the label pill, and no redraw when nothing changed", () => {
  const b = boot();
  b.show(fixture(), { needs: NEEDS });
  const bar = b.document.getElementById("filterbar");
  assert.equal(bar.hidden, false);
  assert.deepEqual(bar.children.map((x) => x.textContent), ["Waiting on you · 4", "May be stuck · 2", "30-second looks · 1"]);
  const wait = b.document.getElementById("waitBtn"), quick = b.document.getElementById("checkSizeBtn");
  assert.equal(wait.localName, "button");
  assert.equal(wait.getAttribute("aria-pressed"), "false");
  assert.match(wait.title, /waiting on you/);
  assert.match(quick.title, /30-second look/);
  assert.equal(b.document.querySelectorAll("header #waitBtn, header #checkSizeBtn").length, 0, "not in the header any more");
  // Nothing changed: the same buttons, untouched.
  b.run("render()");
  assert.equal(b.document.getElementById("waitBtn"), wait);
  // is:stuck AND is:waiting: only the card that is both.
  const stuckBtn = bar.children.find((x) => x.dataset.term === "is:stuck");
  stuckBtn.click();
  assert.equal(b.document.getElementById("q").value, "is:stuck");
  assert.deepEqual(b.document.querySelectorAll(".card[data-id]").map((x) => x.dataset.id).sort(), ["c122", "c126"]);
  wait.click();
  assert.equal(b.document.getElementById("q").value, "is:stuck is:waiting");
  assert.deepEqual(b.document.querySelectorAll(".card[data-id]").map((x) => x.dataset.id), ["c126"]);
  assert.equal(wait.getAttribute("aria-pressed"), "true");
  assert.equal(stuckBtn.getAttribute("aria-pressed"), "true");
  // A label term shows as a pressed pill; a click takes it out.
  b.show(fixture(), { needs: NEEDS, q: "label:frontend" });
  const lp = bar.children.find((x) => x.classList.contains("lblpill"));
  assert.equal(lp.textContent, "label:frontend");
  assert.equal(lp.getAttribute("aria-pressed"), "true");
  lp.click();
  assert.equal(b.document.getElementById("q").value, "");
  assert.equal(bar.children.some((x) => x.classList.contains("lblpill")), false);
  // A pill with nothing to show hides, unless its term is on.
  b.show([], { needs: {} });
  assert.equal(bar.hidden, true);
  b.show([], { needs: {}, q: "is:stuck" });
  assert.deepEqual(bar.children.map((x) => x.textContent), ["May be stuck · 0"]);
});

test("Human test shots: two small lazy images, loaded once and reused across renders", () => {
  let reads = 0;
  const b = boot();
  b.ctx.loadShot = (p) => { reads++; return Promise.resolve("data:image/png;base64,AAAA"); };
  b.show(fixture(), { needs: NEEDS });
  const imgs = b.card("c121").querySelectorAll(".face-thumb");
  assert.equal(imgs.length, 2);
  assert.deepEqual(imgs.map((i) => i.dataset.kind), ["before", "after"]);
  assert.ok(imgs.every((i) => i.getAttribute("loading") === "lazy" && i.getAttribute("decoding") === "async"));
  b.run("render()");
  const again = b.card("c121").querySelectorAll(".face-thumb");
  assert.equal(again[0], imgs[0], "the same decoded image node");
  assert.equal(reads, 2, "one read per shot");
  assert.equal(b.card("c120").querySelector(".face-thumbs"), null, "only Human test cards");
});

test("what changed: the counts and up to three paths on one line", () => {
  const b = boot();
  b.run(`CHG.briefs.c121={files:2,added:31,removed:9,outputs:0,saved:false,paths:["ui/panel.js","ui/board.css"]}`);
  b.show(fixture(), { needs: NEEDS });
  const line = b.card("c121").querySelector(".face-chg");
  assert.equal(line.textContent, "2 files +31 −9 · ui/panel.js · ui/board.css");
});

test("a stack card keeps its own layout", () => {
  const b = boot();
  const el = b.document.createElement("div");
  el.className = "card stk";
  el.innerHTML = `<div class="stk-top"><span class="t">x</span></div><div class="stk-sub">y</div><span class="tags"><span class="tag">z</span></span>`;
  const before = el.innerHTML;
  b.ctx.Face.arrange(el, { id: "s", column: "Human test", title: "x" });
  assert.equal(el.innerHTML, before);
});
