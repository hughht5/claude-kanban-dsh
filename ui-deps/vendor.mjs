// Copies the panel's browser builds out of node_modules, so the app loads them
// from its own files with no bundler. Run after `npm ci`, in this folder. The packages live
// outside ui/ because the app embeds ui/ whole, and `cargo tauri build` refuses a node_modules in it.
import { copyFileSync, mkdirSync } from "node:fs";
const files = {
  "@xterm/xterm/lib/xterm.js": "xterm.js",
  "@xterm/xterm/css/xterm.css": "xterm.css",
  "@xterm/addon-fit/lib/addon-fit.js": "addon-fit.js",
  "marked/marked.min.js": "marked.min.js",
  "dompurify/dist/purify.min.js": "purify.min.js",
};
mkdirSync("../ui/vendor", { recursive: true });
for (const [from, to] of Object.entries(files)) copyFileSync(`node_modules/${from}`, `../ui/vendor/${to}`);
console.log("vendored", Object.keys(files).length, "files");
