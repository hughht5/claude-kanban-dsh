// The session's to-do list in the card's chat (card #158): steps.js's stepsChat on the real ui/*.js
// files in a vm (harness.mjs), with a stand-in for panel.js's open panel.
import test from "node:test";
import assert from "node:assert/strict";
import { boot } from "./harness.mjs";

const item = (text, status) => ({ text, status });
const six = [
  item("Make the worktrees", "completed"), item("Rewrite the prompts", "completed"),
  item("Update the docs", "completed"), item("Write the tests", "completed"),
  item("Code review, card-reviewer, card-tester", "in_progress"), item("Hand over", "pending"),
];

// A board with steps.js loaded, an open panel on card c1 whose chat has #p-todo, and the steps given.
function open(items, { store = {} } = {}) {
  const storage = { getItem: (k) => (k in store ? store[k] : null), setItem: (k, v) => { store[k] = String(v); }, removeItem: (k) => { delete store[k]; } };
  const b = boot({ storage });
  b.ctx.__items = items;
  b.run(`var host=document.createElement("div");host.innerHTML='<div class="todo" id="p-todo" hidden></div>';document.body.appendChild(host);
    var panel={id:"c1",el:host};steps=__items?{c1:{done:0,total:__items.length,line:"",items:__items}}:{};stepsChat();`);
  const el = b.document.getElementById("p-todo");
  return { b, el, store };
}
const rows = (el) => el.querySelectorAll(".todo-i").map((li) => [li.querySelector(".todo-m").textContent, li.querySelector(".todo-x").textContent, li.className.replace("todo-i ", "")]);

test("the chat shows the list with its counts and each task marked", () => {
  const { el } = open(six);
  assert.equal(el.hidden, false);
  assert.equal(el.querySelector(".todo-t").textContent, "6 tasks (4 done, 1 in progress, 1 open)");
  assert.deepEqual(rows(el)[0], ["✓", "Make the worktrees", "completed"]);
  assert.deepEqual(rows(el)[4], ["■", "Code review, card-reviewer, card-tester", "in_progress"]);
  assert.deepEqual(rows(el)[5], ["□", "Hand over", "pending"]);
  assert.equal(el.querySelector(".todo-h").getAttribute("aria-expanded"), "true");
  assert.equal(el.querySelector(".todo-more"), null, "six tasks fit");
});

test("no list, or an empty one, hides the box; a list arriving later shows it", () => {
  for (const items of [null, []]) {
    const { el } = open(items);
    assert.equal(el.hidden, true);
    assert.equal(el.textContent, "");
  }
  const { b, el } = open(null);
  b.ctx.__items = [item("One", "in_progress")];
  b.run(`steps={c1:{done:0,total:1,line:"",items:__items}};stepsChat()`);
  assert.equal(el.hidden, false);
  assert.equal(el.querySelector(".todo-t").textContent, "1 task (1 in progress)");
  b.run("steps={};stepsChat()");
  assert.equal(el.hidden, true, "the list going away hides it again");
});

test("a change of status redraws; the same list doesn't", () => {
  const { b, el } = open(six);
  const before = el.querySelector(".todo-h");
  b.run("stepsChat()");
  assert.equal(el.querySelector(".todo-h"), before, "nothing changed: not redrawn");
  b.ctx.__items = six.map((x) => ({ ...x, status: "completed" }));
  b.run(`steps={c1:{done:6,total:6,line:"",items:__items}};stepsChat()`);
  assert.equal(el.querySelector(".todo-t").textContent, "6 tasks (all done)");
  assert.ok(el.classList.contains("all-done"));
});

test("the header folds the list to one line, remembered", () => {
  const { el, store } = open(six);
  el.querySelector(".todo-h").click();
  assert.equal(store["panel.todo"], "folded");
  assert.equal(el.querySelector(".todo-l"), null, "no list while folded");
  assert.equal(el.querySelector(".todo-h").getAttribute("aria-expanded"), "false");
  assert.equal(el.querySelector(".todo-now").textContent, "Code review, card-reviewer, card-tester", "the task in progress beside the counts");
  // A new panel reads the fold.
  const next = open(six, { store });
  assert.equal(next.el.querySelector(".todo-l"), null);
  next.el.querySelector(".todo-h").click();
  assert.equal(store["panel.todo"], "open");
  assert.equal(next.el.querySelectorAll(".todo-i").length, 6);

});

test("a long list hides its earliest done tasks behind '… +N completed'", () => {
  const items = [...Array(9)].map((_, i) => item("Done " + (i + 1), "completed")).concat([item("Now", "in_progress"), item("Later", "pending")]);
  const { el } = open(items);
  assert.equal(el.querySelectorAll(".todo-i").length, 8);
  assert.deepEqual(rows(el).map((r) => r[1]), ["Done 4", "Done 5", "Done 6", "Done 7", "Done 8", "Done 9", "Now", "Later"]);
  assert.equal(el.querySelector(".todo-more").textContent, "… +3 completed");
  el.querySelector(".todo-more").click();
  assert.equal(el.querySelectorAll(".todo-i").length, 11, "show all");
  assert.equal(el.querySelector(".todo-more"), null);
});

test("open tasks are never hidden, however long the list", () => {
  const items = [...Array(12)].map((_, i) => item("Open " + (i + 1), "pending"));
  const { el } = open(items);
  assert.equal(el.querySelectorAll(".todo-i").length, 12);
  assert.equal(el.querySelector(".todo-more"), null);
});

test("task text is shown as text, never as HTML", () => {
  const { el } = open([item("<img src=x onerror=alert(1)>", "pending")]);
  assert.equal(el.querySelector("img"), null);
  assert.equal(el.querySelector(".todo-x").textContent, "<img src=x onerror=alert(1)>");
});

test("no open panel: nothing happens", () => {
  const b = boot();
  assert.doesNotThrow(() => b.run("stepsChat()"));
});

test("a subagent's view hides the list; back on Main shows it", () => {
  const { b, el } = open(six);
  b.run(`panel.view="agent-1";stepsChat()`);
  assert.equal(el.hidden, true);
  b.run(`panel.view="";stepsChat()`);
  assert.equal(el.hidden, false);
  assert.equal(el.querySelectorAll(".todo-i").length, 6);
});

test("folding keeps focus on the header, and still folds when storage throws", () => {
  const { b, el } = open(six);
  b.run(`localStorage.setItem=()=>{throw new Error("no storage")};localStorage.getItem=()=>{throw new Error("no storage")}`);
  el.querySelector(".todo-h").focus();
  el.querySelector(".todo-h").click();
  assert.equal(el.querySelector(".todo-l"), null, "folded");
  assert.equal(b.document.activeElement, el.querySelector(".todo-h"), "focus on the new header");
  el.querySelector(".todo-h").click();
  assert.equal(el.querySelectorAll(".todo-i").length, 6, "unfolded again");
});
