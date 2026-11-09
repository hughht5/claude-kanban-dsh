// Settings › Sessions: the repos a card can be for (card #155), the Add card Repo list, kept in the
// private folder's repos.json. Rows of repo, short name and folder, with Add a repo, Remove and
// Save; on the real ui/*.js files in a vm (harness.mjs).
import test from "node:test";
import assert from "node:assert/strict";
import { boot, FILES } from "./harness.mjs";

const tick = async (n = 20) => { for (let i = 0; i < n; i++) await new Promise((r) => setImmediate(r)); };
const plain = (x) => JSON.parse(JSON.stringify(x)); // objects from the vm, compared as data

function settings({ dir = "~\\kanban-private", repos = [], save, load } = {}) {
  const saves = [];
  let list = repos;
  const b = boot({
    files: [...FILES, "settings.js"],
    invoke: {
      get_setting: ({ key }) => ({ key, value: key === "private.dir" ? dir : key.endsWith("_repos") ? "[]" : "3", default: "3" }),
      set_setting: ({ value }) => value,
      get_setting_log: () => [],
      lane_status: () => [],
      edit_repos: load || (() => list),
      // As the app answers: each row as written, with the name it now has as `was`.
      save_repos: save || (({ repos }) => { saves.push(plain(repos)); list = repos.map(({ repo, label, folder }) => ({ repo, label, folder, was: repo })); return list; }),
    },
  });
  return { b, saves, setList: (l) => { list = l; } };
}

const open = async (b) => { b.run(`openSettings("sessions")`); await tick(); return b.document.querySelector(".st-repos"); };
const values = (box) => box.querySelectorAll(".st-rrow").map((r) => [r.querySelector(".st-rr").value, r.querySelector(".st-rl").value, r.querySelector(".st-rf").value]);
const two = [{ repo: "acme/web", label: "web", folder: "~/web", was: "acme/web" }, { repo: "acme/docs", label: "", folder: "", was: "acme/docs" }];

test("settings, Sessions: the repo list sits under the private folder and shows each repo as written", async () => {
  const { b } = settings({ repos: two });
  const box = await open(b);
  assert.ok(box, "the box is there");
  const boxes = b.document.querySelectorAll(".st-box");
  assert.ok(boxes[boxes.indexOf(box) - 1].querySelector("#st-priv"), "straight after the private folder");
  assert.equal(box.querySelector("b").textContent, "Repos a card can be for");
  assert.match(box.querySelector(".meta").textContent, /Repo list on Add card.*repos\.json.*loginDirs.*is kept.*repos\.json\.bak.*Removing a repo leaves the cards already on it/);
  assert.deepEqual(values(box), [["acme/web", "web", "~/web"], ["acme/docs", "", ""]]);
});

test("settings, Sessions: add a repo, rename one, remove one, and Save sends the list in order", async () => {
  const { b, saves } = settings({ repos: two });
  const box = await open(b);
  box.querySelectorAll(".st-rx")[1].click(); // Remove acme/docs
  await tick();
  box.querySelector("#st-radd").click();
  await tick();
  assert.equal(box.querySelectorAll(".st-rrow").length, 2);
  const [first, last] = box.querySelectorAll(".st-rrow");
  first.querySelector(".st-rr").value = "acme/website"; // a rename keeps the name it was loaded with
  last.querySelector(".st-rr").value = " me/new-app ";
  last.querySelector(".st-rf").value = "~\\new-app";
  box.querySelector("#st-rsave").click();
  await tick();
  assert.deepEqual(saves, [[{ repo: "acme/website", label: "web", folder: "~/web", was: "acme/web" }, { repo: "me/new-app", label: "", folder: "~\\new-app" }]]);
  assert.equal(box.querySelector("#st-rmsg").textContent, "Saved");
  assert.deepEqual(values(box), [["acme/website", "web", "~/web"], ["me/new-app", "", "~\\new-app"]]);
});

test("settings, Sessions: a refused save says why and keeps the rows as typed", async () => {
  const { b } = settings({ repos: two.slice(0, 1), save: () => { throw new Error("\"acme\" isn't a repo: give it as owner/name"); } });
  const box = await open(b);
  box.querySelector("#st-radd").click();
  await tick();
  box.querySelectorAll(".st-rrow")[1].querySelector(".st-rr").value = "acme";
  box.querySelector("#st-rsave").click();
  await tick();
  assert.match(box.querySelector("#st-rmsg").textContent, /^Not saved: .*isn't a repo/);
  assert.ok(box.querySelector("#st-rmsg").classList.contains("err"));
  assert.deepEqual(values(box), [["acme/web", "web", "~/web"], ["acme", "", ""]]);
});

test("settings, Sessions: a repos.json the app can't read is shown as an error, with no Save", async () => {
  const { b } = settings({ load: () => { throw new Error("repos.json can't be read (expected a string): fix it by hand first"); } });
  const box = await open(b);
  assert.match(box.querySelector("#st-rmsg").textContent, /^Couldn't read the list: .*fix it by hand/);
  assert.equal(box.querySelector("#st-rsave"), null);
  assert.equal(box.querySelector("#st-radd"), null);
});

test("settings, Sessions: with no private folder the box asks for one, and saving a folder draws the list", async () => {
  const { b, setList } = settings({ dir: "" });
  const box = await open(b);
  assert.match(box.textContent, /Set a private folder above first/);
  assert.equal(box.querySelector("#st-rsave"), null);
  setList(two.slice(0, 1));
  b.document.querySelector("#st-priv").value = "~\\kanban-private";
  b.document.querySelector("#st-psave").click();
  await tick();
  assert.deepEqual(values(box), [["acme/web", "web", "~/web"]]);
  assert.ok(box.querySelector("#st-rsave"));
});
