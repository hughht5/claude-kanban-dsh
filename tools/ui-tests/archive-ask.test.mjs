// The Done pile (card #131), on the real ui/*.js files in a vm.
import test from "node:test";
import assert from "node:assert/strict";
import { boot } from "./harness.mjs";

const ask = (over = {}) => ({ reason: "its work is live", evidence: "dsh-110 is in origin/main", at: "2026-11-08T10:00:00Z", by: "the router", column: "Human test", ...over });

function fixture() {
  return [
    { id: "c110", number: 110, title: "Search sessions", column: "Human test", version: 21, archiveAsk: ask() },
    { id: "c111", number: 111, title: "Older ask", column: "Deploy complete", version: 4, archiveAsk: ask({ column: "Deploy complete", at: "2026-11-08T09:00:00Z", evidence: null }) },
    { id: "c96", number: 96, title: "Stack lead", column: "Human test", version: 9, stack: [97, 98], archiveAsk: ask({ reason: "the stack shipped" }) },
    { id: "c97", number: 97, title: "Child A", column: "Human test", version: 3, stackOf: 96 },
    { id: "c98", number: 98, title: "Child B", column: "Human test", version: 3, stackOf: 96 },
    { id: "c120", number: 120, title: "Still working", column: "In progress", version: 2 },
    { id: "c121", number: 121, title: "Archived already", column: "Done", version: 7, archived: true, archiveAsk: ask() },
  ];
}

const pile = (b) => b.document.querySelector(".col.c-DonePile");
const ids = (el) => [...el.querySelectorAll(".pile-row")].map((r) => r.getAttribute("data-id"));

test("the pile holds the Router's asks, oldest first, and they leave their own columns", () => {
  const b = boot();
  b.show(fixture());
  const p = pile(b);
  assert.ok(p, "the Done pile is drawn");
  assert.deepEqual(ids(p), ["c111", "c110", "c96"]);
  assert.match(p.querySelector(".colhead").textContent, /Done pile\s*3/);
  assert.equal(p.querySelector(".pile-all").textContent, "Archive all 3");
  // Not in their own columns while they wait.
  const ht = b.document.querySelector('.col[data-col="Human test"]');
  assert.equal(ht.querySelector('.card[data-id="c110"]'), null);
  assert.ok(b.document.querySelector('.col[data-col="In progress"] .card[data-id="c120"]'));
  // The pile sits before the Archive column.
  const arch = b.document.querySelector(".col.c-Archived");
  assert.equal(p.nextElementSibling, arch);
});

test("each row says where it came from, why, its evidence and its stack", () => {
  const b = boot();
  b.show(fixture());
  const row = (id) => pile(b).querySelector(`.pile-row[data-id="${id}"]`);
  assert.match(row("c110").textContent, /From Human test/);
  assert.match(row("c110").textContent, /its work is live/);
  assert.match(row("c110").textContent, /dsh-110 is in origin\/main/);
  assert.match(row("c111").textContent, /From Deploy complete/);
  assert.equal(row("c111").querySelector(".pile-evidence"), null);
  assert.match(row("c96").textContent, /and its 2 children/);
});

test("a row names the running sessions archiving would stop", () => {
  const b = boot();
  b.run(`globalThis.activity={c110:"idle",c97:"working",c98:"ended"}`);
  b.show(fixture());
  const p = pile(b);
  assert.match(p.querySelector('.pile-row[data-id="c110"]').textContent, /Its session is running: archiving stops it/);
  assert.match(p.querySelector('.pile-row[data-id="c96"]').textContent, /#97 is running: archiving stops it/);
  assert.equal(p.querySelector('.pile-row[data-id="c111"] .pile-run'), null);
  b.run(`globalThis.activity={c97:"working",c98:"permission"}`);
  b.show(fixture());
  assert.match(pile(b).querySelector('.pile-row[data-id="c96"]').textContent, /#97 and #98 are running: archiving stops them/);
});

test("Archive all sends every card shown, with its version, and a refusal stays with its reason", async () => {
  const cards = fixture();
  const b = boot({
    invoke: {
      list_cards: () => cards,
      router_archive_approve: ({ items }) => items.map((i) => ({ id: i.id, number: +i.id.slice(1), done: i.id !== "c96", why: i.id === "c96" ? "card #96's stack has #97 in Ship it" : undefined })),
    },
  });
  b.show(cards);
  pile(b).querySelector(".pile-all").click();
  await new Promise((r) => setImmediate(r));
  await new Promise((r) => setImmediate(r));
  const sent = b.calls.filter(([c]) => c === "router_archive_approve");
  assert.equal(sent.length, 1);
  assert.deepEqual(JSON.parse(JSON.stringify(sent[0][1].items)), [
    { id: "c111", version: 4 },
    { id: "c110", version: 21 },
    { id: "c96", version: 9 },
  ]);
  b.run("render()");
  assert.match(pile(b).querySelector('.pile-row[data-id="c96"]').textContent, /Left here: card #96's stack has #97 in Ship it/);
});

test("Archive all sends only the cards the filter shows", async () => {
  const b = boot();
  b.show(fixture(), { q: "search" });
  assert.deepEqual(ids(pile(b)), ["c110"]);
  pile(b).querySelector(".pile-all").click();
  await new Promise((r) => setImmediate(r));
  const sent = b.calls.filter(([c]) => c === "router_archive_approve");
  assert.deepEqual(JSON.parse(JSON.stringify(sent[0][1].items)), [{ id: "c110", version: 21 }]);
});

test("Not now sends just that card", async () => {
  const b = boot();
  b.show(fixture());
  pile(b).querySelector('.pile-row[data-id="c110"] .pile-not').click();
  await new Promise((r) => setImmediate(r));
  const sent = b.calls.filter(([c]) => c === "router_archive_dismiss");
  assert.equal(sent.length, 1);
  assert.deepEqual(JSON.parse(JSON.stringify(sent[0][1].items)), [{ id: "c110", version: 21 }]);
  assert.equal(b.calls.filter(([c]) => c === "router_archive_approve").length, 0);
});

test("no asks, no pile", () => {
  const b = boot();
  b.show([{ id: "c1", number: 1, title: "plain", column: "Human test", version: 1 }]);
  assert.equal(pile(b), null);
  assert.ok(b.document.querySelector('.col[data-col="Human test"] .card[data-id="c1"]'));
});
