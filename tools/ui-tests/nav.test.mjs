// Back and Forward between panels and views (card #187): ui/nav.js in a vm, with a stand-in for
// panel.js, review.js, stacks.js, router.js, agents-feed.js and settings.js that keeps the same
// state names (panel, stkDlg, rvView, htOpen) and calls navNote() where they do.
import test from "node:test";
import assert from "node:assert/strict";
import { boot } from "./harness.mjs";

const tick = () => new Promise((r) => setTimeout(r, 0));

// The app's openers, as small as the history needs them. Each counts its calls in `calls`.
const APP = `
var calls=[];
var cards=[{id:"lead"},{id:"kid"},{id:"ht1"},{id:"ht2"},{id:"a"},{id:"b"}];
var HT=new Set(["ht1","ht2"]);   // cards in the Human test review queue
var panel=null,stkDlg=null,rvView="review",paneSrc=null,agentsOpen=false,settingsAt=null;
function mkPanel(id){const ta=document.createElement("textarea");ta.id="p-text";const el=document.createElement("section");el.append(ta);return{id,view:"",el}}
function openPanel(id){calls.push("openPanel:"+id);if(panel&&panel.id===id)return;closePanel(true);if(!cards.some(c=>c.id===id))return;panel=mkPanel(id);rvView="review";
  const t=Nav.takeText("p:"+id);if(t)panel.el.querySelector("#p-text").value=t;navNote()}
function closePanel(step){calls.push("closePanel");if(!panel)return;panel=null;navNote()}
function showView(id){calls.push("showView:"+id);if(!panel||panel.view===id)return;panel.view=id;navNote()}
function htOpen(){return panel&&HT.has(panel.id)?{id:panel.id}:null}
function htView(v){calls.push("htView:"+v);if(!htOpen())return;rvView=v==="chat"?"chat":"review";navNote()}
function stkKidOpen(lead,kid){calls.push("stkKidOpen:"+lead+">"+kid);stkDlg={lead,kid};navNote()}
function stkDlgClose(){calls.push("stkDlgClose");if(stkDlg){stkDlg=null;navNote()}}
window.sessionPaneShows=()=>paneSrc?Object.assign({},paneSrc):null;
window.openSessionPane=src=>{calls.push("openPane:"+src.kind);paneSrc=Object.assign({},src);navNote()};
window.closeSessionPane=()=>{calls.push("closePane");if(paneSrc){paneSrc=null;navNote()}};
window.agentsPane={isOpen:()=>agentsOpen,open:()=>{calls.push("agentsOpen");agentsOpen=true;navNote()},close:()=>{calls.push("agentsClose");agentsOpen=false;navNote()}};
window.settingsShows=()=>settingsAt;
window.openSettings=w=>{calls.push("openSettings");settingsAt=w||"columns";navNote()};
window.closeSettings=()=>{calls.push("closeSettings");settingsAt=null;navNote()};
`;

function app() {
  const b = boot({ files: ["nav.js"] });
  b.run(APP);
  const at = () => b.run(`JSON.stringify({p:panel&&[panel.id,panel.view,htOpen()?rvView:""],s:stkDlg&&[stkDlg.lead,stkDlg.kid],r:paneSrc&&paneSrc.kind,a:agentsOpen,set:settingsAt})`);
  const view = (o) => JSON.stringify(Object.assign({ p: null, s: null, r: null, a: false, set: null }, o));
  return { ...b, at, view, back: () => b.run("Nav.back()"), forward: () => b.run("Nav.forward()") };
}

test("back with an empty history does nothing", async () => {
  const b = app();
  assert.equal(b.back(), false);
  assert.equal(b.forward(), false);
  assert.equal(b.run("calls.length"), 0, "nothing was opened or closed");
  assert.equal(b.at(), b.view({}));
});

test("stack: lead's panel, then a child in the overlay, Back returns to the lead", async () => {
  const b = app();
  b.run(`openPanel("lead")`); await tick();
  b.run(`stkKidOpen("lead","kid")`); await tick();
  assert.equal(b.at(), b.view({ s: ["lead", "kid"], p: ["lead", "", ""] }));
  assert.equal(b.back(), true);
  assert.equal(b.at(), b.view({ p: ["lead", "", ""] }), "the overlay closed; the lead's panel stays");
  assert.equal(b.forward(), true);
  assert.equal(b.at(), b.view({ s: ["lead", "kid"], p: ["lead", "", ""] }));
});

test("stack: lead's panel, then the child's own panel (Open the card), Back returns to the lead", async () => {
  const b = app();
  b.run(`openPanel("lead")`); await tick();
  b.run(`stkKidOpen("lead","kid")`); await tick();
  b.run(`stkDlgClose();openPanel("kid")`); await tick();   // the overlay's "Open the card": one step
  assert.equal(b.at(), b.view({ p: ["kid", "", ""] }));
  assert.equal(b.back(), true);
  assert.equal(b.at(), b.view({ p: ["lead", "", ""], s: ["lead", "kid"] }), "back to the child's row in the lead's overlay");
  assert.equal(b.back(), true);
  assert.equal(b.at(), b.view({ p: ["lead", "", ""] }));
  assert.equal(b.back(), true);
  assert.equal(b.at(), b.view({}), "back to the board");
  assert.equal(b.back(), false, "nothing before the board");
});

test("testing view: the review, then the card's chat, Back returns to the review", async () => {
  const b = app();
  b.run(`openPanel("ht1")`); await tick();
  assert.equal(b.at(), b.view({ p: ["ht1", "", "review"] }));
  b.run(`htView("chat")`); await tick();   // Open the card
  assert.equal(b.at(), b.view({ p: ["ht1", "", "chat"] }));
  assert.equal(b.back(), true);
  assert.equal(b.at(), b.view({ p: ["ht1", "", "review"] }));
  // Stepping through the queue is a step each.
  b.run(`closePanel(true);openPanel("ht2")`); await tick();
  assert.equal(b.at(), b.view({ p: ["ht2", "", "review"] }));
  assert.equal(b.back(), true);
  assert.equal(b.at(), b.view({ p: ["ht1", "", "review"] }));
});

test("forward: after Back, Forward goes one step on; a new open drops what was ahead", async () => {
  const b = app();
  b.run(`openPanel("a")`); await tick();
  b.run(`openPanel("b")`); await tick();
  b.run(`showView("sub1")`); await tick();   // a subagent's tab
  b.back(); b.back();
  assert.equal(b.at(), b.view({ p: ["a", "", ""] }));
  assert.equal(b.forward(), true);
  assert.equal(b.at(), b.view({ p: ["b", "", ""] }));
  assert.equal(b.forward(), true);
  assert.equal(b.at(), b.view({ p: ["b", "sub1", ""] }));
  assert.equal(b.forward(), false, "nothing ahead");
  b.back(); b.back();
  b.run(`window.openSessionPane({kind:"router"})`); await tick();
  assert.equal(b.at(), b.view({ p: ["a", "", ""], r: "router" }));
  assert.equal(b.forward(), false, "a new view drops the forward steps");
  assert.equal(b.back(), true);
  assert.equal(b.at(), b.view({ p: ["a", "", ""] }), "the Router pane closed, the panel stays");
});

test("the session pane, the Agents drawer and Settings are steps too", async () => {
  const b = app();
  b.run(`agentsPane.open()`); await tick();
  b.run(`window.openSessionPane({kind:"sub",cardId:"a",id:"s1"})`); await tick();
  b.run(`window.openSettings("sessions")`); await tick();
  b.back();
  assert.equal(b.at(), b.view({ a: true, r: "sub" }));
  b.back();
  assert.equal(b.at(), b.view({ a: true }));
  b.back();
  assert.equal(b.at(), b.view({}));
  b.forward(); b.forward(); b.forward();
  assert.equal(b.at(), b.view({ a: true, r: "sub", set: "sessions" }));
});

test("a view that can't be shown any more is skipped", async () => {
  const b = app();
  b.run(`openPanel("a")`); await tick();
  b.run(`openPanel("b")`); await tick();
  b.run(`openPanel("lead")`); await tick();
  b.run(`cards=cards.filter(c=>c.id!=="b")`);   // b was archived
  assert.equal(b.back(), true);
  assert.equal(b.at(), b.view({ p: ["a", "", ""] }), "straight back to a");
});

test("Back keeps the unsent text, and coming back puts it in the box again", async () => {
  const b = app();
  b.run(`openPanel("a")`); await tick();
  b.run(`openPanel("b")`); await tick();
  b.run(`panel.el.querySelector("#p-text").value="half a message"`);
  b.back();
  assert.equal(b.at(), b.view({ p: ["a", "", ""] }));
  b.forward();
  assert.equal(b.run(`panel.el.querySelector("#p-text").value`), "half a message");
});

test("the keys: Alt+Left and Alt+Right, not while typing; the mouse buttons always", async () => {
  const b = app();
  b.run(`openPanel("a")`); await tick();
  b.run(`openPanel("b")`); await tick();
  const ev = (o) => b.run(`(()=>{const e=Object.assign({prevented:false,preventDefault(){this.prevented=true},stopPropagation(){},defaultPrevented:false,isComposing:false,repeat:false,altKey:false,ctrlKey:false,metaKey:false,shiftKey:false,target:document.body},${JSON.stringify(o)});return e})()`);
  // Typing in a box: Alt+Left is the box's (word moves are Ctrl, but leave it alone anyway).
  b.run(`globalThis.__ta=document.createElement("textarea");document.body.append(__ta)`);
  b.run(`Nav.onKey({key:"ArrowLeft",altKey:true,ctrlKey:false,metaKey:false,shiftKey:false,repeat:false,isComposing:false,defaultPrevented:false,target:__ta,preventDefault(){},stopPropagation(){}})`);
  assert.equal(b.at(), b.view({ p: ["b", "", ""] }), "no step while typing");
  // The mouse's back button: even with the focus in a box.
  b.run(`Nav.onMouse({type:"mousedown",button:3,target:__ta,preventDefault(){},stopPropagation(){}})`);
  assert.equal(b.at(), b.view({ p: ["b", "", ""] }), "the press only stops the WebView's own back");
  b.run(`Nav.onMouse({type:"mouseup",button:3,target:__ta,preventDefault(){},stopPropagation(){}})`);
  assert.equal(b.at(), b.view({ p: ["a", "", ""] }));
  b.run(`Nav.onMouse({type:"mouseup",button:4,target:__ta,preventDefault(){},stopPropagation(){}})`);
  assert.equal(b.at(), b.view({ p: ["b", "", ""] }));
  // Alt+Left on the board goes back; plain Left and Ctrl+Alt+Left don't.
  const k = ev({ key: "ArrowLeft", altKey: true });
  b.ctx.__k = k; b.run(`Nav.onKey(__k)`);
  assert.equal(k.prevented, true);
  assert.equal(b.at(), b.view({ p: ["a", "", ""] }));
  b.ctx.__k = ev({ key: "ArrowRight" }); b.run(`Nav.onKey(__k)`);
  b.ctx.__k = ev({ key: "ArrowRight", altKey: true, ctrlKey: true }); b.run(`Nav.onKey(__k)`);
  assert.equal(b.at(), b.view({ p: ["a", "", ""] }));
  b.ctx.__k = ev({ key: "ArrowRight", altKey: true }); b.run(`Nav.onKey(__k)`);
  assert.equal(b.at(), b.view({ p: ["b", "", ""] }));
  // Another button (the middle one) is left alone.
  let stopped = false;
  b.ctx.__m = { type: "mouseup", button: 1, preventDefault() { stopped = true; }, stopPropagation() {} };
  b.run(`Nav.onMouse(__m)`);
  assert.equal(stopped, false);
});

test("several opens and closes in one click are one step", async () => {
  const b = app();
  b.run(`openPanel("a")`); await tick();
  b.run(`closePanel();window.openSessionPane({kind:"router"});openPanel("b")`); await tick();
  assert.equal(b.run("Nav.state().views.length"), 3, "the board, a, then b with the Router");
  b.back();
  assert.equal(b.at(), b.view({ p: ["a", "", ""] }));
});
