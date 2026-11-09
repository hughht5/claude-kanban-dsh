// The Router (card #119): one long-running Claude Code session the app owns for the whole board.
// It is not a card: in the app's session map its key is "router", so the per-session commands
// (session_input, session_resize, session_snapshot, session_answer, transcript_read) take
// cardId "router". Its own commands: router_status, router_start, router_stop, router_send,
// router_feed, router_opened, and the "router-changed" event.
//
// A "Router" button in the header (with a state dot) opens a panel docked on the right of the
// board: the digest of what happened while it was closed, the live log, then the Router's Chat
// and Terminal. The board narrows to make room, and a card panel opens to its left.
//
// The session pane (card #129): the same panel opens any running session, the way it opens the
// Router (window.openSessionPane(src)). One at a time: opening another swaps what it shows. src:
//   {kind:"router"}                            the Router, as above
//   {kind:"card", cardId, num, title}          a card session in the app: its chat takes messages
//                                              (session_send) and answers, and it has a Terminal
//   {kind:"lane", key, repo}                   a merge lane (the merger, cards #160, #162): read only
//   {kind:"sub", cardId, num, session, id, type, desc, running}
//                                              a subagent's own conversation (subagent_read), read only
//   {kind:"outside", id, cwd, title, account, cards}
//                                              a Claude Code session outside the app that used this
//                                              board (outside_transcript_read), read only
// Each has its live log beside its chat (the Agents feed, narrowed to its cards or agent).
// Uses board.js's $, esc, invoke, errText, cards, cardNum, render, boardHooks and panel.js's
// md, stamp, groups, showShots, shotImg, attachedImages, userHtml, openShot, askAnswers, askAuto,
// askReady, b64bytes, panel and openPanel.
(()=>{
const RID="router";
const W_KEY="router.width",W_OLD=380,W_MIN=300;
// The default width: 720 px, or 55% of the window if that is less (card #128: log and chat side by side).
const defW=()=>Math.round(Math.min(720,innerWidth*0.55));
const STATE_TEXT={off:"Off",starting:"Starting",idle:"Ready",working:"Working",asks:"Asks you",down:"Down"};
const store=(k,v)=>{try{if(v===undefined)return localStorage.getItem(k);localStorage.setItem(k,v)}catch(e){return null}};
const unstore=k=>{try{localStorage.removeItem(k)}catch(e){}};
const ev=()=>window.__TAURI__.event;

let st={on:false,live:false,state:"off",model:"",effort:"",mayStart:false,pending:0,startedAt:null};
let stErr="";
let pane=null; // the open panel: {el, src, cst, tab, entries, tools, offset, open, more, ask, timer, term, fit, feed, digest, feedOpen, unlisten}
let pillDone=false,fallbackPill=null;

// ---------- what the pane shows (card #129) ----------
const kind=()=>pane?pane.src.kind:"router";
const isRouter=()=>kind()==="router";
// The session's key in the app's session map: the Router's, a lane's or the card's id.
const key=()=>({router:RID,card:pane&&pane.src.cardId,lane:pane&&pane.src.key}[kind()]||null);
// Types into it: the Router and a card session. The rest are read only.
const writable=()=>isRouter()||kind()==="card";
const hasTerm=()=>isRouter()||kind()==="card";
// The state the pane draws: the Router's, else the card session's (session_info), else none.
function cur(){
  if(isRouter())return{live:st.live,state:st.state};
  if(kind()==="card")return pane.cst||{live:false,state:"off"};
  if(kind()==="lane"){const s=pane.src.state||"off";return{live:s!=="off",state:STATE_TEXT[s]?s:s==="permission"?"asks":"idle"}}
  return{live:false,state:"off"};
}
const curState=()=>STATE_TEXT[cur().state]?cur().state:"off";
const sameSrc=(a,b)=>a.kind===b.kind&&(a.cardId||"")===(b.cardId||"")&&(a.id||"")===(b.id||"")&&(a.key||"")===(b.key||"");
// A card session's state as the Router's words: permission is "asks".
const cardWord=i=>!i||!i.running?"off":i.state==="permission"?"asks":STATE_TEXT[i.state]?i.state:i.state==="failed"?"down":"starting";
const shortPath=p=>{const s=String(p||"").replace(/[\\/]+$/,"");const parts=s.split(/[\\/]/);return parts.length>2?"…\\"+parts.slice(-2).join("\\"):s};

// ---------- status and the header button ----------
async function refreshStatus(){
  try{st=Object.assign({},st,await invoke("router_status"));stErr=""}
  catch(e){stErr=errText(e)}
  drawButton();if(isRouter()){drawHead();drawCompose()}drawPillFallback();
  if(typeof window.fbDraw==="function")try{window.fbDraw()}catch(e){}
}
const dotState=()=>STATE_TEXT[st.state]?st.state:"off";
const neverStarted=()=>dotState()==="off"&&!st.on&&!st.startedAt;
function drawButton(){
  const b=$("#routerBtn");if(!b)return;
  const s=dotState(),open=!!pane&&isRouter();
  b.innerHTML=`<span class="rdot r-${s}" aria-hidden="true"></span>${neverStarted()?"Start Router":"Router"}`;
  b.setAttribute("aria-pressed",open?"true":"false");
  b.title=stErr?`Router: can't read its state (${stErr})`:neverStarted()?"Start the Router: one session that watches the whole board":`Router: ${STATE_TEXT[s]}. Click to ${open?"close":"open"} its panel.`;
}
async function onButton(){
  if(pane&&isRouter()){closePane();return}
  if(neverStarted()){
    const b=$("#routerBtn");if(b)b.disabled=true;
    try{st=Object.assign({},st,await invoke("router_start"));stErr=""}catch(e){stErr=errText(e);if(typeof status==="function")status("Couldn't start the Router: "+stErr,true)}
    if(b)b.disabled=false;drawButton();
  }
  openPane({kind:"router"});
}

// ---------- "Router asks": the filter bar's pill (card #120) or our own ----------
const openRouter=()=>openPane({kind:"router"});
function setupPill(){
  if(pillDone)return;
  if(typeof window.registerPill==="function"){
    pillDone=true;
    // Third in the row (order 30), in the Router's own tone, redrawn when its state changes (final-test F3).
    try{window.registerPill({key:"router-asks",label:"Router asks",tone:"router",order:30,term:"is:router-asks",deps:()=>st.state,count:()=>(st.state==="asks"?1:0),onClick:openRouter})}catch(e){console.error("router pill",e)}
    return;
  }
  drawPillFallback();
}
function drawPillFallback(){
  if(pillDone)return;
  const b=$("#routerBtn");if(!b)return;
  if(!fallbackPill){fallbackPill=document.createElement("button");fallbackPill.type="button";fallbackPill.className="rpill";fallbackPill.textContent="Router asks";
    fallbackPill.title="The Router is asking you something. Click to open its panel.";fallbackPill.onclick=openRouter;b.after(fallbackPill)}
  fallbackPill.hidden=st.state!=="asks";
}
// The pill's term in the filter box: every card still matches, and the term's arrival opens the panel.
let askTermSeen=false,askTermWas=false;
if(typeof boardHooks==="object"){
  boardHooks.filters.push((c,t)=>{if(t!=="is:router-asks")return null;askTermSeen=true;return true});
  boardHooks.rendered.push(()=>{if(askTermSeen&&!askTermWas&&!(pane&&isRouter()))setTimeout(openRouter,0);askTermWas=askTermSeen;askTermSeen=false});
}

// ---------- the panel ----------
function width(){
  // A saved 380 is the old default (the old double-click wrote it): it counts as unset.
  let w=parseFloat(store(W_KEY)||"");if(!w||w===W_OLD)w=defW();
  return Math.round(Math.max(W_MIN,Math.min(w,Math.max(W_MIN,innerWidth-360))));
}
function applyWidth(w){
  document.documentElement.style.setProperty("--rpane-w",w+"px");
  placeTop();fitCardPanel();
}
// Full height under the header.
function placeTop(){
  if(!pane)return;const h=document.querySelector("header");
  const top=h?Math.round(h.getBoundingClientRect().bottom+8):16;
  pane.el.style.top=top+"px";document.documentElement.style.setProperty("--rpane-top",top+"px");
}
// The header's title and its buttons, by what the pane shows.
function headHtml(src){
  const x=`<button class="ibtn x" id="r-close" type="button" aria-label="Close the ${src.kind==="router"?"Router":"session"} panel">✕</button>`;
  if(src.kind==="router")return`<span class="rdot" id="r-dot"></span><b class="rpane-ttl">Router</b><span class="rpane-meta" id="r-meta"></span><span class="rpane-asks" id="r-asks" hidden>Router asks</span>
    <span class="rpane-acts"><button class="ibtn" id="r-start" type="button" hidden>Start</button><button class="ibtn" id="r-stop" type="button" hidden>Stop</button>${x}</span>`;
  let ttl="",open="";
  if(src.kind==="card"){ttl=`<button type="button" class="rcard" data-card="${esc(src.num)}">#${esc(src.num)}</button> ${esc(src.title||"")}`;open=`<button class="ibtn" id="r-opencard" type="button" title="Open the card's panel">Open card</button>`}
  else if(src.kind==="lane")ttl=`Merge lane <span class="rpane-sub">${esc(src.repo||"")}</span>`;
  else if(src.kind==="sub"){ttl=`${esc(src.type||"subagent")} on <button type="button" class="rcard" data-card="${esc(src.num)}">#${esc(src.num)}</button>`;open=`<button class="ibtn" id="r-opencard" type="button" title="Open the card whose session runs it">Open card session</button>`}
  else ttl=`Outside the app${src.title?` <span class="rpane-sub">${esc(src.title)}</span>`:""}`;
  return`<span class="rdot" id="r-dot"></span><b class="rpane-ttl">${ttl}</b><span class="rpane-meta" id="r-meta"></span><span class="rpane-asks" id="r-asks" hidden>Asks you</span><span class="rpane-acts">${open}${x}</span>`;
}
// The note in place of the message box, for a session nothing can type into.
function roNote(src){
  if(src.kind==="lane")return"The merge lane works on its own: it takes no messages here.";
  if(src.kind==="sub")return`A subagent takes no messages: its card's session does. Open the card session to talk to it.`;
  if(src.kind==="outside")return`Read only: this session runs outside the app${src.cwd?`, in ${esc(src.cwd)}`:""}${src.account?` (${src.account==="account2"?"Account 2":"main account"})`:""}. Type to it where it runs.`;
  return"";
}
function openPane(src){
  src=src&&src.kind?src:{kind:"router"};
  if(pane){if(sameSrc(pane.src,src)){drawButton();return}closePane(true)}
  const router=src.kind==="router",term=src.kind==="router"||src.kind==="card",wr=src.kind==="router"||src.kind==="card";
  const label=router?"Router":"Session";
  const el=document.createElement("aside");el.className="rpane router"+(router?"":" session s-"+src.kind);el.setAttribute("aria-label",router?"Router":"Session");
  el.innerHTML=`<div class="rpane-rz" title="Drag to change the panel's width. Double-click to reset."></div>
  <div class="rpane-grid" id="r-grid"><div class="rpane-head">${headHtml(src)}</div>
  <div class="rpane-left" id="r-left"><div id="r-digest"></div>
    <div class="rpane-feedwrap"><div class="rpane-feedhead"><h3>Live log</h3><button class="ibtn rpane-col" id="r-fold" type="button" aria-expanded="true" title="Collapse the live log">▾</button></div>
    <div class="rpane-feed" id="r-feed"><div class="quiet">Reading the log…</div></div></div></div>
  <div class="rpane-right"><div class="rpane-tabs" role="tablist" aria-label="${label} views"><button class="sub on" type="button" role="tab" data-tab="chat" aria-selected="true"><b>Chat</b></button>${term?`<button class="sub" type="button" role="tab" data-tab="term" aria-selected="false"><b>Terminal</b></button>`:""}</div>
  <div class="rpane-body" id="r-body">
    <div class="rpane-chat chat" id="r-chat"><div class="msgs" id="r-msgs"><div class="quiet">Reading the conversation…</div></div><div class="busy" id="r-busy" hidden></div><div class="rtrust" id="r-trust" hidden></div>
      ${wr?`<div class="compose" id="r-compose"><div class="atts" id="r-atts"></div><textarea id="r-text" rows="2" aria-label="Message to the ${router?"Router":"card's session"}"></textarea><button class="btn" id="r-send" type="button">Send</button></div>`:`<div class="rpane-ro" id="r-ro">${roNote(src)}</div>`}
      <div class="rpane-foot" id="r-foot"></div></div>
    ${term?`<div class="rpane-term" id="r-termhost" hidden></div>`:""}
  </div></div></div>`;
  document.body.appendChild(el);
  pane={el,src,cst:null,tab:"chat",entries:[],tools:{},offset:0,open:new Set(),more:new Set(),ask:{},reading:false,timer:null,term:null,fit:null,
    feed:null,feedOpen:store("router.feedOpen")!=="0",digest:null,unlisten:[],question:null,att:[]};
  document.body.classList.add("rpane-open","router-open");
  applyWidth(width());
  el.querySelector("#r-close").onclick=()=>closePane();
  if(router){el.querySelector("#r-stop").onclick=stopRouter;el.querySelector("#r-start").onclick=startRouter}
  const oc=el.querySelector("#r-opencard");if(oc)oc.onclick=()=>{const p=pane&&pane.src;if(!p)return;if(p.kind==="sub"&&p.cardId)openCardId(p.cardId);else openCard(p.num)};
  el.querySelector("#r-fold").onclick=()=>{pane.feedOpen=!pane.feedOpen;store("router.feedOpen",pane.feedOpen?"1":"0");drawFold()};
  el.querySelectorAll(".rpane-tabs [data-tab]").forEach(b=>b.onclick=()=>showTab(b.dataset.tab));
  addResize(el.querySelector(".rpane-rz"));
  setupChat(el);
  drawFold();drawHead();drawCompose();drawButton();
  mountFeed();
  if(router){
    // The app holds the Router's toasts while its panel is open, as a card's (final-test F7).
    invoke("router_panel_opened",{open:true}).catch(()=>{});
    loadDigest();
  }
  if(src.kind==="card")refreshCard();
  pollTranscript();
  pane.timer=setInterval(()=>{if(!pane)return;if(pane.tab==="chat"){pollTranscript();if(isRouter())checkTrust()}if(kind()==="card")refreshCard()},1500);
  pane.busyTick=setInterval(drawBusy,1000);
  // A card panel already open moves to the left of this one.
  fitCardPanel();
}
// `swap`: another session opens in its place at once, so the board keeps its margin.
function closePane(swap){
  if(!pane)return;
  clearInterval(pane.timer);clearInterval(pane.busyTick);
  if(isRouter())invoke("router_panel_opened",{open:false}).catch(()=>{});
  pane.unlisten.forEach(u=>{try{u()}catch(e){}});
  dropTerm();
  if(pane.feed&&typeof pane.feed.destroy==="function"){try{pane.feed.destroy()}catch(e){}}
  pane.el.remove();pane=null;
  if(swap===true){drawButton();return}
  document.body.classList.remove("router-open");if(!document.body.classList.contains("agents-open"))document.body.classList.remove("rpane-open");
  document.documentElement.style.removeProperty("--rpane-w"); // the board margins fall back to 380 px
  fitCardPanel();
  drawButton();
}
window.openRouterPanel=openRouter;window.closeRouterPanel=()=>{if(pane&&isRouter())closePane()};
// Card #129: open any session in this panel; close it whatever it shows.
window.openSessionPane=src=>openPane(src);window.closeSessionPane=()=>closePane();
window.sessionPaneShows=()=>pane?Object.assign({},pane.src):null;
// The width the board and a card panel leave free for this panel (0 when it is closed).
window.routerPaneWidth=()=>pane?width():0;

function drawHead(){
  if(!pane)return;const el=pane.el,c=cur(),s=curState();
  const d=el.querySelector("#r-dot");d.className="rdot r-"+s;
  const k=kind();
  if(k==="router"){
    d.title=`Router: ${STATE_TEXT[s]}${stErr?` (${stErr})`:""}`;
    const meta=[st.model,st.effort].filter(Boolean).join(" · ");
    el.querySelector("#r-meta").innerHTML=`${esc(meta)}${st.mayStart?` <span class="rpane-may" title="Router may start cards (Settings, Router)">· may start cards</span>`:""}`;
    el.querySelector("#r-stop").hidden=!st.live;
    el.querySelector("#r-start").hidden=!!st.live;
  }else{
    const src=pane.src,i=pane.cst&&pane.cst.info;
    d.title=k==="card"?`Session: ${STATE_TEXT[s]}`:k==="sub"?(src.running?"Running":"Finished"):k==="lane"?`Merge lane: ${STATE_TEXT[s]}`:"Outside the app";
    if(k==="sub"&&src.running)d.className="rdot r-working";
    let meta="";
    if(k==="card"){const row=i&&(i.running||i.latest);meta=[i?(i.running?STATE_TEXT[s]:"not running"):"",(row&&row.model)||(i&&i.model),(row&&row.effort)||(i&&i.effort)].filter(Boolean).join(" · ")}
    else if(k==="sub")meta=[src.running?"running":"finished",src.desc].filter(Boolean).join(" · ");
    else if(k==="lane")meta=STATE_TEXT[s];
    else meta=[src.cwd?shortPath(src.cwd):"",src.account==="account2"?"Account 2":src.account?"main account":"",(src.cards||[]).length?(src.cards||[]).map(n=>"#"+n).join(" "):""].filter(Boolean).join(" · ");
    el.querySelector("#r-meta").textContent=meta;el.querySelector("#r-meta").title=k==="outside"&&src.cwd?src.cwd:meta;
  }
  el.querySelector("#r-asks").hidden=c.state!=="asks";
}
// A card session's state, from session_info: whether it runs, and asks.
async function refreshCard(){
  if(!pane||kind()!=="card")return;const p=pane;
  let i=null;try{i=await invoke("session_info",{cardId:p.src.cardId})}catch(e){i=null}
  if(pane!==p)return;
  const was=p.cst&&p.cst.live;
  p.cst={live:!!(i&&i.running),state:cardWord(i),info:i};
  drawHead();drawCompose();termGuard();
  if(p.term&&was!==p.cst.live)attachTerm();
}
async function startRouter(){
  const b=pane&&pane.el.querySelector("#r-start");if(b){b.disabled=true;b.textContent="Starting…"}
  try{st=Object.assign({},st,await invoke("router_start"));stErr="";foot("")}catch(e){foot(`<span class="err">Couldn't start the Router: ${esc(errText(e))}</span>`)}
  if(b){b.disabled=false;b.textContent="Start"}
  drawButton();drawHead();drawCompose();if(pane&&pane.term)attachTerm();
}
async function stopRouter(){
  const b=pane&&pane.el.querySelector("#r-stop");if(b){b.disabled=true;b.textContent="Stopping…"}
  try{st=Object.assign({},st,await invoke("router_stop"));stErr="";foot("")}catch(e){foot(`<span class="err">Couldn't stop the Router: ${esc(errText(e))}</span>`)}
  if(b){b.disabled=false;b.textContent="Stop"}
  drawButton();drawHead();drawCompose();if(pane&&pane.term)attachTerm();
}
function foot(html){if(pane)pane.el.querySelector("#r-foot").innerHTML=html}

// Drag the left edge to resize; the width is kept.
function addResize(h){
  h.addEventListener("mousedown",e=>{
    if(e.button!==0)return;e.preventDefault();
    document.body.classList.add("resizing");document.body.style.cursor="ew-resize";
    const move=ev=>{const w=Math.max(W_MIN,Math.min(innerWidth-ev.clientX-16,innerWidth-360));store(W_KEY,String(Math.round(w)));applyWidth(width())};
    const up=()=>{document.body.classList.remove("resizing");document.body.style.cursor="";removeEventListener("mousemove",move);removeEventListener("mouseup",up);if(pane&&pane.fit){try{pane.fit.fit()}catch(e){}}};
    addEventListener("mousemove",move);addEventListener("mouseup",up);
  });
  h.addEventListener("dblclick",()=>{unstore(W_KEY);applyWidth(width())});
}

// A card panel (panel.js) opens to the left of the Router panel, never over it. Its own box
// (moved or resized by Hugh) is narrowed or moved left as needed; the default box gets its
// right edge from router.css.
let fitting=false;
function fitCardPanel(){
  const p=document.querySelector("section.panel");if(!p||fitting)return;
  fitting=true;
  try{
    if(!pane||!p.style.left||p.style.left==="auto")return;
    const limit=innerWidth-width()-(document.body.classList.contains("agents-open")?parseFloat(getComputedStyle(document.documentElement).getPropertyValue("--apane-w"))+16:0)-16-14,r=p.getBoundingClientRect();
    if(r.right<=limit)return;
    const w=Math.max(Math.min(480,limit-14),Math.min(r.width,limit-14)),l=Math.max(14,Math.min(r.left,limit-w));
    p.style.left=l+"px";p.style.width=w+"px";
  }finally{fitting=false}
}
const cardPanelWatch=new MutationObserver(muts=>{
  for(const m of muts){
    if(m.type==="childList"&&[...m.addedNodes].some(n=>n.nodeType===1&&n.matches&&n.matches("section.panel"))){watchPanelStyle();fitCardPanel()}
  }
});
let styleWatch=null;
function watchPanelStyle(){
  if(styleWatch)styleWatch.disconnect();
  const p=document.querySelector("section.panel");if(!p)return;
  styleWatch=new MutationObserver(()=>{if(!fitting)fitCardPanel()});
  styleWatch.observe(p,{attributes:true,attributeFilter:["style"]});
}

// ---------- digest and the live log ----------
function cardLink(n){
  if(n==null||n==="")return"";
  return`<button type="button" class="rcard" data-card="${esc(n)}" title="Open card ${esc(n)}">#${esc(n)}</button>`;
}
function openCard(n){
  n=+n;if(!n)return;
  const c=(typeof cards!=="undefined"?cards:[]).find(x=>cardNum(x)===n);
  if(!c){if(typeof status==="function")status(`There's no card ${n} on this board.`,true);return}
  if(typeof openPanel==="function")openPanel(c.id);
}
function openCardId(id){
  const c=(typeof cards!=="undefined"?cards:[]).find(x=>x.id===id);
  if(!c){if(typeof status==="function")status("That card isn't on this board.",true);return}
  if(typeof openPanel==="function")openPanel(c.id);
}
window.routerOpenCard=openCard;
// "#12" in a line of text: bold and clickable.
function linkText(t){return esc(t).replace(/#(\d+)\b/g,(m,n)=>cardLink(n))}
async function loadDigest(){
  let r=null;try{r=await invoke("router_opened")}catch(e){r=null}
  if(!pane||!isRouter())return;
  pane.digest=r&&r.digest?r.digest:null;drawDigest();
}
function drawDigest(){
  if(!pane)return;const box=pane.el.querySelector("#r-digest"),d=pane.digest;
  if(!d){box.innerHTML="";return}
  const list=(a,f)=>Array.isArray(a)&&a.length?a.map(f).join(""):"";
  const t=x=>x&&x.title?` <span class="rd-t">${esc(x.title)}</span>`:"";
  const sec=(h,items)=>items?`<div class="rd-sec"><h4>${h}</h4><ul>${items}</ul></div>`:"";
  const since=d.since?Date.parse(d.since):NaN;
  const html=[
    sec("Moved",list(d.moved,x=>`<li>${cardLink(x.card)}${t(x)} <span class="rd-m">${esc(x.from||"?")} → ${esc(x.to||"?")}</span></li>`)),
    sec("Waiting for you",list(d.waiting,x=>`<li>${cardLink(x.card)}${t(x)}${x.why?` <span class="rd-m">${linkText(x.why)}</span>`:""}</li>`)),
    sec("May be stuck",list(d.stuck,x=>`<li>${cardLink(x.card)}${t(x)}${x.reason?` <span class="rd-m">${linkText(x.reason)}</span>`:""}</li>`)),
    sec("Router's decisions",list(d.decisions,x=>`<li>${x.card!=null?cardLink(x.card)+" ":""}${linkText(x.text||"")}${x.at?` <span class="rd-time">${esc(hm(x.at))}</span>`:""}</li>`)),
  ].join("");
  box.innerHTML=`<div class="rpane-digest"><div class="rd-head"><b>While you were away</b>${isNaN(since)?"":` <span class="rd-time" title="${esc(d.since)}">since ${esc(hm(d.since))}</span>`}<button class="ibtn x" type="button" id="r-dx" aria-label="Dismiss the digest">✕</button></div>${html||`<p class="rd-m">Nothing happened.</p>`}</div>`;
  box.querySelector("#r-dx").onclick=()=>{pane.digest=null;drawDigest()};
}
// "16:43" today, "4 Nov 16:43" before.
function hm(at){
  const d=new Date(at);if(isNaN(d))return"";
  const t=d.toLocaleTimeString("en-GB",{hour:"2-digit",minute:"2-digit"});
  return d.toDateString()===new Date().toDateString()?t:d.toLocaleDateString("en-GB",{day:"numeric",month:"short"})+" "+t;
}
// The live log's filter for what the pane shows: the Router's is the whole feed.
function feedFilter(src){
  if(src.kind==="card")return{cards:[+src.num]};
  if(src.kind==="sub"){const a=window.AgentsFeed?window.AgentsFeed.agentOf(src.type):"other";return a!=="other"?{cards:[+src.num],agents:[a]}:{cards:[+src.num]}}
  if(src.kind==="lane")return{agents:["merger"]};
  if(src.kind==="outside")return(src.cards||[]).length?{cards:src.cards.map(Number)}:null;
  return{};
}
function mountFeed(){
  const el=pane.el.querySelector("#r-feed"),f=feedFilter(pane.src);
  if(!f){el.innerHTML=`<div class="rf-empty">It hasn't named a card yet, so there is nothing to show here.</div>`;return}
  if(typeof window.mountAgentsFeed==="function"){
    // The Agents feed itself (agents_feed), which holds the Router's log too (final-test F4).
    try{el.innerHTML="";pane.feed=window.mountAgentsFeed(el,{filter:f,compact:true,onCard:n=>openCard(n),onSession:item=>window.openSessionFromFeed&&window.openSessionFromFeed(item)});return}
    catch(e){console.error("agents feed",e);pane.feed=null}
  }
  if(isRouter())refreshFeed();
}
async function refreshFeed(){
  if(!pane)return;
  if(pane.feed&&typeof pane.feed.refresh==="function"){try{pane.feed.refresh()}catch(e){}return}
  if(!isRouter())return;
  const el=pane.el.querySelector("#r-feed");
  let rows;try{rows=await invoke("router_feed",{limit:200})}catch(e){if(pane)el.innerHTML=`<div class="rf-empty err">Can't read the Router's log: ${esc(errText(e))}</div>`;return}
  if(!pane)return;
  if(!Array.isArray(rows)||!rows.length){el.innerHTML=`<div class="rf-empty">Nothing in the log yet.</div>`;return}
  el.innerHTML=rows.map(r=>{
    const txt=String(r.text||""),lead=r.card!=null&&!new RegExp(`#${r.card}\\b`).test(txt)?cardLink(r.card)+" ":"";
    return`<div class="rf-row rf-${esc(r.kind||"note")}"><time title="${esc(r.at||"")}">${esc(hm(r.at))}</time><span class="rf-agent">${esc(r.agent||"router")}</span><span class="rf-text">${lead}${linkText(txt)}</span></div>`;
  }).join("");
}
function drawFold(){
  if(!pane)return;const f=pane.el.querySelector("#r-feed"),b=pane.el.querySelector("#r-fold");
  f.hidden=!pane.feedOpen;const g=pane.el.querySelector("#r-grid");if(g)g.classList.toggle("folded",!pane.feedOpen);b.setAttribute("aria-expanded",String(pane.feedOpen));b.textContent=pane.feedOpen?"▾":"▸";b.title=pane.feedOpen?"Collapse the live log":"Show the live log";
}

// ---------- tabs ----------
function showTab(t){
  if(!pane||pane.tab===t)return;if(t==="term"&&!hasTerm())return;pane.tab=t;
  pane.el.querySelectorAll(".rpane-tabs [data-tab]").forEach(b=>{const on=b.dataset.tab===t;b.classList.toggle("on",on);b.setAttribute("aria-selected",String(on))});
  pane.el.querySelector("#r-chat").hidden=t!=="chat";
  const host=pane.el.querySelector("#r-termhost");if(host)host.hidden=t!=="term";
  if(t==="term"){if(termTaken()){termGuard();return}if(!pane.term)openTerm();else{try{pane.fit.fit()}catch(e){}}if(pane.term)pane.term.focus()}
  else pollTranscript();
}

// ---------- the terminal: xterm on the session, as the card panel's ----------
// One terminal per session (card #129): a card's terminal open in its card panel keeps this
// one away, so the two never fight over the session's size.
function termTaken(){
  return kind()==="card"&&typeof panel!=="undefined"&&!!panel&&panel.id===pane.src.cardId&&!!panel.term;
}
function dropTerm(){
  if(!pane||!pane.term)return;
  termUnlink(pane.link);pane.link=null;try{pane.term.dispose()}catch(e){}pane.term=null;pane.fit=null;
  const host=pane.el.querySelector("#r-termhost");if(host)host.innerHTML="";
}
function termGuard(){
  if(!pane||!hasTerm())return;const host=pane.el.querySelector("#r-termhost");if(!host)return;
  if(termTaken()){
    dropTerm();
    if(!host.querySelector(".rterm-note"))host.innerHTML=`<div class="rterm-note">Its terminal is open in the card panel. Close it there to use it here.</div>`;
    return;
  }
  const n=host.querySelector(".rterm-note");if(n){n.remove();if(pane.tab==="term")openTerm()}
}
function openTerm(){
  if(!pane||!hasTerm()||termTaken())return;
  const host=pane.el.querySelector("#r-termhost"),k=key();
  const term=new Terminal({fontFamily:'"IBM Plex Mono", Consolas, monospace',fontSize:13,cursorBlink:true,scrollback:5000,
    macOptionIsMeta:typeof IS_MAC!=="undefined"&&IS_MAC,theme:{background:"#0f1117",foreground:"#e6e8ef"}});
  const fit=new FitAddon.FitAddon();term.loadAddon(fit);term.open(host);pane.term=term;pane.fit=fit;
  pane.link=termLink(term,k,isRouter()?"The Router isn't running. Start it with the Start button above.":"No session is running for this card. Open the card to start or resume one.");
  new ResizeObserver(()=>{if(pane&&pane.term===term&&!host.hidden&&!termTaken()){try{fit.fit()}catch(e){}if(pane.link&&pane.link.run)invoke("session_resize",{cardId:k,cols:term.cols,rows:term.rows}).catch(()=>{})}}).observe(host);
  attachTerm();
}
// termlink.js (card #95): replays once per open or new run, so a refresh never wipes the screen.
function attachTerm(){
  if(!pane||!pane.term||!pane.link)return;
  try{pane.fit.fit()}catch(e){}
  termAttach(pane.link);
}

// ---------- the chat: the session's transcript, its questions and the message box ----------
// Claude Code's folder-trust question at the Router's first start (final-test F5): shown in the
// chat with buttons, so Hugh needn't find it in the Terminal tab.
async function checkTrust(){
  if(!pane||!isRouter())return;const box=pane.el.querySelector("#r-trust");if(!box)return;
  let q=null;
  if(st.live)try{q=await invoke("router_trust")}catch(e){q=null}
  if(!pane)return;
  if(!q){box.hidden=true;box.innerHTML="";return}
  if(!box.hidden&&box.dataset.q===q)return;
  box.dataset.q=q;box.hidden=false;
  box.innerHTML=`<div class="ask-head">The Router is asking you</div><p>${esc(q)}</p><div class="rtrust-acts"><button class="btn" type="button" data-yes="1">Yes, I trust this folder</button><button class="btn ghost" type="button" data-yes="0">No, exit</button></div>`;
  box.querySelectorAll("button").forEach(b=>b.onclick=async()=>{
    box.querySelectorAll("button").forEach(x=>x.disabled=true);
    try{await invoke("router_trust_answer",{yes:b.dataset.yes==="1"})}catch(e){foot(`<span class="err">${esc(errText(e))}</span>`)}
    setTimeout(checkTrust,1500);
  });
}
// Where each kind's conversation is read.
function readChunk(src,offset){
  if(src.kind==="router")return invoke("transcript_read",{cardId:RID,offset,agent:null});
  if(src.kind==="card")return invoke("transcript_read",{cardId:src.cardId,offset,agent:null});
  if(src.kind==="lane")return invoke("transcript_read",{cardId:src.key,offset,agent:null});
  if(src.kind==="sub")return invoke("subagent_read",{cardId:src.cardId,session:src.session,agent:src.id,offset});
  return invoke("outside_transcript_read",{id:src.id,offset});
}
async function pollTranscript(){
  if(!pane||pane.reading)return;pane.reading=true;const p=pane;
  let chunk;
  try{chunk=await readChunk(p.src,p.offset)}
  catch(e){p.reading=false;if(pane===p&&!p.entries.length)p.el.querySelector("#r-msgs").innerHTML=`<div class="quiet">Can't read the conversation: ${esc(errText(e))}</div>`;return}
  p.reading=false;if(pane!==p)return;
  if(chunk.offset<p.offset){p.entries=[];p.tools={}}
  const first=p.offset===0;p.offset=chunk.offset;
  if(!chunk.entries.length&&!first)return;
  for(const e of chunk.entries){
    if(e.kind==="queued"||e.kind==="unqueued")continue;
    if(e.kind==="result"){const t=e.tool_id&&p.tools[e.tool_id];if(t&&!t.output)t.output=e.output;continue}
    if(e.kind==="tool"&&e.tool_id)p.tools[e.tool_id]=e;
    p.entries.push(e);
  }
  renderChat(first);
}
const SHOW=300;
function emptyText(){
  const k=kind();
  if(k==="router")return st.live?"No conversation yet. Send the Router a message below.":"The Router has no conversation yet. Start it to begin one.";
  if(k==="card")return"No conversation yet. Send the session a message below.";
  if(k==="sub")return"This subagent hasn't written anything yet.";
  return"Nothing in this conversation yet.";
}
function renderChat(first){
  if(!pane)return;const box=pane.el.querySelector("#r-msgs");
  const near=box.scrollHeight-box.scrollTop-box.clientHeight<120;
  const all=groups(pane.entries),list=all.slice(-SHOW);
  if(!all.length){box.innerHTML=`<div class="quiet">${emptyText()}</div>`;return}
  const html=[];
  if(all.length>list.length)html.push(`<div class="quiet">Showing the newest ${SHOW} entries.</div>`);
  for(const g of list){
    if(g.kind==="tools"){
      const names=[...new Set(g.items.map(i=>i.text.split("\t")[0]))];
      const k="g:"+(g.items[0].tool_id||g.items[0].at||names.join());
      const errs=g.items.filter(i=>i.output&&i.output.is_error).length;
      html.push(`<details class="tools" data-k="${esc(k)}"${pane.open.has(k)?" open":""}><summary>${stamp(g.items[0].at)}<b>Ran ${g.items.length} tool${g.items.length===1?"":"s"}</b> · ${esc(names.slice(0,4).join(", "))}${names.length>4?"…":""}${errs?` <em class="terr">${errs} failed</em>`:""}</summary><div class="tlist">${g.items.map(toolHtml).join("")}</div></details>`);
    }else if(g.kind==="ask")html.push(askHtml(g.e));
    else if(g.kind==="task")html.push(`<div class="m task">${stamp(g.at)}<span>${esc(g.text)}</span></div>`);
    else if(g.kind==="cmdout")html.push(`<div class="m cmdout">${stamp(g.at)}${esc(g.text)}</div>`);
    else if(g.kind==="assistant")html.push(`<div class="m assistant">${stamp(g.at)}${md(g.text,g.at)}</div>`);
    else html.push(userHtml(g));
  }
  const fo=document.activeElement,typing=fo&&fo.classList&&fo.classList.contains("qtext")&&box.contains(fo)?{k:fo.closest("[data-ask]").dataset.ask,q:fo.dataset.q,s:fo.selectionStart,e:fo.selectionEnd}:null;
  box.innerHTML=html.join("");
  showShots(box);
  if(typing){const t=box.querySelector(`[data-ask="${CSS.escape(typing.k)}"] .qtext[data-q="${typing.q}"]`);if(t){t.focus();t.setSelectionRange(typing.s,typing.e)}}
  if(first||near)box.scrollTop=box.scrollHeight;
}
// panel.js's toolHtml and askHtml read the card panel's own state, so the Router has its own.
const OUT_LINES=8;
function toolHtml(i){
  const [n,d]=String(i.text||"").split("\t"),k=i.tool_id||"",o=i.output;
  let body=i.input?`<pre class="tin">${esc(i.input)}</pre>`:"";
  if(o){
    const long=o.text.split("\n").length>OUT_LINES||o.text.length>OUT_LINES*120,open=pane.more.has(k);
    body+=`<pre class="tout${o.is_error?" err":""}${long&&!open?" clip":""}">${o.text?esc(o.text):"<i>(no output)</i>"}</pre>`;
    if(long)body+=`<button class="more" type="button" data-more="${esc(k)}">${open?"Show less":`Show more (${o.lines} line${o.lines===1?"":"s"})`}</button>`;
    if(o.truncated)body+=`<div class="tnote">The result is cut to its first 4 KB.</div>`;
  }else body+=`<div class="tnote">No result yet.</div>`;
  if(i.images&&i.images.length&&!(o&&o.is_error))body+=`<div class="shots">${i.images.map(p=>shotImg(p,i.at)).join("")}</div>`;
  const tk="t:"+k;
  return`<details class="tool" data-k="${esc(tk)}"${k&&pane.open.has(tk)?" open":""}><summary><b>${esc(n)}</b>${d?` <span>${esc(d)}</span>`:""}${o&&o.is_error?` <em class="terr">error</em>`:""}</summary><div class="tbody">${body}</div></details>`;
}
function askState(k,n){if(!pane.ask[k])pane.ask[k]={picks:Array.from({length:n},()=>[]),text:Array(n).fill(""),sent:false};return pane.ask[k]}
// Live: the session is at a question, and this is it (the latest unanswered one). Only the
// sessions Hugh can answer: the Router and a card's.
function askLive(e){
  const c=cur();
  if(e.output||!writable()||!c.live||c.state!=="asks")return false;
  const last=[...pane.entries].reverse().find(x=>x.ask&&!x.output);return last===e;
}
function askHtml(e){
  const qs=e.ask,k=e.tool_id||"",live=askLive(e),s=askState(k,qs.length),o=e.output;
  const who=isRouter()?"The Router":"The session";
  const answers=o&&!o.is_error?askAnswers(o.text,qs):null;
  const tag=o?(o.is_error?`${who} asked · dismissed`:`${who} asked · answered`):s.sent&&live?"Sending your answer…":live?`${who} is asking you`:`${who} asked`;
  let h=`<div class="m ask${live?" live":""}${o?" done":""}" data-ask="${esc(k)}">${stamp(e.at)}<div class="ptag">${esc(tag)}</div>`;
  qs.forEach((q,i)=>{
    h+=`<div class="q"><div class="qq">${q.header?`<span class="qh">${esc(q.header)}</span>`:""}${esc(q.question)}${q.multiSelect?` <span class="qm">pick any</span>`:""}</div>`;
    if(answers)h+=`<div class="qa">→ ${esc(answers[i])}</div>`;
    else if(!o){
      const off=!live||s.sent?" disabled":"";
      h+=`<div class="opts">${q.options.map((op,j)=>{const on=s.picks[i].includes(j);
        return`<button type="button" class="opt${on?" on":""}" data-q="${i}" data-o="${j}" aria-pressed="${on}"${off}><b>${esc(op.label)}</b>${op.description&&op.description!==op.label?`<span class="od">${esc(op.description)}</span>`:""}</button>`}).join("")}</div>`;
      if(live&&!q.multiSelect)h+=`<input class="qtext" data-q="${i}" type="text" autocomplete="off" placeholder="Or type your own answer${askAuto(qs)?" and press Enter":""}" aria-label="Your own answer to: ${esc(q.question)}" value="${esc(s.text[i])}"${off}>`;
    }
    h+=`</div>`;
  });
  if(o&&o.is_error)h+=`<div class="tnote">Dismissed in the terminal, so ${isRouter()?"the Router":"the session"} stopped there.</div>`;
  else if(o&&!answers)h+=`<div class="qa">${esc(o.text)}</div>`;
  if(live&&!s.sent)h+=`<div class="pacts">${askAuto(qs)?"":`<button class="btn" type="button" data-ask-submit${askReady(qs,s)?"":" disabled"}>Submit answers</button>`}<span>${askAuto(qs)?"Click an answer, or answer in the terminal":"or answer in the terminal"}</span></div>`;
  else if(!o&&!live)h+=`<div class="tnote">${writable()&&cur().state==="asks"?"Answer it in the terminal.":"Not answered yet."}</div>`;
  return h+`</div>`;
}
async function askSubmit(k){
  if(!pane)return;const e=pane.entries.find(x=>x.ask&&(x.tool_id||"")===k);if(!e||!askLive(e))return;
  const s=askState(k,e.ask.length);if(s.sent||!askReady(e.ask,s))return;
  const answers=e.ask.map((q,i)=>!q.multiSelect&&s.text[i].trim()?{picks:[],text:s.text[i].trim()}:{picks:s.picks[i]});
  s.sent=true;renderChat(false);
  try{await invoke("session_answer",{cardId:key(),toolId:k,answers})}
  catch(err){if(!pane)return;s.sent=false;foot(`<span class="err">Not answered: ${esc(errText(err))}</span>`);renderChat(false)}
}
function setupChat(el){
  const box=el.querySelector("#r-msgs");
  const at=t=>{const c=t.closest("[data-ask]");if(!c)return null;const k=c.dataset.ask,e=pane.entries.find(x=>x.ask&&(x.tool_id||"")===k);return e?{k,e,s:askState(k,e.ask.length)}:null};
  box.addEventListener("toggle",e=>{const k=e.target.dataset&&e.target.dataset.k;if(!k||!pane)return;if(e.target.open){pane.open.add(k);showShots(e.target)}else pane.open.delete(k)},true);
  box.addEventListener("click",ev=>{
    if(!pane)return;
    const shot=ev.target.closest("img.shot[src]");if(shot){ev.preventDefault();openShot(shot);return}
    const mo=ev.target.closest("button.more");
    if(mo){const k=mo.dataset.more,pre=mo.previousElementSibling,out=pane.tools[k]&&pane.tools[k].output;
      if(pre.classList.toggle("clip")){pane.more.delete(k);mo.textContent=out?`Show more (${out.lines} line${out.lines===1?"":"s"})`:"Show more"}else{pane.more.add(k);mo.textContent="Show less"}return}
    const b=ev.target.closest(".opt,[data-ask-submit]");if(!b||b.disabled)return;
    const a=at(b);if(!a||!askLive(a.e)||a.s.sent)return;
    if(b.hasAttribute("data-ask-submit")){askSubmit(a.k);return}
    const i=+b.dataset.q,j=+b.dataset.o,q=a.e.ask[i];
    if(q.multiSelect){const p=a.s.picks[i],n=p.indexOf(j);if(n>=0)p.splice(n,1);else p.push(j)}
    else{a.s.picks[i]=[j];a.s.text[i]=""}
    if(askAuto(a.e.ask)){askSubmit(a.k);return}
    renderChat(false);
  });
  box.addEventListener("input",ev=>{
    const t=ev.target;if(!t.classList||!t.classList.contains("qtext")||!pane)return;
    const a=at(t);if(!a)return;const i=+t.dataset.q;a.s.text[i]=t.value;
    if(t.value.trim()&&a.s.picks[i].length){a.s.picks[i]=[];t.closest(".q").querySelectorAll(".opt.on").forEach(o=>{o.classList.remove("on");o.setAttribute("aria-pressed","false")})}
    const sb=t.closest("[data-ask]").querySelector("[data-ask-submit]");if(sb)sb.disabled=!askReady(a.e.ask,a.s);
  });
  box.addEventListener("keydown",ev=>{
    const t=ev.target;if(ev.key!=="Enter"||ev.isComposing||!t.classList||!t.classList.contains("qtext")||!pane)return;
    ev.preventDefault();const a=at(t);if(a&&a.s.text[+t.dataset.q].trim())askSubmit(a.k);
  });
  // Clicking "#12" anywhere in the panel opens that card.
  el.addEventListener("click",e=>{const c=e.target.closest("button.rcard[data-card]");if(!c)return;e.preventDefault();openCard(c.dataset.card)});
  // A file dropped anywhere on the pane is cancelled (dragDropEnabled is off, so the window would
  // open it), and taken only by a session that takes messages.
  el.addEventListener("dragover",onDragOver);
  el.addEventListener("drop",onDrop);
  // The message box: Enter sends, Shift+Enter is a new line.
  const ta=el.querySelector("#r-text");if(!ta)return;
  ta.addEventListener("keydown",e=>{if(e.isComposing)return;if(e.key==="Enter"&&!e.shiftKey){e.preventDefault();send()}});
  el.querySelector("#r-send").onclick=send;
  ta.addEventListener("paste",onPaste);
  el.querySelector("#r-atts").addEventListener("click",e=>{
    const b=e.target.closest(".rm");if(!b||!pane)return;
    pane.att.splice(+b.dataset.i,1);renderAtts();clearErr();ta.focus();
  });
}
// ---------- attachments: pasted or dropped images and files, saved in the data folder ----------
// As the card chat (panel.js): saved in the data folder under attachments/<key> (the Router's
// "router", a card's id), and sent as "[Attached: <path>]" lines.
// Uses panel.js's hasFiles, withAttachments, readDataUrl, baseName and IMG_EXT.
const clearErr=()=>{if(pane){const f=pane.el.querySelector("#r-foot");if(f&&f.querySelector(".err"))f.innerHTML=""}};
// A card session takes files whether it runs or not (the message waits, as in the card panel).
const canAttach=()=>writable()&&(isRouter()?st.live:true);
function onPaste(e){
  const files=[...(e.clipboardData?e.clipboardData.files:[])];
  if(!files.length||!pane||!canAttach())return;
  // Keep any text that came with the image; only an image-only paste is taken over.
  if(!e.clipboardData.getData("text/plain"))e.preventDefault();
  addFiles(files);
}
async function addFiles(files){
  if(!pane)return;const p=pane,k=key();
  for(const f of files){
    const a={path:null,name:f.name||"screenshot.png",thumb:null};
    p.att.push(a);renderAtts();
    try{
      if(f.size>25*1024*1024)throw new Error("it is over 25 MB");
      const data=await readDataUrl(f);
      if((f.type||"").startsWith("image/"))a.thumb=data;
      const name=f.name&&f.name!=="image.png"?f.name:`screenshot${f.type==="image/jpeg"?".jpg":".png"}`;
      const path=await invoke("attachment_save",{cardId:k,name,data});
      a.path=path;a.name=baseName(path);
      if(pane===p)clearErr();
    }catch(err){
      p.att=p.att.filter(x=>x!==a);
      if(pane===p)foot(`<span class="err">Couldn't attach ${esc(a.name)}: ${esc(errText(err))}</span>`);
    }
    if(pane===p)renderAtts();
  }
}
function renderAtts(){
  if(!pane)return;const box=pane.el.querySelector("#r-atts");if(!box)return;
  box.innerHTML=pane.att.map((a,i)=>`<span class="att${a.path?"":" saving"}" title="${esc(a.path||"Saving…")}">${a.thumb?`<img src="${esc(a.thumb)}" alt="">`:`<span class="ficon" aria-hidden="true">${IMG_EXT.test(a.name)?"IMG":"FILE"}</span>`}<span class="nm">${esc(a.name)}</span><button type="button" class="rm" data-i="${i}" aria-label="Remove ${esc(a.name)}">✕</button></span>`).join("");
}
// dragDropEnabled is off, so a file dropped anywhere on the pane must be cancelled or the window opens it.
// File drags only (a text drag in the box still works). dropEffect stays "copy" so a drop fires
// even with the Router off, and says why nothing was added.
function onDragOver(e){
  if(!hasFiles(e))return;e.preventDefault();e.dataTransfer.dropEffect="copy";
}
function onDrop(e){
  if(!hasFiles(e))return;e.preventDefault();
  if(!pane)return;
  if(!writable()){if(typeof status==="function")status("This session is read-only: it takes no files",true);return}
  if(!canAttach()){if(typeof status==="function")status("Start the Router to attach files",true);return}
  if(pane.tab!=="chat")showTab("chat");
  addFiles([...e.dataTransfer.files]);
}
async function send(){
  if(!pane||!writable()||pane.sending)return;if(isRouter()&&!st.live)return;
  const p=pane,ta=p.el.querySelector("#r-text");
  const snap=p.att.slice();
  if(snap.some(a=>!a.path)){foot(`<span class="err">Wait for the attachments to finish saving.</span>`);return}
  const text=withAttachments(ta.value,snap.map(a=>a.path));
  if(!text.trim())return;
  p.sending=true;
  try{
    if(isRouter())await invoke("router_send",{text});else await invoke("session_send",{cardId:p.src.cardId,text});
    if(pane===p){ta.value="";p.att=p.att.filter(a=>!snap.includes(a));renderAtts();foot("")}}
  catch(e){
    // The same message comes when the typed text holds a secret; a saved file's name can too.
    const m=errText(e),hint=snap.length&&/looks like it holds a token, key/.test(m)?" If your text is fine, rename the file and attach it again.":"";
    if(pane===p)foot(`<span class="err">Not sent: ${esc(m)}${hint}</span>`)}
  finally{if(pane===p)p.sending=false;if(p.src.kind==="router")refreshStatus();else refreshCard()}
}
function drawCompose(){
  if(!pane)return;const ta=pane.el.querySelector("#r-text"),b=pane.el.querySelector("#r-send");
  if(!ta||!b)return;
  const f=pane.el.querySelector("#r-foot");
  if(isRouter()){
    ta.disabled=b.disabled=!st.live;
    ta.placeholder=st.live?`Send the Router a message… (Enter to send, Shift+Enter for a new line${st.state==="working"?"; it is typed in when the Router is free":""})`:"Start the Router to send it a message";
    if(!f.querySelector(".err"))f.innerHTML=st.pending>0?`<span>${st.pending} waiting for the Router: typed in when it is free</span>`:"";
    return;
  }
  const c=cur(),i=pane.cst&&pane.cst.info;
  ta.disabled=b.disabled=false;
  ta.placeholder=c.live?`Send the session a message… (Enter to send, Shift+Enter for a new line${c.state==="working"?"; it is typed in when the session is free":""})`:"The session isn't running: a message waits until it starts (open the card to start or resume it)";
  if(!f.querySelector(".err"))f.innerHTML=i&&i.waiting?`<span>A message is waiting for the session.</span>`:"";
}
function drawBusy(){
  if(!pane)return;const el=pane.el.querySelector("#r-busy"),c=cur();
  const w=writable()&&c.live&&(c.state==="working"||c.state==="asks"||c.state==="starting");
  if(!w){el.hidden=true;el.innerHTML="";el.classList.remove("asking");return}
  const asking=c.state==="asks";
  el.innerHTML=asking?`<span class="g">?</span> <span class="v">Waiting for you: answer the question above, or in the terminal</span>`:`<span class="g">✻</span> <span class="v">${c.state==="starting"?"Starting…":"Working…"}</span>`;
  el.classList.toggle("asking",asking);el.hidden=false;
}

// ---------- opening a session from the Agents feed or the Running now list (card #129) ----------
// A feed row's target (AgentsFeed.sessionTarget): the Router, or a subagent the app finds by its
// id (a verdict) or by its type and the row's time, in the card's conversations or its lead's.
window.openSessionFromFeed=async item=>{
  const t=window.AgentsFeed&&window.AgentsFeed.sessionTarget(item);if(!t)return;
  if(t.kind==="router"){openRouter();return}
  let f=null;
  try{f=await invoke("subagent_find",{card:t.card,agentId:t.agentId||null,kind:t.type,before:t.before||null})}
  catch(e){if(typeof status==="function")status(`Couldn't find that ${t.type}: ${errText(e)}`,true);return}
  if(!f){if(typeof status==="function")status(`There's no ${t.type} conversation for #${t.card} on this machine.`,true);return}
  openPane({kind:"sub",cardId:f.cardId,num:t.card,session:f.session,id:f.id,type:f.agentType,desc:f.description,running:!!f.running});
};

// A lane's or a subagent's state follows the Running now poll (agents-feed.js), so the header
// says when it stops: a lane gone from the list is off, a subagent gone from it has finished.
window.onRunningNow=r=>{
  if(!pane||!r)return;const src=pane.src;
  if(src.kind==="lane"){const l=(r.lanes||[]).find(x=>x.key===src.key);const s=l?l.state:"off";if(s!==src.state){src.state=s;drawHead();drawBusy()}}
  else if(src.kind==="sub"){const on=(r.cards||[]).some(c=>(c.subagents||[]).some(x=>x.id===src.id));if(on!==!!src.running){src.running=on;drawHead()}}
};

// ---------- start-up ----------
function init(){
  const b=$("#routerBtn");if(b){b.hidden=false;b.onclick=onButton}
  drawButton();
  refreshStatus().then(setupPill);
  // card #120's filter bar may load after this file.
  addEventListener("load",()=>setTimeout(setupPill,0));
  try{
    ev().listen("router-changed",()=>{refreshStatus();refreshFeed();if(pane&&pane.term&&isRouter())attachTerm()});
    ev().listen("activity",e=>{if(e.payload&&e.payload.card===RID)refreshStatus();else if(pane&&kind()==="card"&&e.payload&&e.payload.card===pane.src.cardId)refreshCard()});
    ev().listen("session-changed",e=>{if(e.payload===RID){refreshStatus();if(pane&&pane.term&&isRouter())attachTerm()}else if(pane&&kind()==="card"&&e.payload===pane.src.cardId){refreshCard();if(pane.term)attachTerm()}});
  }catch(e){}
  // A slow check besides the events, so a missed event can't leave the dot wrong for long.
  setInterval(refreshStatus,15000);
  // The one-terminal rule: a card panel's terminal opening or closing.
  setInterval(termGuard,1000);
  cardPanelWatch.observe(document.body,{childList:true});
  if(document.querySelector("section.panel"))watchPanelStyle();
  addEventListener("resize",()=>{if(pane){applyWidth(width());if(pane.fit){try{pane.fit.fit()}catch(e){}}}});
  const h=document.querySelector("header");if(h)new ResizeObserver(placeTop).observe(h);
}
if(document.readyState==="loading")document.addEventListener("DOMContentLoaded",init);else init();
})();
