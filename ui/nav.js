// Back and Forward between panels and views (card #187): the mouse's back and forward buttons
// (buttons 3 and 4), Alt+Left and Alt+Right, and the BrowserBack/BrowserForward keys.
// One history of what is on screen: the card panel (its card, its subagent tab, review or chat in
// the Human test review), the stack overlay (stacks.js), the session pane (router.js: the Router,
// a card, a subagent, a lane), the Agents drawer (agents-feed.js) and Settings. Every place that
// opens or closes one of them calls navNote(); the change is recorded once its click is done (a
// microtask), so "close this, open that" is one step. Back with nothing before does nothing.
// Leaving a panel or the session pane by Back or Forward keeps its unsent text, and coming back
// puts it in the box again (panel.js and router.js ask Nav.takeText when they make the box).
// The keys never act while Hugh types in a box or the terminal; the mouse buttons always do.
// Uses (when they are there) panel.js (panel, openPanel, closePanel, showView), review.js (htOpen,
// rvView, htView), stacks.js (stkDlg, stkKidOpen, stkDlgClose), router.js (sessionPaneShows,
// openSessionPane, closeSessionPane), agents-feed.js (agentsPane) and settings.js (settingsShows,
// openSettings, closeSettings).
const Nav=(()=>{
const MAX=100;
const def=n=>{try{return typeof globalThis[n]==="function"?globalThis[n]:null}catch(e){return null}};
const EMPTY={p:null,s:null,r:null,a:false,set:null};
let hist=[EMPTY],at=0,going=false,queued=false;
const texts=new Map(); // view key -> unsent text left by Back or Forward

// What is on screen now.
function snap(){
  let p=null,s=null;
  try{if(typeof panel!=="undefined"&&panel){
    let rv="";try{if(typeof htOpen==="function"&&htOpen())rv=typeof rvView!=="undefined"&&rvView==="chat"?"chat":"review"}catch(e){}
    p={id:panel.id,sub:panel.view||"",rv};
  }}catch(e){}
  try{if(typeof stkDlg!=="undefined"&&stkDlg)s={lead:stkDlg.lead,kid:stkDlg.kid}}catch(e){}
  const shows=def("sessionPaneShows"),r=shows?shows():null;
  const ap=globalThis.agentsPane,a=!!(ap&&typeof ap.isOpen==="function"&&ap.isOpen());
  const ss=def("settingsShows"),set=ss?ss():null;
  return{p,s,r:r?Object.assign({},r):null,a,set:set||null};
}
const paneKey=r=>r?[r.kind,r.cardId||"",r.id||"",r.key||"",r.kind==="card"&&!r.cardId?r.num||"":""].join("|"):"";
// Two views are the same when they show the same things (not the Settings section, not a pane's running flag).
function key(v){
  return JSON.stringify([v.p&&[v.p.id,v.p.sub,v.p.rv],v.s&&[v.s.lead,v.s.kid],paneKey(v.r),!!v.a,!!v.set]);
}

function record(){
  const v=snap();
  if(key(hist[at])===key(v)){hist[at]=v;return}
  hist=hist.slice(0,at+1);hist.push(v);
  if(hist.length>MAX)hist.shift();
  at=hist.length-1;
}
// Called after every open and close. Coalesced, and ignored while Back or Forward restores a view.
function note(){
  if(going||queued)return;
  queued=true;
  queueMicrotask(()=>{queued=false;if(!going)record()});
}
function flush(){if(queued){queued=false;record()}}

// Unsent text in the panel's and the pane's boxes, so Back and Forward never lose it.
function keepTexts(v){
  try{if(v.p&&typeof panel!=="undefined"&&panel){const ta=panel.el.querySelector("#p-text");if(ta&&ta.value)texts.set("p:"+v.p.id,ta.value)}}catch(e){}
  try{if(v.r){const ta=document.getElementById("r-text");if(ta&&ta.value)texts.set("r:"+paneKey(v.r),ta.value)}}catch(e){}
}
function takeText(k){const t=texts.get(k);texts.delete(k);return t||""}
function putTexts(){
  try{if(typeof panel!=="undefined"&&panel){const ta=panel.el.querySelector("#p-text");if(ta&&!ta.value){const t=takeText("p:"+panel.id);if(t)ta.value=t}}}catch(e){}
  try{const shows=def("sessionPaneShows"),r=shows?shows():null,ta=document.getElementById("r-text");if(r&&ta&&!ta.value){const t=takeText("r:"+paneKey(r));if(t)ta.value=t}}catch(e){}
}

// Shows view `t`: closes what it hasn't, then opens what it has, panel before overlay (a modal).
function apply(t){
  const c=snap(),call=(n,...a)=>{const f=def(n);if(f)f(...a)};
  if(c.set&&!t.set)call("closeSettings");
  // The overlay is modal: out of the way first, unless it stays over the same panel (it then steps in place).
  const samePanel=(c.p?c.p.id:null)===(t.p?t.p.id:null);
  if(c.s&&!(t.s&&samePanel))call("stkDlgClose");
  if(!t.p){if(c.p)call("closePanel")}
  else{
    if(!c.p||c.p.id!==t.p.id)call("openPanel",t.p.id);
    if(typeof panel!=="undefined"&&panel&&panel.id===t.p.id){
      if((panel.view||"")!==t.p.sub)call("showView",t.p.sub);
      try{if(t.p.rv&&typeof htOpen==="function"&&htOpen()&&rvView!==t.p.rv)call("htView",t.p.rv)}catch(e){}
    }
  }
  const now=snap();
  if(t.s){if(!now.s||now.s.lead!==t.s.lead||now.s.kid!==t.s.kid)call("stkKidOpen",t.s.lead,t.s.kid)}
  else if(now.s)call("stkDlgClose");
  if(paneKey(c.r)!==paneKey(t.r)){if(t.r)call("openSessionPane",t.r);else call("closeSessionPane")}
  const ap=globalThis.agentsPane;
  if(ap&&!!t.a!==!!c.a){try{t.a?ap.open():ap.close()}catch(e){}}
  if(t.set&&!c.set)call("openSettings",t.set);
}

// A view whose card (or stack) is gone or archived can't be shown: openPanel would only close
// what is open. board.js's cards and isArch.
function showable(v){
  let list=null;try{list=typeof cards!=="undefined"&&Array.isArray(cards)?cards:null}catch(e){}
  if(!list)return true;
  const ok=id=>{const c=list.find(x=>x.id===id);if(!c)return false;try{return!(typeof isArch==="function"&&isArch(c))}catch(e){return true}};
  return(!v.p||ok(v.p.id))&&(!v.s||ok(v.s.lead));
}
// One step back (d -1) or forward (d 1). A view that can't be shown any more (its card was
// archived, say) is skipped. False when there was nowhere to go.
function go(d){
  flush();
  const from=snap(),fk=key(from);
  let i=at+d;
  while(i>=0&&i<hist.length){
    if(!showable(hist[i])){hist.splice(i,1);if(i<=at)at--;if(d<0)i--;continue}
    keepTexts(from);
    going=true;
    try{apply(hist[i])}catch(e){console.error(e)}
    finally{going=false}
    const now=snap();
    if(key(now)!==fk){at=i;hist[at]=now;queued=false;putTexts();return true}
    hist.splice(i,1);if(i<=at)at--;   // nothing changed: drop that view and try the next one
    if(d<0)i--;
  }
  return false;
}
const back=()=>go(-1),forward=()=>go(1);

// The keys: not while Hugh types in a box or the terminal.
function typing(t){
  const a=t&&t.nodeType===1?t:document.activeElement;
  if(!a||a===document.body)return false;
  const tag=String(a.tagName||"").toLowerCase();
  if(tag==="input"||tag==="textarea"||tag==="select"||a.isContentEditable)return true;
  return !!(a.closest&&a.closest(".xterm,.term,[contenteditable]"));
}
function onKey(e){
  if(e.defaultPrevented||e.isComposing)return;
  let d=0;
  if(e.key==="BrowserBack")d=-1;else if(e.key==="BrowserForward")d=1;
  else if(e.altKey&&!e.ctrlKey&&!e.metaKey&&!e.shiftKey){if(e.key==="ArrowLeft")d=-1;else if(e.key==="ArrowRight")d=1}
  if(!d||typing(e.target))return;
  e.preventDefault();e.stopPropagation();
  if(!e.repeat)go(d);
}
// The mouse's back (3) and forward (4) buttons, wherever the pointer is. The default (the
// WebView's own page history) is stopped on the press and the release.
function onMouse(e){
  if(e.button!==3&&e.button!==4)return;
  e.preventDefault();e.stopPropagation();
  if(e.type==="mouseup")go(e.button===3?-1:1);
}
function install(w){
  w.addEventListener("keydown",onKey,true);
  w.addEventListener("mousedown",onMouse,true);
  w.addEventListener("mouseup",onMouse,true);
  w.addEventListener("auxclick",e=>{if(e.button===3||e.button===4){e.preventDefault();e.stopPropagation()}},true);
}
if(typeof window!=="undefined"&&typeof window.addEventListener==="function")install(window);

return{note,back,forward,go,snap,key,takeText,state:()=>({at,views:hist.map(v=>JSON.parse(JSON.stringify(v)))}),onKey,onMouse};
})();
function navNote(){Nav.note()}
