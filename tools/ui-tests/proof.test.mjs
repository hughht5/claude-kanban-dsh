// Tests for ui/proof.js (card #125): node --test tools/ui-tests/proof.test.mjs
// proof.js loads alone in a vm with stubs for board.js's invoke and stacks.js's stkMd. Drawing a
// report (docHtml) needs a real DOM and DOMPurify, so it is checked in the test app over CDP.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const SRC = readFileSync(fileURLToPath(new URL("../../ui/proof.js", import.meta.url)), "utf8");
const SECRET = /ct=|customtoken|idtoken|eyJ[A-Za-z0-9_-]{6,}|sk_(live|test)|password/i;

function load() {
  const calls = [];
  const answers = [];
  const ctx = {
    console, Math, String, Array, Map, Set, Promise,
    document: { addEventListener() {} },
    SECRET,
    esc: (s) => String(s ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c])),
    stkMd: (t) => (SECRET.test(t) ? '<span class="hidden-secret">hidden: looks like a secret</span>' : `<p>${t.replace(/\n/g, "<br>")}</p>`),
    invoke: (cmd, args) => { calls.push([cmd, args]); return new Promise((res) => answers.push(res)); },
  };
  ctx.window = ctx;
  vm.createContext(ctx);
  vm.runInContext(SRC, ctx, { filename: "proof.js" });
  return { P: ctx.Proof, ctx, calls, answers };
}

const item = (path, spellings, more = {}) => ({ path, name: path.split(/[\\/]/).pop(), kind: "image", state: "ok", spellings, ...more });

test("a path becomes a chip with its file name, and other paths show their last part", () => {
  const { P, ctx } = load();
  const shot = item("C:\\Users\\me\\final\\B-107-1.png", ["C:\\Users\\me\\final\\B-107-1.png"]);
  const sib = item("C:\\Users\\me\\final\\B-107-2.png", ["B-107-2.png"]);
  const text = "Screenshots: C:\\Users\\me\\final\\B-107-1.png (the line) and B-107-2.png. Code in C:\\Users\\me\\repo\\ui\\review.js, folder C:\\Users\\me\\mockup.";
  const { text: t, marks } = P.proofChips(text, [shot, sib]);
  assert.ok(!t.includes("C:\\"), t);
  const html = P.pfSwap(ctx.stkMd(t), marks, ctx.esc);
  assert.match(html, /<button type="button" class="pf-chip" data-pf-chip="C:\\Users\\me\\final\\B-107-1.png"[^>]*>B-107-1.png<\/button> \(the line\)/);
  assert.match(html, /data-pf-chip="C:\\Users\\me\\final\\B-107-2.png"[^>]*>B-107-2.png<\/button>\./);
  assert.match(html, /<span class="pf-short" title="C:\\Users\\me\\repo\\ui\\review.js">review.js<\/span>,/);
  assert.match(html, /<span class="pf-short" title="C:\\Users\\me\\mockup">mockup<\/span>\./);
  // Only the tooltips hold a full path.
  assert.ok(!html.replace(/(title|data-pf-chip)="[^"]*"/g, "").includes("C:\\"), html);
});

test("a bare name inside a full path is never chipped on its own, and two spellings both chip", () => {
  const { P, ctx } = load();
  const a = item("C:\\Users\\me\\s\\a.png", ["C:\\Users\\me\\s\\a.png", "C:/Users/me/s/a.png", "a.png"]);
  const { text, marks } = P.proofChips("See C:\\Users\\me\\s\\a.png, C:/Users/me/s/a.png and a.png.", [a]);
  const html = P.pfSwap(text, marks, ctx.esc);
  assert.equal((html.match(/class="pf-chip"/g) || []).length, 3);
  assert.ok(!html.replace(/(title|data-pf-chip)="[^"]*"/g, "").includes("Users"), html);
  // Two items on one line.
  const b = item("C:\\Users\\me\\s\\b.md", ["C:\\Users\\me\\s\\b.md"], { kind: "doc" });
  const two = P.proofChips("C:\\Users\\me\\s\\a.png and C:\\Users\\me\\s\\b.md", [a, b]);
  assert.equal(two.marks.filter((m) => m.item).length, 2);
});

test("code spans, code blocks and Markdown image and link targets are left as written", () => {
  const { P } = load();
  const a = item("C:\\Users\\me\\a.png", ["C:\\Users\\me\\a.png"]);
  const text = [
    "Run `C:\\Users\\me\\a.png --x` and see `C:\\Users\\me\\a.png`.",
    "![shot](C:\\Users\\me\\a.png) [link](C:\\Users\\me\\a.png)",
    "",
    "```",
    "C:\\Users\\me\\a.png",
    "```",
    "",
    "    C:\\Users\\me\\a.png",
    "",
    "1. Step",
    "    then C:\\Users\\me\\a.png",
  ].join("\n");
  const { text: t, marks } = P.proofChips(text, [a]);
  const lines = t.split("\n");
  assert.ok(lines[0].startsWith("Run `C:\\Users\\me\\a.png --x` and see PFMARK"), lines[0]);
  assert.equal(lines[1], "![shot](C:\\Users\\me\\a.png) [link](C:\\Users\\me\\a.png)");
  assert.equal(lines[4], "C:\\Users\\me\\a.png");
  assert.equal(lines[7], "    C:\\Users\\me\\a.png");
  assert.ok(lines[10].startsWith("    then PFMARK"), lines[10]);
  assert.equal(marks.filter((m) => m.item).length, 2);
  assert.deepEqual(Array.from(P.pfLines(text), (l) => l.code), [false, false, false, true, true, true, false, true, false, false, false]);
});

test("a folder with a dot keeps its whole path in the chip", () => {
  const { P, ctx } = load();
  const d = item("C:\\Users\\me\\.claude-kanban-dsh\\r.md", ["C:\\Users\\me\\.claude-kanban-dsh\\r.md"], { kind: "doc", state: "refused" });
  const { text, marks } = P.proofChips("Report C:\\Users\\me\\.claude-kanban-dsh\\r.md.", [d]);
  assert.match(P.pfSwap(text, marks, ctx.esc), /data-pf-chip="C:\\Users\\me\\.claude-kanban-dsh\\r.md"[^>]*>r.md<\/button>\./);
});

test("single line breaks are kept and secret-looking steps stay hidden with no chips", async () => {
  const { P, ctx, answers } = load();
  ctx.panel = null;
  const c = { id: "c1", version: 1, nextStep: "1. Open C:\\Users\\me\\a.png\n2. Check it" };
  ctx.proofSteps(c); // asks proof_list
  answers.shift()([item("C:\\Users\\me\\a.png", ["C:\\Users\\me\\a.png"])]);
  await new Promise((r) => setImmediate(r));
  const html = ctx.proofSteps(c);
  assert.match(html, /a.png<\/button><br>2. Check it/);
  const word = ["pass", "word"].join("");
  const hidden = ctx.proofSteps({ ...c, nextStep: `The ${word} is in C:\\Users\\me\\a.png` });
  assert.equal(hidden, '<span class="hidden-secret">hidden: looks like a secret</span>');
});

test("links in a report: only http and https go anywhere", () => {
  const { P } = load();
  assert.equal(P.pfLinkOk("https://example.com/x"), "outside");
  assert.equal(P.pfLinkOk("HTTP://example.com"), "outside");
  for (const h of ["102.md", "file:///C:/x.md", "#a", "javascript:alert(1)", "", null]) assert.equal(P.pfLinkOk(h), "none", String(h));
  assert.deepEqual([...P.PF_TAGS].filter((t) => ["form", "button", "input", "area", "map", "style", "img", "iframe", "script"].includes(t)), []);
  // No data-* or aria-* attributes: the board's page-wide click handlers act on data-* ones.
  assert.equal(P.PF_PURIFY.ALLOW_DATA_ATTR, false);
  assert.equal(P.PF_PURIFY.ALLOW_ARIA_ATTR, false);
  assert.deepEqual([...P.PF_PURIFY.ALLOWED_ATTR], ["href", "align", "start"]);
});

test("the proof list is asked once per card version, and a redraw asks nothing again", async () => {
  const { P, ctx, calls, answers } = load();
  ctx.panel = null;
  const c = { id: "c1", version: 3 };
  P.pfState(c);
  P.pfState(c);
  assert.equal(calls.filter(([k]) => k === "proof_list").length, 1);
  answers.shift()([item("C:\\Users\\me\\a.png", [])]);
  await new Promise((r) => setImmediate(r));
  assert.equal(P.pfItems(c).length, 1);
  P.pfItems(c);
  assert.equal(calls.length, 1);
  P.pfState({ id: "c1", version: 4 });
  assert.equal(calls.filter(([k]) => k === "proof_list").length, 2);
});

// A box as review.js draws it: one proof picture and one open report fold.
function fakeBox(imgPath, docPath) {
  const img = { attrs: { "data-pf": imgPath }, dataset: {}, getAttribute(k) { return this.attrs[k] ?? null; }, set src(v) { this.attrs.src = v; }, get src() { return this.attrs.src; } };
  const body = { dataset: {}, innerHTML: "" };
  const doc = { attrs: { "data-pf": docPath }, open: true, getAttribute(k) { return this.attrs[k] ?? null; }, querySelector() { return body; } };
  return { img, body, box: { querySelectorAll: (sel) => (sel.startsWith("img") ? [img] : [doc]) } };
}

test("a redraw puts the kept picture and report back and reads nothing again", async () => {
  const { P, ctx, calls, answers } = load();
  ctx.panel = null;
  const c = { id: "c1", version: 1 };
  const png = "C:\\Users\\me\\a.png", md = "C:\\Users\\me\\r.txt";
  P.pfState(c);
  answers.shift()([item(png, []), item(md, [], { kind: "doc" })]);
  await new Promise((r) => setImmediate(r));
  const first = fakeBox(png, md);
  P.proofFill(first.box);
  const reads = () => calls.filter(([k]) => k === "proof_read");
  assert.equal(reads().length, 2);
  answers.shift()({ kind: "image", url: "data:image/png;base64,QQ==" });
  answers.shift()({ kind: "doc", text: "The report", plain: true, cut: false });
  await new Promise((r) => setImmediate(r));
  // The board redraws: a new box, the same card version.
  const second = fakeBox(png, md);
  P.proofFill(second.box);
  P.pfItems(c);
  assert.equal(reads().length, 2, "no read again");
  assert.equal(calls.filter(([k]) => k === "proof_list").length, 1);
  assert.equal(second.img.src, "data:image/png;base64,QQ==");
  assert.match(second.body.innerHTML, /<pre class="pf-txt">The report<\/pre>/);
});
