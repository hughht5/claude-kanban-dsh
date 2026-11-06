// Drives the app's WebView over the Chrome DevTools Protocol, for checking the
// UI from a script. Start the app with
//   WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9333 --remote-allow-origins=http://127.0.0.1:9333
// (set CDP_PORT to use another port, e.g. 9334 for the test version)
// then: node tools/cdp.mjs eval "<expression>"   or   node tools/cdp.mjs shot <out.png>
import { writeFileSync } from "node:fs";

const [, , cmd, arg] = process.argv;
const PORT = process.env.CDP_PORT || 9333;
const pages = await (await fetch(`http://127.0.0.1:${PORT}/json`)).json();
const page = pages.find((p) => p.type === "page");
const ws = new WebSocket(page.webSocketDebuggerUrl, { headers: { Origin: `http://127.0.0.1:${PORT}` } });
let id = 0;
const pending = new Map();
ws.onmessage = (m) => {
  const msg = JSON.parse(m.data);
  if (msg.id && pending.has(msg.id)) {
    pending.get(msg.id)(msg);
    pending.delete(msg.id);
  }
};
await new Promise((r) => (ws.onopen = r));
const send = (method, params = {}) =>
  new Promise((r) => {
    const n = ++id;
    pending.set(n, r);
    ws.send(JSON.stringify({ id: n, method, params }));
  });

if (cmd === "eval") {
  const r = await send("Runtime.evaluate", { expression: arg, awaitPromise: true, returnByValue: true });
  console.log(JSON.stringify(r.result?.result?.value ?? r.result?.exceptionDetails ?? r, null, 2));
} else if (cmd === "shot") {
  const scheme = process.argv[4] || "light";
  await send("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: scheme }] });
  const r = await send("Page.captureScreenshot", { format: "png" });
  writeFileSync(arg, Buffer.from(r.result.data, "base64"));
  console.log("saved", arg);
}
ws.close();
