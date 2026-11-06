// A tiny DOM for the UI tests (card #120): just enough of Element, Text, document, a forgiving HTML
// parser and a selector engine (tag, #id, .class, [attr], [attr="v"], :scope, :not(...), the
// descendant and ">" combinators, comma lists) to run the board's own files in node's vm. No npm.
const VOID = new Set(["img", "input", "br", "hr", "meta", "link", "source", "area", "col", "wbr"]);
const ENT = { amp: "&", lt: "<", gt: ">", quot: '"', apos: "'", nbsp: " " };
const decode = (s) => s.replace(/&(#x[0-9a-f]+|#\d+|[a-z]+);/gi, (m, e) =>
  e[0] === "#" ? String.fromCodePoint(e[1] === "x" || e[1] === "X" ? parseInt(e.slice(2), 16) : +e.slice(1)) : ENT[e.toLowerCase()] ?? m);
const escText = (s) => String(s).replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
const escAttr = (s) => String(s).replace(/&/g, "&amp;").replace(/"/g, "&quot;");

class Node {
  constructor(doc) { this.ownerDocument = doc; this.parentNode = null; this.childNodes = []; this._ls = {}; }
  get parentElement() { return this.parentNode instanceof Element ? this.parentNode : null; }
  get firstChild() { return this.childNodes[0] || null; }
  get lastChild() { return this.childNodes[this.childNodes.length - 1] || null; }
  get nextSibling() { const p = this.parentNode; if (!p) return null; return p.childNodes[p.childNodes.indexOf(this) + 1] || null; }
  get previousSibling() { const p = this.parentNode; if (!p) return null; return p.childNodes[p.childNodes.indexOf(this) - 1] || null; }
  get isConnected() { let n = this; while (n.parentNode) n = n.parentNode; return n === this.ownerDocument; }
  _detach() { if (this.parentNode) { const a = this.parentNode.childNodes; a.splice(a.indexOf(this), 1); this.parentNode = null; } }
  remove() { this._detach(); }
  _nodes(xs) { const out = []; for (const x of xs) { if (x instanceof Fragment) out.push(...x.childNodes.slice()); else if (x instanceof Node) out.push(x); else out.push(this.ownerDocument.createTextNode(String(x))); } return out; }
  insertBefore(n, ref) {
    const list = n instanceof Fragment ? n.childNodes.slice() : [n];
    for (const x of list) { if (x === ref) continue; x._detach(); x.parentNode = this; const i = ref ? this.childNodes.indexOf(ref) : -1; if (i < 0) this.childNodes.push(x); else this.childNodes.splice(i, 0, x); }
    return n;
  }
  appendChild(n) { return this.insertBefore(n, null); }
  append(...xs) { for (const n of this._nodes(xs)) this.appendChild(n); }
  prepend(...xs) { const f = this.firstChild; for (const n of this._nodes(xs)) this.insertBefore(n, f); }
  before(...xs) { const p = this.parentNode; if (p) for (const n of this._nodes(xs)) p.insertBefore(n, this); }
  after(...xs) { const p = this.parentNode; if (!p) return; let ref = this.nextSibling; for (const n of this._nodes(xs)) p.insertBefore(n, ref); }
  contains(n) { while (n) { if (n === this) return true; n = n.parentNode; } return false; }
  addEventListener(t, f) { (this._ls[t] ||= []).push(f); }
  removeEventListener(t, f) { const a = this._ls[t]; if (a) { const i = a.indexOf(f); if (i >= 0) a.splice(i, 1); } }
  dispatchEvent(e) {
    e.target ||= this; let n = this;
    while (n) { e.currentTarget = n; for (const f of (n._ls[e.type] || []).slice()) f.call(n, e); if (n[`on${e.type}`] && n !== this.ownerDocument) n[`on${e.type}`](e); if (!e.bubbles || e._stop) break; n = n.parentNode; }
    return !e.defaultPrevented;
  }
  get textContent() { return this.childNodes.map((c) => c.textContent).join(""); }
  set textContent(v) { for (const c of this.childNodes) c.parentNode = null; this.childNodes = []; if (v !== "" && v != null) this.appendChild(this.ownerDocument.createTextNode(String(v))); }
}
class Text extends Node {
  constructor(doc, t) { super(doc); this.data = t; this.nodeType = 3; }
  get textContent() { return this.data; } set textContent(v) { this.data = String(v); }
  get outerHTML() { return escText(this.data); }
}
class Fragment extends Node { constructor(doc) { super(doc); this.nodeType = 11; } }
const elementsOf = (n) => n.childNodes.filter((c) => c instanceof Element);
class Element extends Node {
  constructor(doc, tag) {
    super(doc); this.tagName = tag.toUpperCase(); this.localName = tag.toLowerCase(); this.nodeType = 1; this.attrs = new Map(); this.style = {};
    const self = this;
    this.dataset = new Proxy({}, {
      get: (_, k) => (typeof k === "string" ? self.getAttribute("data-" + k.replace(/[A-Z]/g, (c) => "-" + c.toLowerCase())) ?? undefined : undefined),
      set: (_, k, v) => { self.setAttribute("data-" + k.replace(/[A-Z]/g, (c) => "-" + c.toLowerCase()), String(v)); return true; },
      deleteProperty: (_, k) => { self.removeAttribute("data-" + k.replace(/[A-Z]/g, (c) => "-" + c.toLowerCase())); return true; },
      has: (_, k) => self.hasAttribute("data-" + String(k).replace(/[A-Z]/g, (c) => "-" + c.toLowerCase())),
      ownKeys: () => [...self.attrs.keys()].filter((a) => a.startsWith("data-")).map((a) => a.slice(5).replace(/-([a-z])/g, (_, c) => c.toUpperCase())),
      getOwnPropertyDescriptor: (_, k) => (self.hasAttribute("data-" + String(k).replace(/[A-Z]/g, (c) => "-" + c.toLowerCase())) ? { enumerable: true, configurable: true, value: self.dataset[k] } : undefined),
    });
    if (this.localName === "template") this.content = this;
  }
  get children() { return elementsOf(this); }
  get firstElementChild() { return this.children[0] || null; }
  get lastElementChild() { const c = this.children; return c[c.length - 1] || null; }
  get nextElementSibling() { let n = this.nextSibling; while (n && !(n instanceof Element)) n = n.nextSibling; return n; }
  get previousElementSibling() { let n = this.previousSibling; while (n && !(n instanceof Element)) n = n.previousSibling; return n; }
  getAttribute(k) { return this.attrs.has(k) ? this.attrs.get(k) : null; }
  setAttribute(k, v) { this.attrs.set(String(k).toLowerCase(), String(v)); }
  removeAttribute(k) { this.attrs.delete(k); }
  hasAttribute(k) { return this.attrs.has(k); }
  toggleAttribute(k, on) { on = on === undefined ? !this.hasAttribute(k) : on; if (on) this.setAttribute(k, ""); else this.removeAttribute(k); return on; }
  get id() { return this.getAttribute("id") || ""; } set id(v) { this.setAttribute("id", v); }
  get className() { return this.getAttribute("class") || ""; } set className(v) { this.setAttribute("class", v); }
  get title() { return this.getAttribute("title") || ""; } set title(v) { this.setAttribute("title", v); }
  get hidden() { return this.hasAttribute("hidden"); } set hidden(v) { this.toggleAttribute("hidden", !!v); }
  get src() { return this.getAttribute("src") || ""; } set src(v) { this.setAttribute("src", v); }
  get alt() { return this.getAttribute("alt") || ""; } set alt(v) { this.setAttribute("alt", v); }
  get type() { return this.getAttribute("type") || ""; } set type(v) { this.setAttribute("type", v); }
  get href() { return this.getAttribute("href") || ""; } set href(v) { this.setAttribute("href", v); }
  get rel() { return this.getAttribute("rel") || ""; } set rel(v) { this.setAttribute("rel", v); }
  get disabled() { return this.hasAttribute("disabled"); } set disabled(v) { this.toggleAttribute("disabled", !!v); }
  get open() { return this.hasAttribute("open"); } set open(v) { this.toggleAttribute("open", !!v); }
  get draggable() { return this.getAttribute("draggable") === "true"; } set draggable(v) { this.setAttribute("draggable", v ? "true" : "false"); }
  get tabIndex() { return +(this.getAttribute("tabindex") || -1); } set tabIndex(v) { this.setAttribute("tabindex", v); }
  get value() { return this._value ?? this.getAttribute("value") ?? ""; } set value(v) { this._value = String(v); }
  get classList() {
    const el = this, get = () => el.className.split(/\s+/).filter(Boolean), put = (a) => { el.className = a.join(" "); };
    return {
      contains: (c) => get().includes(c),
      add: (...cs) => { const a = get(); for (const c of cs) if (!a.includes(c)) a.push(c); put(a); },
      remove: (...cs) => put(get().filter((c) => !cs.includes(c))),
      toggle: (c, on) => { const has = get().includes(c); on = on === undefined ? !has : !!on; if (on && !has) put([...get(), c]); if (!on && has) put(get().filter((x) => x !== c)); return on; },
      get length() { return get().length; },
    };
  }
  get innerHTML() { return this.childNodes.map((c) => c.outerHTML).join(""); }
  set innerHTML(h) { this.textContent = ""; for (const n of parseHTML(this.ownerDocument, h)) this.appendChild(n); }
  get outerHTML() {
    const a = [...this.attrs].map(([k, v]) => (v === "" && k !== "value" ? ` ${k}` : ` ${k}="${escAttr(v)}"`)).join("");
    return VOID.has(this.localName) ? `<${this.localName}${a}>` : `<${this.localName}${a}>${this.innerHTML}</${this.localName}>`;
  }
  set outerHTML(h) { const p = this.parentNode; if (!p) return; for (const n of parseHTML(this.ownerDocument, h)) p.insertBefore(n, this); this.remove(); }
  insertAdjacentHTML(where, h) {
    const ns = parseHTML(this.ownerDocument, h);
    if (where === "beforebegin") this.before(...ns); else if (where === "afterend") this.after(...ns);
    else if (where === "afterbegin") this.prepend(...ns); else this.append(...ns);
  }
  querySelectorAll(sel) { const out = []; const walk = (n) => { for (const c of elementsOf(n)) { if (matches(c, sel, this)) out.push(c); walk(c); } }; walk(this); return out; }
  querySelector(sel) { return this.querySelectorAll(sel)[0] || null; }
  matches(sel) { return matches(this, sel, null); }
  closest(sel) { let n = this; while (n instanceof Element) { if (matches(n, sel, null)) return n; n = n.parentNode; } return null; }
  focus() { this.ownerDocument.activeElement = this; }
  blur() {}
  click() { const e = new Event("click", { bubbles: true }); this.dispatchEvent(e); }
  getBoundingClientRect() { return { top: 0, left: 0, width: 0, height: 0, bottom: 0, right: 0 }; }
  scrollIntoView() {}
  showModal() { this.open = true; } close() { this.open = false; }
}
class Document extends Node {
  constructor() {
    super(null); this.ownerDocument = this; this.nodeType = 9;
    this.documentElement = new Element(this, "html"); this.appendChild(this.documentElement);
    this.head = new Element(this, "head"); this.body = new Element(this, "body");
    this.documentElement.append(this.head, this.body); this.activeElement = this.body; this.title = "";
  }
  createElement(t) { return new Element(this, t); }
  createTextNode(t) { return new Text(this, t); }
  createDocumentFragment() { return new Fragment(this); }
  createRange() { return { selectNodeContents() {} }; }
  getElementById(id) { return this.documentElement.querySelector("#" + id) || (this.documentElement.id === id ? this.documentElement : null); }
  querySelectorAll(s) { return [this.documentElement, ...this.documentElement.querySelectorAll(s)].filter((e, i) => i > 0 || matches(e, s, null)); }
  querySelector(s) { return this.querySelectorAll(s)[0] || null; }
}
export class Event {
  constructor(type, o = {}) { this.type = type; this.bubbles = !!o.bubbles; this.defaultPrevented = false; this.target = null; }
  preventDefault() { this.defaultPrevented = true; } stopPropagation() { this._stop = true; } stopImmediatePropagation() { this._stop = true; }
}

// ---- HTML ----
function parseHTML(doc, html) {
  const root = new Fragment(doc); const stack = [root]; let i = 0; const s = String(html ?? "");
  const top = () => stack[stack.length - 1];
  while (i < s.length) {
    if (s.startsWith("<!--", i)) { const e = s.indexOf("-->", i); i = e < 0 ? s.length : e + 3; continue; }
    if (s[i] === "<" && s[i + 1] === "/") {
      const e = s.indexOf(">", i); const name = s.slice(i + 2, e).trim().toLowerCase(); i = e + 1;
      for (let k = stack.length - 1; k > 0; k--) if (stack[k].localName === name) { stack.length = k; break; }
      continue;
    }
    if (s[i] === "<" && /[a-zA-Z!]/.test(s[i + 1] || "")) {
      if (s[i + 1] === "!") { i = s.indexOf(">", i) + 1; continue; }
      const m = /^<([a-zA-Z][\w-]*)/.exec(s.slice(i)); const el = doc.createElement(m[1].toLowerCase()); i += m[0].length;
      let self = false;
      for (;;) {
        const w = /^\s*/.exec(s.slice(i)); i += w[0].length;
        if (s[i] === ">") { i++; break; }
        if (s.startsWith("/>", i)) { i += 2; self = true; break; }
        if (i >= s.length) break;
        const a = /^([^\s=>\/]+)(?:\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+)))?/.exec(s.slice(i));
        if (!a) { i++; continue; }
        i += a[0].length; el.setAttribute(a[1], decode(a[2] ?? a[3] ?? a[4] ?? ""));
      }
      top().appendChild(el);
      if (!self && !VOID.has(el.localName)) stack.push(el);
      continue;
    }
    const e = s.indexOf("<", i + 1); const end = e < 0 ? s.length : e;
    top().appendChild(doc.createTextNode(decode(s.slice(i, end)))); i = end;
  }
  return root.childNodes.slice();
}

// ---- selectors ----
function splitTop(sel, ch) { const out = []; let d = 0, q = null, cur = ""; for (const c of sel) { if (q) { if (c === q) q = null; } else if (c === '"' || c === "'") q = c; else if (c === "(" || c === "[") d++; else if (c === ")" || c === "]") d--; if (c === ch && !d && !q) { out.push(cur); cur = ""; } else cur += c; } out.push(cur); return out; }
function parseCompound(t) {
  const parts = []; let i = 0;
  while (i < t.length) {
    let m;
    if ((m = /^:scope/.exec(t.slice(i)))) parts.push({ k: "scope" });
    else if ((m = /^:not\(/.exec(t.slice(i)))) { let d = 1, j = i + 5; while (j < t.length && d) { if (t[j] === "(") d++; if (t[j] === ")") d--; j++; } parts.push({ k: "not", v: t.slice(i + 5, j - 1) }); i = j; continue; }
    else if ((m = /^:[\w-]+/.exec(t.slice(i)))) parts.push({ k: "pseudo", v: m[0] });
    else if ((m = /^#([\w-]+)/.exec(t.slice(i)))) parts.push({ k: "id", v: m[1] });
    else if ((m = /^\.([\w-]+)/.exec(t.slice(i)))) parts.push({ k: "class", v: m[1] });
    else if ((m = /^\[\s*([\w-]+)\s*(?:([~^$*|]?=)\s*(?:"([^"]*)"|'([^']*)'|([^\]\s]*)))?\s*\]/.exec(t.slice(i)))) parts.push({ k: "attr", n: m[1].toLowerCase(), op: m[2], v: m[3] ?? m[4] ?? m[5] });
    else if ((m = /^\*/.exec(t.slice(i)))) parts.push({ k: "any" });
    else if ((m = /^[\w-]+/.exec(t.slice(i)))) parts.push({ k: "tag", v: m[0].toLowerCase() });
    else throw new Error("selector not supported: " + t);
    i += m[0].length;
  }
  return parts;
}
function matchCompound(el, parts, scope) {
  for (const p of parts) {
    if (p.k === "scope" && el !== scope) return false;
    if (p.k === "tag" && el.localName !== p.v) return false;
    if (p.k === "id" && el.id !== p.v) return false;
    if (p.k === "class" && !el.classList.contains(p.v)) return false;
    if (p.k === "attr") { const v = el.getAttribute(p.n); if (v === null) return false; if (p.op === "=" && v !== p.v) return false; if (p.op === "^=" && !v.startsWith(p.v)) return false; if (p.op === "*=" && !v.includes(p.v)) return false; }
    if (p.k === "not" && matches(el, p.v, scope)) return false;
    if (p.k === "pseudo" && p.v !== ":hover") return false;
  }
  return true;
}
function matches(el, sel, scope) {
  return splitTop(sel, ",").some((one) => {
    const toks = splitTop(one.trim().replace(/\s*>\s*/g, " > ").replace(/\s+/g, " "), " ").filter(Boolean);
    const seq = []; let comb = " ";
    for (const t of toks) { if (t === ">") { comb = ">"; continue; } seq.push({ comb, parts: parseCompound(t) }); comb = " "; }
    const go = (e, k) => {
      if (!matchCompound(e, seq[k].parts, scope)) return false;
      if (k === 0) return true;
      if (seq[k].comb === ">") { const p = e.parentNode; return p instanceof Element && go(p, k - 1); }
      for (let p = e.parentNode; p instanceof Element; p = p.parentNode) if (go(p, k - 1)) return true;
      return false;
    };
    return go(el, seq.length - 1);
  });
}

export function makeDocument(bodyHtml) { const d = new Document(); if (bodyHtml) d.body.innerHTML = bodyHtml; return d; }
export { Element, Text, Document };
