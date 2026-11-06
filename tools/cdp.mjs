// Drives the app's WebView over the Chrome DevTools Protocol, for checking the UI from a script.
// The app needs its WebView debug port: start it with
//   WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9333 --remote-allow-origins=http://127.0.0.1:9333
// (live 9333; the test app gets 9334 from scripts/install.ps1 -Channel test).
//
//   node tools/cdp.mjs eval "<expression>"            the port in CDP_PORT, else 9333
//   CDP_PORT=9334 node tools/cdp.mjs shot <out.png> [light|dark] [css selector]
//
// shot (card #101):
// - refuses when CDP_PORT isn't set, so a shot never comes from the live board by accident;
// - with a css selector, crops to that element (nothing matches: an error, and no file);
// - hides the terminal (.xterm), the chat pane (.chat) and masked inputs while it captures,
//   except, with a selector, the element shot and each one that holds it (so a change inside the
//   chat can be shot); a masked input stays hidden unless it is the element shot;
// - emulates the colour scheme (light unless dark is given) for the capture only;
// - never brings the window to the front;
// - writes no file when every pixel is the same colour ("blank shot");
// - reads the build the app runs (its update_status command: running.commit) and writes it into
//   the PNG as a tEXt chunk, "kanban-build", right after IHDR ("unknown" when the page can't say),
//   then prints "saved <file> (...)" and "build <commit>".
// The hiding style, its marks and the emulated colour scheme are undone in a finally, also on a
// failure. Every refusal or failure prints why and exits 1.
import { writeFileSync } from "node:fs";
import { inflateSync } from "node:zlib";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

const USAGE =
  'usage: node tools/cdp.mjs eval "<js>"   or   CDP_PORT=9334 node tools/cdp.mjs shot <out.png> [light|dark] [css selector]';
const HIDE_ID = "cdp-shot-hide";
// What a shot hides: the terminal, the chat pane and masked inputs.
const HIDE = [".xterm", ".chat", "input[type=password]"];
// The mark on the element shot and each element that holds it, which the hiding style spares.
const KEEP = "data-cdp-shot-keep";
// The PNG tEXt keyword the build goes under (src-tauri/src/evidence.rs reads it).
export const BUILD_KEYWORD = "kanban-build";
const J = JSON.stringify;

// A refusal or failure we explain ourselves: its message is printed without a stack.
class Fail extends Error {}

// shot's arguments: the output file, then an optional scheme, then an optional selector (the
// rest of the line, so an unquoted "a b" selector still works).
export function parseShotArgs(args) {
  const [out, ...rest] = args;
  if (!out) throw new Fail(`shot needs an output file. ${USAGE}`);
  let scheme = "light";
  if (rest[0] === "light" || rest[0] === "dark") scheme = rest.shift();
  const selector = rest.join(" ").trim() || null;
  return { out, scheme, selector };
}

// True when every pixel of the PNG is the same colour. Reads the non-interlaced 8- and 16-bit
// PNGs Page.captureScreenshot gives; for any other kind it answers false, so the file is kept.
export function isBlankPng(buf) {
  const SIG = [137, 80, 78, 71, 13, 10, 26, 10];
  if (buf.length < 8 || SIG.some((b, i) => buf[i] !== b)) throw new Fail("the capture isn't a PNG");
  let pos = 8, w = 0, h = 0, depth = 0, type = 0, interlace = 0;
  const idat = [];
  while (pos + 8 <= buf.length) {
    const len = buf.readUInt32BE(pos), kind = buf.toString("latin1", pos + 4, pos + 8);
    const data = buf.subarray(pos + 8, pos + 8 + len);
    if (kind === "IHDR") { w = data.readUInt32BE(0); h = data.readUInt32BE(4); depth = data[8]; type = data[9]; interlace = data[12]; }
    else if (kind === "IDAT") idat.push(data);
    else if (kind === "IEND") break;
    pos += 12 + len;
  }
  const channels = { 0: 1, 2: 3, 3: 1, 4: 2, 6: 4 }[type];
  if (!w || !h || !channels || interlace || depth < 8) return false;
  const bpp = (channels * depth) / 8, stride = w * bpp;
  const raw = inflateSync(Buffer.concat(idat));
  if (raw.length < h * (stride + 1)) throw new Fail("the capture is a broken PNG");
  let prev = Buffer.alloc(stride), cur = Buffer.alloc(stride), first = null;
  for (let y = 0; y < h; y++) {
    const at = y * (stride + 1), f = raw[at];
    for (let i = 0; i < stride; i++) {
      const a = i >= bpp ? cur[i - bpp] : 0, b = prev[i], c = i >= bpp ? prev[i - bpp] : 0;
      let v = raw[at + 1 + i];
      if (f === 1) v += a;
      else if (f === 2) v += b;
      else if (f === 3) v += (a + b) >> 1;
      else if (f === 4) {
        const p = a + b - c, pa = Math.abs(p - a), pb = Math.abs(p - b), pc = Math.abs(p - c);
        v += pa <= pb && pa <= pc ? a : pb <= pc ? b : c;
      }
      cur[i] = v & 255;
    }
    if (!first) first = Buffer.from(cur.subarray(0, bpp));
    for (let i = 0; i < stride; i += bpp) for (let k = 0; k < bpp; k++) if (cur[i + k] !== first[k]) return false;
    [prev, cur] = [cur, prev];
  }
  return true;
}

// The CRC-32 a PNG chunk ends with (as zlib's), over its type and data.
const CRC_TABLE = (() => {
  const t = new Uint32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    t[n] = c >>> 0;
  }
  return t;
})();
export function crc32(buf) {
  let c = 0xffffffff;
  for (const b of buf) c = CRC_TABLE[(c ^ b) & 255] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

// A build name as the shot carries it: only 0-9 a-z A-Z . _ - and at most 64 of them, the same
// rule src-tauri/src/evidence.rs reads it with; "unknown" when nothing is left.
export function cleanBuild(c) {
  const b = typeof c === "string" ? c.replace(/[^0-9A-Za-z._-]/g, "").slice(0, 64) : "";
  return b || "unknown";
}

// A PNG tEXt chunk: length, "tEXt", keyword, a 0 byte, the text, then the CRC.
export function textChunk(keyword, text) {
  const data = Buffer.concat([Buffer.from(keyword, "latin1"), Buffer.from([0]), Buffer.from(text, "latin1")]);
  const out = Buffer.alloc(12 + data.length);
  out.writeUInt32BE(data.length, 0);
  out.write("tEXt", 4, "latin1");
  data.copy(out, 8);
  out.writeUInt32BE(crc32(out.subarray(4, 8 + data.length)), 8 + data.length);
  return out;
}

// The PNG with the build in a tEXt chunk (BUILD_KEYWORD) right after its IHDR chunk.
export function withBuild(png, build) {
  if (png.length < 8 + 12 + 13 || png.toString("latin1", 12, 16) !== "IHDR") throw new Fail("the capture is a broken PNG");
  const end = 8 + 12 + png.readUInt32BE(8);
  if (end > png.length) throw new Fail("the capture is a broken PNG");
  return Buffer.concat([png.subarray(0, end), textChunk(BUILD_KEYWORD, cleanBuild(build)), png.subarray(end)]);
}

// Opens the page's DevTools socket. send(method, params, ms) answers the raw reply; ms 0 waits
// for ever (eval keeps its old behaviour), otherwise a reply that doesn't come in time fails.
async function connect(port) {
  let pages;
  try {
    pages = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
  } catch (e) {
    throw new Fail(`Nothing answers on port ${port}: is the app running with its debug port?`);
  }
  const page = Array.isArray(pages) && pages.find((p) => p.type === "page");
  if (!page) throw new Fail(`No page on port ${port}`);
  const ws = new WebSocket(page.webSocketDebuggerUrl, { headers: { Origin: `http://127.0.0.1:${port}` } });
  let id = 0;
  const pending = new Map();
  ws.onmessage = (m) => {
    const msg = JSON.parse(m.data);
    if (msg.id && pending.has(msg.id)) {
      pending.get(msg.id)(msg);
      pending.delete(msg.id);
    }
  };
  await new Promise((ok, no) => {
    const t = setTimeout(() => no(new Fail(`The page on port ${port} didn't open its socket`)), 10000);
    ws.onopen = () => { clearTimeout(t); ok(); };
    ws.onerror = () => { clearTimeout(t); no(new Fail(`Couldn't open the page's socket on port ${port}`)); };
  });
  const send = (method, params = {}, ms = 20000) =>
    new Promise((ok, no) => {
      const n = ++id;
      const t = ms ? setTimeout(() => { pending.delete(n); no(new Fail(`${method} got no answer in ${ms / 1000} s`)); }, ms) : null;
      pending.set(n, (msg) => { if (t) clearTimeout(t); ok(msg); });
      ws.send(J({ id: n, method, params }));
    });
  return { send, close: () => ws.close() };
}

// Runs in the page: the build the app runs, from its update_status command (as ui/update.js
// calls it, through window.__TAURI__.core, else window.__TAURI_INTERNALS__), or null.
async function pageBuild() {
  try {
    const core = window.__TAURI__ && window.__TAURI__.core;
    const internals = window.__TAURI_INTERNALS__;
    const s = core && typeof core.invoke === "function" ? await core.invoke("update_status")
      : internals && typeof internals.invoke === "function" ? await internals.invoke("update_status", {})
      : null;
    const c = s && s.running && s.running.commit;
    return typeof c === "string" ? c : null;
  } catch (e) {
    return null;
  }
}

// Runs in the page: hides `hide` (the terminal, the chat, masked inputs) while the shot is
// taken. With a selector, its element (the first match, as findBox measures) and each element
// that holds it are marked `keep`, and the style spares them; every other one is hidden, also
// one drawn after this step. Marks from an earlier shot are cleared first.
function hideFor(id, keep, hide, sel) {
  for (const e of document.querySelectorAll(`[${keep}]`)) e.removeAttribute(keep);
  let el = null;
  if (sel) {
    try { el = document.querySelector(sel); } catch (e) { el = null; }
  }
  for (; el; el = el.parentElement) el.setAttribute(keep, "");
  let s = document.getElementById(id);
  if (!s) { s = document.createElement("style"); s.id = id; document.head.append(s); }
  s.textContent = hide.map((h) => `${h}:not([${keep}])`).join(",") + "{visibility:hidden!important}";
  return true;
}

// Runs in the page: removes the hiding style and every mark.
function unhide(id, keep) {
  const s = document.getElementById(id);
  if (s) s.remove();
  for (const e of document.querySelectorAll(`[${keep}]`)) e.removeAttribute(keep);
  return true;
}

// Runs in the page: the element's box on screen, in page coordinates, or why there is none.
function findBox(sel) {
  let el;
  try { el = document.querySelector(sel); } catch (e) { return { error: `Not a valid css selector: ${sel}` }; }
  if (!el) return { error: `Nothing on the page matches ${sel}: no shot taken` };
  el.scrollIntoView({ block: "nearest", inline: "nearest" });
  const r = el.getBoundingClientRect();
  const x0 = Math.max(0, r.left), y0 = Math.max(0, r.top);
  const x1 = Math.min(innerWidth, r.right), y1 = Math.min(innerHeight, r.bottom);
  if (x1 - x0 < 1 || y1 - y0 < 1) return { error: `${sel} has no size on screen: no shot taken` };
  return { x: x0 + scrollX, y: y0 + scrollY, width: x1 - x0, height: y1 - y0 };
}

// Two frames, so the hiding style and any scroll are drawn; a timer too, as a window behind
// another one may not draw frames.
const FRAME = `new Promise(r=>{let d=0;const f=()=>{if(!d){d=1;r(true)}};requestAnimationFrame(()=>requestAnimationFrame(f));setTimeout(f,250)})`;

export async function shot(send, { out, scheme, selector }) {
  const call = async (method, params, ms) => {
    const r = await send(method, params, ms);
    if (r.error) throw new Fail(`${method} failed: ${r.error.message || J(r.error)}`);
    return r.result || {};
  };
  const js = async (expression, ms) => {
    const r = await call("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true }, ms);
    if (r.exceptionDetails) throw new Fail(`The page refused a step: ${r.exceptionDetails.exception?.description || r.exceptionDetails.text}`);
    return r.result?.value;
  };
  // The build the app runs, read before anything on the page changes; "unknown" when it can't
  // be read (no Tauri on the page, an error, or no answer in 5 s).
  let build = "unknown";
  try { build = cleanBuild(await js(`(${pageBuild.toString()})()`, 5000)); } catch (e) { build = "unknown"; }
  let png;
  try {
    await call("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: scheme }] });
    await js(`(${hideFor.toString()})(${J(HIDE_ID)},${J(KEEP)},${J(HIDE)},${J(selector)})`);
    let clip;
    if (selector) {
      const box = await js(`(${findBox.toString()})(${J(selector)})`);
      if (!box || box.error) throw new Fail((box && box.error) || `Couldn't measure ${selector}`);
      clip = { ...box, scale: 1 };
    }
    await js(FRAME);
    const r = await call("Page.captureScreenshot", clip ? { format: "png", clip } : { format: "png" }, 30000);
    if (!r.data) throw new Fail("The capture came back empty");
    png = Buffer.from(r.data, "base64");
  } finally {
    // Put the page back as it was, each step on its own, so one failure doesn't skip the other.
    const left = [];
    try { await js(`(${unhide.toString()})(${J(HIDE_ID)},${J(KEEP)})`, 5000); }
    catch (e) { left.push(`the terminal and chat may still be hidden (${e.message}); reload the page`); }
    try { await call("Emulation.setEmulatedMedia", { media: "", features: [] }, 5000); }
    catch (e) { left.push(`the colour scheme may still be emulated (${e.message})`); }
    if (left.length) {
      const msg = `Couldn't put the page back: ${left.join("; ")}`;
      // Don't hide the capture's own failure behind the clean-up's.
      if (png) throw new Fail(`${msg}. No file written.`);
      console.error(msg);
    }
  }
  if (isBlankPng(png)) throw new Fail("blank shot: every pixel is the same colour, so no file was written");
  const file = withBuild(png, build);
  writeFileSync(out, file);
  return { bytes: file.length, build };
}

async function main(argv) {
  const [cmd, ...args] = argv;
  if (cmd === "shot") {
    if (!process.env.CDP_PORT) throw new Fail("shot needs CDP_PORT: 9334 for the test app");
    const opts = parseShotArgs(args);
    const cdp = await connect(process.env.CDP_PORT);
    try {
      const { bytes, build } = await shot(cdp.send, opts);
      console.log("saved", opts.out, `(${bytes} bytes${opts.selector ? `, cropped to ${opts.selector}` : ""})`);
      console.log("build", build);
    } finally {
      cdp.close();
    }
  } else if (cmd === "eval") {
    const cdp = await connect(process.env.CDP_PORT || 9333);
    try {
      const r = await cdp.send("Runtime.evaluate", { expression: args[0], awaitPromise: true, returnByValue: true }, 0);
      console.log(J(r.result?.result?.value ?? r.result?.exceptionDetails ?? r, null, 2));
    } finally {
      cdp.close();
    }
  } else {
    throw new Fail(USAGE);
  }
}

// Run as a script (not imported, as a test may do for isBlankPng).
const self = fileURLToPath(import.meta.url);
const ran = process.argv[1] ? resolve(process.argv[1]) : "";
if (process.platform === "win32" ? self.toLowerCase() === ran.toLowerCase() : self === ran) {
  main(process.argv.slice(2)).catch((e) => {
    console.error(e instanceof Fail ? e.message : (e && e.stack) || String(e));
    process.exitCode = 1;
  });
}
