// A card that needed Hugh keeps its place once its need clears (card #132): the held places in
// ui/needs.js and the drag in ui/board.js's placeCard, on the real ui/*.js files in a vm.
import test from "node:test";
import assert from "node:assert/strict";
import { boot } from "./harness.mjs";

const NOW = Date.now();
const iso = (minsAgo) => new Date(NOW - minsAgo * 60000).toISOString();
const IP = "In progress";

// Four cards in In progress, newest update first: a, b, c, d.
const fixture = () => ["a", "b", "c", "d"].map((id, i) => ({ id, number: 10 + i, title: id, column: IP, version: 1, updatedAt: iso(i + 1), columnSince: iso(600) }));

// A board whose needs_you answers whatever the test last set, with a localStorage that keeps.
// The page's own first load (list_cards, needs_you) settles before the test sets its cards.
async function board({ storage = memStorage(), cards = fixture() } = {}) {
  const st = { needs: {} };
  const b = boot({ storage, invoke: { list_cards: () => cards, needs_you: () => (typeof st.needs === "function" ? st.needs() : st.needs), update_card: () => ({ ok: true }) } });
  await new Promise((r) => setImmediate(r));
  b.show(cards);
  b.setNeeds = async (n) => { st.needs = n; await b.run("loadNeeds()"); };
  b.st = st;
  b.order = (col = IP) => [...b.document.querySelectorAll(`.col[data-col="${col}"] .card`)].map((k) => k.dataset.id);
  b.edit = (id, patch) => { b.ctx.__p = patch; b.run(`Object.assign(cards.find(x=>x.id===${JSON.stringify(id)}),__p);render()`); };
  b.held = () => JSON.parse(b.run("JSON.stringify(held)"));
  // What placeCard wrote, applied to the cards as the next load would bring them.
  b.applyWrites = () => {
    for (const [cmd, args] of b.calls.splice(0)) if (cmd === "update_card") b.edit(args.id, args.patch);
  };
  return b;
}
function memStorage() {
  const m = new Map();
  return { getItem: (k) => (m.has(k) ? m.get(k) : null), setItem: (k, v) => m.set(k, String(v)), removeItem: (k) => m.delete(k) };
}

test("a card keeps its place at the top after its need clears", async () => {
  const b = await board();
  assert.deepEqual(b.order(), ["a", "b", "c", "d"]);
  await b.setNeeds({ c: { reason: "unread", at: iso(1) }, d: { reason: "permission", at: iso(2) } });
  assert.deepEqual(b.order(), ["c", "d", "a", "b"], "cards that need Hugh go first, newest need first");
  await b.setNeeds({ d: { reason: "permission", at: iso(2) } });
  assert.deepEqual(b.order(), ["c", "d", "a", "b"], "c was opened: it stays first");
  await b.setNeeds({});
  assert.deepEqual(b.order(), ["c", "d", "a", "b"], "d was answered: it stays second");
  assert.deepEqual(Object.keys(b.held()).sort(), ["c", "d"]);
  // An ordinary update doesn't move them either.
  b.edit("b", { updatedAt: new Date().toISOString() });
  assert.deepEqual(b.order(), ["c", "d", "b", "a"]);
});

test("a later need goes above a held card, and its own hold ends", async () => {
  const b = await board();
  await b.setNeeds({ c: { reason: "unread", at: iso(1) } });
  await b.setNeeds({});
  await b.setNeeds({ b: { reason: "unread", at: iso(0) } });
  assert.deepEqual(b.order(), ["b", "c", "a", "d"]);
  // c needs him again, newer still: its hold goes and the new need puts it on top.
  await b.setNeeds({ b: { reason: "unread", at: iso(0) }, c: { reason: "failed", at: new Date().toISOString() } });
  assert.deepEqual(b.order(), ["c", "b", "a", "d"]);
  assert.equal(b.held().c, undefined);
});

test("a column move ends the hold for good, with or without columnSince", async () => {
  for (const since of [null, "later"]) {
    const b = await board();
    await b.setNeeds({ d: { reason: "unread", at: iso(1) } });
    await b.setNeeds({});
    assert.deepEqual(b.order(), ["d", "a", "b", "c"]);
    b.edit("d", { column: "Human test", columnSince: since && new Date(Date.now() + 1000).toISOString() });
    assert.deepEqual(b.order(), ["a", "b", "c"]);
    assert.deepEqual(b.order("Human test"), ["d"]);
    assert.equal(b.held().d, undefined, "seen in another column: dropped");
    b.edit("d", { column: IP });
    assert.deepEqual(b.order(), ["a", "b", "c", "d"], "moved back: an ordinary card again");
  }
});

test("a move out and back between two loads ends the hold (columnSince after the hold)", async () => {
  const b = await board();
  await b.setNeeds({ d: { reason: "unread", at: iso(1) } });
  await b.setNeeds({});
  b.edit("d", { columnSince: new Date(Date.now() + 1000).toISOString() });
  assert.deepEqual(b.order(), ["a", "b", "c", "d"]);
  assert.equal(b.held().d, undefined);
});

test("an archived card loses its hold", async () => {
  const b = await board();
  await b.setNeeds({ d: { reason: "unread", at: iso(1) } });
  await b.setNeeds({});
  b.edit("d", { archived: true });
  assert.equal(b.held().d, undefined);
});

test("a hold survives a fresh page with the same storage", async () => {
  const storage = memStorage();
  const b = await board({ storage });
  await b.setNeeds({ c: { reason: "unread", at: iso(1) } });
  await b.setNeeds({});
  const again = await board({ storage });
  assert.deepEqual(again.order(), ["c", "a", "b", "d"]);
  // Storage that throws: the board still works, the hold lasts for the page.
  const bad = { getItem() { throw new Error("blocked"); }, setItem() { throw new Error("blocked"); }, removeItem() {} };
  const b2 = await board({ storage: bad });
  await b2.setNeeds({ c: { reason: "unread", at: iso(1) } });
  await b2.setNeeds({});
  assert.deepEqual(b2.order(), ["c", "a", "b", "d"]);
});

test("a drag among held cards lands where it was dropped and ends the column's holds", async () => {
  const b = await board();
  await b.setNeeds({ c: { reason: "unread", at: iso(1) }, d: { reason: "unread", at: iso(2) } });
  await b.setNeeds({});
  assert.deepEqual(b.order(), ["c", "d", "a", "b"]);
  b.calls.splice(0);
  await b.run(`placeCard("b",${JSON.stringify(IP)},"d")`);
  assert.deepEqual(b.held(), {}, "the drag ended the holds");
  b.applyWrites();
  assert.deepEqual(b.order(), ["c", "b", "d", "a"]);
});

test("a drop right below a card that needs Hugh lands exactly there", async () => {
  // N needs Hugh and has order 5; p and q have orders 1 and 2; e has none.
  const cards = [
    { id: "N", number: 1, title: "N", column: IP, version: 1, order: 5, orderCol: IP, updatedAt: iso(1) },
    { id: "p", number: 2, title: "p", column: IP, version: 1, order: 1, orderCol: IP, updatedAt: iso(2) },
    { id: "q", number: 3, title: "q", column: IP, version: 1, order: 2, orderCol: IP, updatedAt: iso(3) },
    { id: "e", number: 4, title: "e", column: IP, version: 1, updatedAt: iso(4) },
  ];
  const b = await board({ cards });
  await b.setNeeds({ N: { reason: "permission", at: iso(1) } });
  assert.deepEqual(b.order(), ["N", "p", "q", "e"]);
  b.calls.splice(0);
  await b.run(`placeCard("e",${JSON.stringify(IP)},"p")`);
  b.applyWrites();
  assert.deepEqual(b.order(), ["N", "e", "p", "q"]);
});

test("with nothing pinned, a drop between ordered cards still takes the midpoint", async () => {
  const cards = ["p", "q", "r"].map((id, i) => ({ id, number: i + 1, title: id, column: IP, version: 1, order: i + 1, orderCol: IP, updatedAt: iso(i) }));
  const b = await board({ cards });
  b.calls.splice(0);
  await b.run(`placeCard("r",${JSON.stringify(IP)},"q")`);
  const writes = b.calls.filter(([c]) => c === "update_card");
  assert.equal(writes.length, 1);
  assert.deepEqual({ ...writes[0][1].patch }, { order: 1.5, orderCol: IP });
  b.applyWrites();
  assert.deepEqual(b.order(), ["p", "r", "q"]);
});

test("a card he dragged while it needed him keeps his drop once its need clears", async () => {
  const b = await board();
  await b.setNeeds({ a: { reason: "permission", at: iso(1) } });
  assert.deepEqual(b.order(), ["a", "b", "c", "d"]);
  b.calls.splice(0);
  await b.run(`placeCard("a",${JSON.stringify(IP)},"d")`);
  b.applyWrites();
  assert.deepEqual(b.order(), ["a", "b", "c", "d"], "still pinned while it needs him");
  await b.setNeeds({});
  assert.deepEqual(b.order(), ["b", "c", "a", "d"], "his drop: third");
  assert.equal(b.held().a, undefined);
});

test("a card moved to another column while it needed him gets no hold there", async () => {
  const b = await board();
  await b.setNeeds({ d: { reason: "unread", at: iso(5) } });
  b.edit("d", { column: "Human test", columnSince: iso(1) });
  assert.deepEqual(b.order("Human test"), ["d"]);
  await b.setNeeds({});
  assert.equal(b.held().d, undefined);
  // A session's own move just after the turn end that made the need does keep it.
  const b2 = await board();
  await b2.setNeeds({ d: { reason: "unread", at: iso(5) } });
  b2.edit("d", { column: "Human test", columnSince: new Date(NOW - 5 * 60000 + 2000).toISOString() });
  await b2.setNeeds({});
  assert.ok(b2.held().d, "moved by its own session at the turn end: held");
});

test("a drop below the held cards takes the midpoint and keeps their holds", async () => {
  const cards = [
    { id: "h", number: 1, title: "h", column: IP, version: 1, updatedAt: iso(9) },
    { id: "p", number: 2, title: "p", column: IP, version: 1, order: 1, orderCol: IP, updatedAt: iso(2) },
    { id: "q", number: 3, title: "q", column: IP, version: 1, order: 2, orderCol: IP, updatedAt: iso(3) },
  ];
  const b = await board({ cards });
  await b.setNeeds({ h: { reason: "unread", at: iso(1) } });
  await b.setNeeds({});
  assert.deepEqual(b.order(), ["h", "p", "q"]);
  b.calls.splice(0);
  await b.run(`placeCard("q",${JSON.stringify(IP)},"p")`);
  const writes = b.calls.filter(([c]) => c === "update_card");
  assert.deepEqual(writes.map(([, a]) => [a.id, { ...a.patch }]), [["q", { order: 0, orderCol: IP }]], "one write");
  b.applyWrites();
  assert.deepEqual(b.order(), ["h", "q", "p"]);
  assert.ok(b.held().h, "h keeps its hold");
});

test("an older needs read that answers late is ignored", async () => {
  const b = await board();
  await b.setNeeds({ c: { reason: "unread", at: iso(1) } });
  const waits = [];
  b.st.needs = () => new Promise((r) => waits.push(r));
  const first = b.run("loadNeeds()"), second = b.run("loadNeeds()");
  waits[1]({}); await second; // the newer read: c was opened
  waits[0]({ c: { reason: "unread", at: iso(1) } }); await first; // the older one lands last
  assert.equal(b.run("JSON.stringify(needs)"), "{}");
  assert.ok(b.held().c);
  assert.deepEqual(b.order(), ["c", "a", "b", "d"]);
});
