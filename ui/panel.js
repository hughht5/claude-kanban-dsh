// The card panel: the card's conversation, read from Claude's transcript, a box that types into
// its session, and its terminal behind a toggle. Uses board.js's helpers ($, esc, invoke, cards).
const SHOW=300; // render the newest entries; "Show earlier" brings back the rest
let panel=null; // {id, entries, offset, shown, info, term, fit, timer, unlisten:[]}

const b64bytes=termB64; // termlink.js
// Markdown, made safe. A Markdown image of a local file (C:/..., C:\..., file:///C:/...) keeps
// its path in data-local and loses its src, for showShots to draw through image_read: the
// page's CSP takes images only from the app itself or data: URLs. Network paths are left alone.
const LOCAL_IMG=/^(file:\/\/\/?)?[A-Za-z]:[\\/]/i;
// The system, from the app (card #117): on a Mac a local image is a /Users/... path, Cmd is the
// shortcut key, and Option is the terminal's Meta key.
// WKWebView says Macintosh at once, so a panel drawn before the app answers already has the
// Mac rules; the app's answer then settles it.
let IS_MAC=/Macintosh/.test(navigator.userAgent||"");
invoke("app_platform").then(p=>{IS_MAC=p==="macos"}).catch(()=>{});
const isLocalImg=p=>LOCAL_IMG.test(p)||(IS_MAC&&/^\/(?!\/)/.test(p));
// The shortcut key: Ctrl or Cmd on Windows; Cmd only on a Mac, so Ctrl+F and Ctrl+K reach the terminal.
const modKey=e=>IS_MAC?(e.metaKey&&!e.ctrlKey):(e.ctrlKey||e.metaKey);
function localPath(src){let p=src||"";try{p=decodeURI(p)}catch(e){}return IS_MAC?p.replace(/^file:\/\/(localhost)?/i,""):p.replace(/^file:\/\/\/?/i,"")}
let mdAt=""; // the message md() is drawing, so a screenshot path reused later loads afresh
DOMPurify.addHook("beforeSanitizeAttributes",n=>{
  if(n.nodeName!=="IMG")return;
  // marked percent-encodes the link (a backslash comes as %5C), so test the decoded path.
  const path=localPath(n.getAttribute("src"));
  if(/^(\\|\/\/)/.test(path)){n.removeAttribute("src");return} // a network path: never fetched
  if(!isLocalImg(path))return;
  n.setAttribute("data-local",path);n.setAttribute("data-at",mdAt);n.removeAttribute("src");n.classList.add("shot");
});
function md(t,at){mdAt=at||"";try{return DOMPurify.sanitize(marked.parse(t,{gfm:true,breaks:false}))}finally{mdAt=""}}
// Screenshots in the chat. Each is read once per message (path and the message's time), so a
// later message showing a shot saved again under the same name gets the new picture. Kept up
// to SHOT_BYTES in all; a failed read isn't kept, so the next redraw tries again.
const shotCache=new Map(); // key -> data URL, or a Promise while it loads
const SHOT_BYTES=150*1024*1024;
let shotBytes=0;
const shotKey=(path,at)=>path+"|"+(at||"");
function shotImg(path,at){return`<img class="shot" data-local="${esc(path)}" data-at="${esc(at||"")}" alt="${esc(path.split(/[\\/]/).pop())}" title="${esc(path)}">`}
function loadShot(path,at){
  const k=shotKey(path,at);
  if(shotCache.has(k))return Promise.resolve(shotCache.get(k));
  const pr=invoke("image_read",{path}).then(u=>{
    shotCache.set(k,u);shotBytes+=u.length;
    for(const [ok,ov] of shotCache){if(shotBytes<=SHOT_BYTES)break;if(ok!==k&&typeof ov==="string"){shotBytes-=ov.length;shotCache.delete(ok)}}
    return u;
  },err=>{shotCache.delete(k);throw err});
  shotCache.set(k,pr);
  return pr;
}
function showShots(box){
  // Only what is on view: a closed tool's screenshot loads when it is opened.
  box.querySelectorAll("img[data-local]:not([src]):not(.gone)").forEach(img=>{
    if(img.closest("details:not([open])"))return;
    const path=img.dataset.local,got=shotCache.get(shotKey(path,img.dataset.at));
    if(typeof got==="string"){img.src=got;return}
    loadShot(path,img.dataset.at).then(u=>{img.src=u},err=>{img.classList.add("gone");img.alt=`Can't show ${path}: ${errText(err)}`});
  });
}
// A user message's "[Attached: <path>]" / "[Image: <path>]" lines, when the path is an image.
function attachedImages(text){
  const out=[];const re=/\[(?:Attached|Image): ([^\]\n]+)\]/g;let m;
  while((m=re.exec(text)))if(IMG_EXT.test(m[1].trim()))out.push(m[1].trim());
  return out;
}
// Clicking a screenshot shows it full size over the panel; a click or Esc closes it.
function openShot(img){
  const v=document.createElement("div");v.className="shotview";v.setAttribute("role","dialog");v.setAttribute("aria-label","Screenshot");v.tabIndex=-1;
  v.innerHTML=`<img alt="${esc(img.alt||"")}"><div class="cap">${esc(img.dataset.local||"")}</div>`;v.querySelector("img").src=img.src;
  const was=document.activeElement,close=()=>{v.remove();if(was&&was.isConnected&&was!==document.body)was.focus({preventScroll:true})};
  v.onclick=close;v.addEventListener("keydown",e=>{if(e.key==="Escape"){e.preventDefault();e.stopPropagation();close()}});
  // In a modal dialog (the stack overlay, card #116) the view goes inside it, or it would be behind.
  (document.querySelector("dialog[open]")||document.body).append(v);v.focus();
}

// opts.step (card #102, review.js): the Human test review switches card in place, so the close
// before the open keeps the review's queue and doesn't hand the focus back to the board.
function openPanel(id,opts){
  if(panel&&panel.id===id)return; // already open: keep what Hugh is typing
  const step=!!(opts&&opts.step);
  closePanel(step);
  const c=cards.find(x=>x.id===id);if(!c)return;
  const back=document.createElement("div");back.className="panel-back";back.onclick=closePanel;
  const el=document.createElement("section");el.className="panel";el.setAttribute("role","dialog");el.setAttribute("aria-label","Card session");
  const n=issueNum(c);
  el.innerHTML=`<div class="ph"><span class="state" id="p-state" title="No session running"></span>
    <div class="ttl">${numBadge(c)}${n?`<span class="iss">#${esc(n)}</span>`:""}${safe(c.title||"Untitled card")}</div>
    <div class="acts"><span class="live" id="p-live"></span><button class="ibtn" id="p-term" type="button" aria-pressed="false" title="Show the session's terminal">Terminal</button>
    <button class="ibtn" id="p-stop" type="button" hidden>Stop session</button>
    <button class="ibtn x" id="p-close" type="button" aria-label="Close">✕</button></div></div>
  <div class="pb" id="p-body"><aside class="det" id="p-det" aria-label="Card details"></aside><div class="dsplit" title="Drag to change the details' width. Double-click to reset."></div>
    <div class="work" id="p-work"><div class="chat">
      <div class="find" role="search"><input type="search" id="p-q" placeholder="Search the conversation (Ctrl+F)" aria-label="Search the conversation" autocomplete="off" spellcheck="false"><span class="cnt" id="p-cnt" aria-live="polite"></span><button class="ibtn" id="p-prev" type="button" aria-label="Previous match" title="Previous match (Shift+Enter)" disabled>↑</button><button class="ibtn" id="p-next" type="button" aria-label="Next match" title="Next match (Enter)" disabled>↓</button></div>
      <div class="subs" id="p-subs" role="tablist" aria-label="Main conversation and its subagents"></div><div class="msgs" id="p-msgs"><div class="quiet">Reading the conversation…</div></div><div class="busy" id="p-busy" hidden></div><div class="compose" id="p-compose"></div><div class="sfoot" id="p-sfoot" hidden></div></div></div></div>
  <div class="pf" id="p-foot"></div>`;
  document.body.append(back,el);
  addPanelEdge(el);
  panel={id,el,back,entries:[],tools:{},offset:0,shown:SHOW,info:null,term:null,fit:null,timer:null,unlisten:[],
    open:new Set(),more:new Set(),find:{q:"",hits:[],i:-1},att:[],status:null,queued:[],ask:{},view:"",gen:0,subs:[],subsTick:0,lastMainUser:null};
  setupFind(el);setupToolToggles(el);setupDrop(id);setupAsk(el);setupSubs(el);
  if(typeof reviewOpened==="function")reviewOpened(id,step); // review.js: the Human test queue
  // The details column (board.js renders it), and the board's record of which card is open.
  if(!draft||draft.id!==id){draft=null;stale=null}
  selected=id;returnFocusTo=id;try{history.replaceState(null,"","#"+id)}catch(e){}
  addDetailSplit(el.querySelector("#p-body"));
  render();
  el.querySelector("#p-close").onclick=closePanel;
  el.querySelector("#p-term").onclick=toggleTerm;
  el.querySelector("#p-stop").onclick=stopSession;
  document.addEventListener("keydown",panelKeys);
  invoke("panel_opened",{cardId:id}).catch(()=>{}); // no toasts for this card while the window is focused
  refreshInfo().then(pollTranscript);
  panel.timer=setInterval(pollTranscript,1500);
  const ev=window.__TAURI__.event;
  ev.listen("session-changed",e=>{if(panel&&e.payload===id)refreshInfo()}).then(u=>panel&&panel.unlisten.push(u));
  ev.listen("session-status",e=>{if(panel&&panel.id===id&&(!panel.status||!panel.status.row||panel.status.row===e.payload))refreshStatusSoon()}).then(u=>panel&&panel.unlisten.push(u));
  panel.statusTick=setInterval(()=>{if(panel&&panel.status&&panel.status.running)renderStatus();renderBusy();if(panel&&panel.subs.some(subRunning))renderSubs()},1000);
}
function panelKeys(e){
  if(modKey(e)&&!e.altKey&&!e.shiftKey&&e.key.toLowerCase()==="f"&&panel&&!$("#addDlg").open){e.preventDefault();const q=panel.el.querySelector("#p-q");q.focus();q.select();return}
  if(e.key==="Escape"&&panel&&!$("#addDlg").open&&!(e.target.closest&&e.target.closest("#cmdbar"))){e.preventDefault();closePanel()}}
let returnFocusTo=null;
function closePanel(step){
  if(typeof reviewClosed==="function")reviewClosed(step===true); // a close by Hugh ends the Human test queue
  if(!panel)return;
  clearInterval(panel.timer);clearInterval(panel.statusTick);clearTimeout(panel.statusSoon);clearTimeout(panel.findTimer);
  panel.unlisten.forEach(u=>{try{u()}catch(e){}});
  if(panel.term){termUnlink(panel.link);panel.term.dispose()}
  panel.el.remove();panel.back.remove();panel=null;
  invoke("panel_opened",{cardId:null}).catch(()=>{});
  document.removeEventListener("keydown",panelKeys);
  selected=null;draft=null;stale=null;try{history.replaceState(null,"",location.pathname)}catch(e){}
  if(step===true)return; // the review switching card: the next panel opens straight away
  render();
  const back=returnFocusTo;returnFocusTo=null;
  if(back){const k=document.querySelector(`.card[data-id="${CSS.escape(back)}"]`);if(k)k.focus()}
}

async function refreshInfo(){
  if(!panel)return;const id=panel.id;
  let info;try{info=await invoke("session_info",{cardId:id})}catch(e){foot(`<span class="err">${esc(errText(e))}</span>`);return}
  if(!panel||panel.id!==id)return;panel.info=info;
  const run=!!info.running;
  if(info.state!=="working")panel.midTurn=false;
  const q=info.state==="permission"&&info.question!=null?info.question:null;
  if(q!==panel.askOn){panel.askOn=q;if(panel.entries.length)renderChat(false)}
  noteTurn(info);
  const ob=outbox(info).map(o=>o.kind+o.text).join("\u0000");
  if(ob!==panel.outboxKey){panel.outboxKey=ob;renderChat(false)}
  const bk=JSON.stringify(info.btw||[]);if(bk!==panel.btwKey){const first=panel.btwKey==null;panel.btwKey=bk;if(!first||(info.btw||[]).length)renderChat(false)}
  const st=panel.el.querySelector("#p-state");const sv=info.state||(run?"starting":"");st.className="state"+(run?" running":"")+(sv?" s-"+sv:"");st.title=run?`Session: ${STATE_LABEL[sv]||sv}`:"No session running in the app";
  panel.el.querySelector("#p-stop").hidden=!run;
  const row=info.running||info.latest;
  foot(`<span>${run?(STATE_LABEL[info.state]||"starting").toLowerCase():"not running"}</span>${info.waiting?`<span>${esc(waitingWhy(info))}</span>`:""}${panel.midTurn&&info.state==="working"&&!pendingQueued().length?`<span>message sent mid-turn: Claude reads it at its next step</span>`:""}<span>${info.account==="account2"?"Account 2":"main account"}</span><span>${esc((row&&row.model)||info.model)}</span><span>effort ${esc((row&&row.effort)||info.effort)}</span>${info.claudeVersion?`<span>Claude Code ${esc(info.claudeVersion)}</span>`:""}${row?`<span title="Folder">${esc(row.cwd)}</span>`:""}${info.conversation?`<span title="Conversation">${esc(info.conversation.id.slice(0,8))}</span>`:""}`);
  compose(info);
  if(typeof liveSwitch==="function")liveSwitch(info); // live-switch.js: model/effort switch, queued badge
  if(panel.term&&(run||(panel.link&&panel.link.run)))attachTerm(); // replays only for a new run; shows the idle line once it stops (card #95)
  refreshStatus();
}
// Why a message from the box hasn't been typed yet.
function waitingWhy(info){
  if(info.state==="permission")return info.question!=null?"message waiting: the session is asking you a question; answer it in the chat or the terminal":"message waiting: the session is asking you something in the terminal; answer it there";
  if(info.running&&info.screen)return "message waiting: a screen is open in the terminal; it goes in once that closes (Esc there)";
  if(info.running&&info.state!=="starting")return "message waiting: typed in a moment";
  if(info.queued!=null)return "message waiting: the session is queued to start, over the session cap";
  if(!info.running&&info.conversation&&info.conversation.source==="desktop")return "message waiting: resume the session to send it";
  return "message waiting: typed once the session is ready";
}
function foot(html){if(panel)panel.el.querySelector("#p-foot").innerHTML=html}

// The app's own outbox for the chat, from session_info: a message waiting to be typed (with
// any of the app's text, such as a column prompt, that goes in with it), one typed and not
// yet taken by Claude, and one the session didn't take after the app's tries. Each shows as
// a queued bubble until the transcript has it.
function outbox(info){
  if(!info)return [];
  const out=[],key=t=>String(t||"").replace(/[^\p{L}\p{N}]/gu,"").slice(0,60);
  // Already in the transcript (the panel reads it every 1.5 s): no bubble for it.
  // While a subagent is on view, panel.entries are its own: use the main chat's last message.
  const lastUser=panel&&panel.lastMainUser;
  const shown=t=>lastUser&&key(lastUser.text)===key(t);
  if(info.untaken)out.push({kind:"untaken",text:info.untaken});
  if(info.typing&&!shown(info.typing))out.push({kind:"typing",text:info.typing});
  if(info.waitingText)out.push({kind:"queued",text:info.waitingText});
  return out;
}
function outboxTag(o,info){
  if(o.kind==="untaken")return`Not sent: the session didn't take it after ${info.typeTries||3} tries`;
  if(o.kind==="typing")return"Typing it into the session…";
  return"Queued · "+waitingWhy(info).replace(/^message waiting: /,"");
}

// Messages in Claude Code's queue, only while this run of the session is up: what an earlier
// run left queued was never read, and isn't waiting any more.
function pendingQueued(){
  const run=panel.info&&panel.info.running;if(!run||!panel.queued.length)return [];
  const since=run.started_at||run.startedAt||"";
  return panel.queued.filter(q=>!since||!q.at||q.at>=since);
}
// Send now, as Ctrl+X Ctrl+S in the terminal: mid-turn it interrupts the turn and sends the box's
// text (and anything queued) at once. With an empty box (the waiting message's Send now) it
// sends what is queued. Not mid-turn, it is an ordinary Send.
async function sendNow(queuedOnly){
  if(!panel)return;const id=panel.id,ta=panel.el.querySelector("#p-text");
  const paths=panel.att.filter(a=>a.path).map(a=>a.path);
  if(!queuedOnly&&panel.att.some(a=>!a.path)){foot(`<span class="err">Wait for the attachments to finish saving.</span>`);return}
  if(!queuedOnly&&typeof reviewFailing==="function"&&reviewFailing()){const t=withAttachments(ta.value,paths);if(t.trim())reviewFailSend(t);return}
  const text=queuedOnly?"":withAttachments(ta.value,paths);
  if(!queuedOnly&&!text.trim()&&!pendingQueued().length)return;
  hideSlash();
  try{await invoke("session_send_now",{cardId:id,text});if(!panel||panel.id!==id)return;
    if(!queuedOnly){ta.value="";panel.att=[];renderAtts();slashSent(text)}refreshInfo()}
  catch(e){if(panel&&panel.id===id)foot(`<span class="err">Not sent: ${esc(errText(e))}</span>`)}
}

// The `/` menu, as the terminal's: typing a slash at the start of the box lists Claude Code's
// commands and the skills and commands the session sees (slash_commands), filtered as you type.
// Up/Down pick, Tab fills it in, Enter runs it (or fills it in when it needs more, as /btw does),
// Esc closes the menu. /btw is answered here in the chat (session_btw); a command that opens a
// screen of its own (/config, a bare /model) opens the terminal beside the chat.
const SLASH_TAG={btw:"answers here",chat:"",app:"use Stop session",skill:"skill",command:"command"};
function isBtw(t){return /^\s*\/btw(\s|$)/.test(t)}
async function slashList(){
  // Until the app has Claude Code's own list (complete), each new menu asks again.
  const have=panel.slashList&&panel.slashList.id===panel.id?panel.slashList:null;
  if(have&&(have.complete||Date.now()-have.at<1500))return have.items;
  const id=panel.id;let r={items:have?have.items:[],complete:false};try{r=await invoke("slash_commands",{cardId:id})}catch(e){}
  if(panel&&panel.id===id)panel.slashList={id,items:r.items,complete:r.complete,at:Date.now()};return r.items;
}
function hideSlash(){const m=panel&&panel.el.querySelector("#p-slash");if(m){m.hidden=true;m.innerHTML=""}if(panel)panel.slash=null}
async function slashMenu(ta,send){
  const v=ta.value;
  if(!/^\/\S*$/.test(v)||(typeof reviewFailing==="function"&&reviewFailing())){hideSlash();return}
  const items=await slashList();if(!panel||ta.value!==v||panel.sending)return;
  const q=v.slice(1).toLowerCase();
  const starts=[],inName=[],inDesc=[];
  for(const c of items){const n=c.name.toLowerCase();if(n.startsWith(q))starts.push(c);else if(q&&n.includes(q))inName.push(c);else if(q&&c.description.toLowerCase().includes(q))inDesc.push(c)}
  const list=[...starts,...inName,...inDesc];
  if(!list.length){hideSlash();return}
  // Typing picks the best match afresh, as the terminal does, so Enter never runs an earlier
  // pick that now matches only by its description.
  panel.slash={list,i:0,send,v};drawSlash();
}
function drawSlash(){
  const m=panel.el.querySelector("#p-slash"),s=panel.slash;if(!m||!s)return;
  m.innerHTML=s.list.map((c,i)=>{const tag=SLASH_TAG[c.kind]??"";const src=c.source&&c.source!=="user"&&c.source!=="project"?c.source:"";
    return`<div class="sc${i===s.i?" on":""}" role="option" aria-selected="${i===s.i}" data-i="${i}"><b>/${esc(c.name)}</b><span class="sd">${esc(c.description)}</span>${tag||src?`<em>${esc([tag,src].filter(Boolean).join(" · "))}</em>`:""}</div>`}).join("");
  m.hidden=false;
  const on=m.querySelector(".sc.on");if(on)on.scrollIntoView({block:"nearest"});
  m.querySelectorAll(".sc").forEach(el=>{el.onmousedown=e=>{e.preventDefault();panel.slash.i=+el.dataset.i;pickSlash(false)}});
}
// Fills the picked command into the box; with `run`, sends it too, unless it needs more after it.
function pickSlash(run){
  const s=panel.slash;if(!s)return;const c=s.list[s.i],ta=panel.el.querySelector("#p-text");
  const needs=c.kind==="btw"||c.kind==="app"||/<[^>]+>/.test(c.description);
  ta.value=`/${c.name}${run&&!needs?"":" "}`;hideSlash();ta.focus();ta.setSelectionRange(ta.value.length,ta.value.length);
  if(run&&!needs)s.send();
}
// The menu's keys; true when it took the key.
function slashKeys(e,ta,send){
  const s=panel.slash;if(!s)return false;
  // A menu drawn for other text (it opened after that text was sent) is stale.
  if(s.v!==ta.value){hideSlash();return false}
  const ctrl=e.ctrlKey||e.metaKey;
  if(e.key==="ArrowDown"||e.key==="ArrowUp"){e.preventDefault();s.i=(s.i+(e.key==="ArrowDown"?1:-1)+s.list.length)%s.list.length;drawSlash();return true}
  if(e.key==="Tab"&&!e.shiftKey){e.preventDefault();pickSlash(false);return true}
  if(e.key==="Enter"&&!e.shiftKey&&!ctrl){e.preventDefault();pickSlash(true);return true}
  if(e.key==="Escape"){e.preventDefault();e.stopPropagation();hideSlash();return true}
  return false;
}
// After a slash command went: if it left a screen of its own up (/config, /cost, a bare /model:
// session_info's screen, read off the session's screen), the terminal opens beside the chat so
// Hugh can use it. Checked a few times while the command draws.
function slashSent(text){
  if(!/^\s*\//.test(text)||isBtw(text)||!panel)return;
  const id=panel.id;
  [1500,3000,5000].forEach(ms=>setTimeout(async()=>{
    if(!panel||panel.id!==id||panel.term)return;
    try{const i=await invoke("session_info",{cardId:id});if(panel&&panel.id===id&&i.screen&&!panel.term)toggleTerm()}catch(e){}
  },ms));
}

// The message box is always there. With a session running, Send types the message in at once
// (as typing in the terminal would; mid-turn Claude Code queues it). With none, Send starts it
// (resumes this card's conversation in the app, else a new session; over the cap it queues)
// and the message waits until it's ready. The row above the box says which, with buttons to
// start without a message.
function compose(info){
  const box=panel.el.querySelector("#p-compose");
  if(!box.querySelector("#p-text")){
    box.innerHTML=`<div class="slashmenu" id="p-slash" role="listbox" aria-label="Slash commands" hidden></div><div class="startrow" id="p-startrow"></div><div class="atts" id="p-atts" aria-label="Attachments"></div><textarea id="p-text" rows="2" aria-label="Message to the card's session"></textarea><button class="btn" id="p-send" type="button">Send</button><button class="btn ghost" id="p-bug" type="button" hidden title="Send this message as a bug report: the card-reproducer subagent reproduces the bug before the fix and proves the fix after it">Report a bug</button>`;
    const ta=box.querySelector("#p-text");
    // bug: Hugh's "Report a bug" (card #112): the message goes as a bug report for the card-reproducer.
    const post=async bug=>{
      const paths=panel.att.filter(a=>a.path).map(a=>a.path);
      if(panel.att.some(a=>!a.path)){foot(`<span class="err">Wait for the attachments to finish saving.</span>`);return}
      // Fail in Human test (review.js): the box's text is Hugh's note, sent as the card's Fail.
      if(typeof reviewFailing==="function"&&reviewFailing()){const t=withAttachments(ta.value,paths);if(t.trim())reviewFailSend(t);return}
      if(!bug&&isBtw(ta.value)&&panel.att.length){foot(`<span class="err">A /btw question can't take attachments: remove them, or send it as a message.</span>`);return}
      const text=withAttachments(ta.value,paths);if(!text.trim()){if(bug)foot(`<span class="err">Say what the bug is in the box first, then Report a bug.</span>`);return}
      hideSlash();const id=panel.id;
      const mid=!!(panel.info&&panel.info.running&&panel.info.state==="working");
      panel.sending=true;
      try{await invoke("session_send",{cardId:id,text,bug});if(!panel||panel.id!==id)return;panel.midTurn=mid&&(bug||!text.trim().startsWith("/"));ta.value="";hideSlash();panel.att=[];renderAtts();if(!bug)slashSent(text);refreshInfo()}catch(e){if(panel&&panel.id===id)foot(`<span class="err">Not sent: ${esc(errText(e))}</span>`)}
      finally{if(panel)panel.sending=false}};
    const send=()=>post(false);
    // As in the terminal: Enter sends (mid-turn it waits for Claude's next step); Ctrl+X Ctrl+S
    // or Ctrl+Enter sends now, interrupting the turn. Ctrl+X with text selected still cuts.
    let chordAt=0;
    ta.addEventListener("input",()=>slashMenu(ta,send));
    ta.addEventListener("blur",()=>setTimeout(()=>{if(panel&&document.activeElement!==ta)hideSlash()},150));
    ta.addEventListener("keydown",e=>{
      if(e.isComposing)return;
      // Esc in fail mode (review.js) leaves fail mode; the panel stays open.
      if(e.key==="Escape"&&typeof reviewFailing==="function"&&reviewFailing()){e.preventDefault();e.stopPropagation();reviewFailEnd();return}
      if(slashKeys(e,ta,send))return;
      const k=e.key.toLowerCase(),ctrl=e.ctrlKey||e.metaKey;
      if(ctrl&&!e.altKey&&k==="x"&&ta.selectionStart===ta.selectionEnd){e.preventDefault();chordAt=Date.now();return}
      if(ctrl&&!e.altKey&&k==="s"&&Date.now()-chordAt<2000){e.preventDefault();chordAt=0;sendNow(false);return}
      if(!["control","meta","shift","alt"].includes(k))chordAt=0;
      if(e.key==="Enter"&&ctrl&&!e.shiftKey){e.preventDefault();sendNow(false);return}
      if(e.key==="Enter"&&!e.shiftKey){e.preventDefault();send()}});
    ta.addEventListener("paste",onPaste);
    box.querySelector("#p-send").onclick=send;box.querySelector("#p-bug").onclick=()=>post(true);if(!(typeof reviewOwnsFocus==="function"&&reviewOwnsFocus()))ta.focus();
    slashList().catch(()=>{}); // so the / menu has Claude Code's own list by the time it opens
    renderAtts();
  }
  const ta=box.querySelector("#p-text"),row=box.querySelector("#p-startrow");
  // Report a bug is for a card being built; in Human test the bug report is Fail (router, card #112),
  // and a stack's card has Found a problem, which reaches the session leading the stack.
  {const card=cards.find(x=>x.id===panel.id),bb=box.querySelector("#p-bug");if(bb)bb.hidden=!(card&&colOf(card)==="In progress"&&!isArch(card)&&card.stackOf==null)}
  const c=info.conversation,desktop=!!(c&&c.source==="desktop");
  const hint="Enter to send, Shift+Enter for a new line; paste or drop screenshots and files";
  ta.placeholder=info.running?`Send a message… (Enter to send; while Claude works, Ctrl+X Ctrl+S sends it now and interrupts; Shift+Enter for a new line; paste or drop files)`
    :desktop?`Write a message… it is sent once you resume the session (${hint})`
    :`Send a message… it ${c?"resumes":"starts"} the session and is typed once it's ready (${hint})`;
  if(info.running||info.queued!=null){row.innerHTML="";row.dataset.key="";return}
  const where=c?(c.source==="desktop"?"This card's conversation ran in the Claude desktop app.":"This card's session isn't running."):"This card has no session yet.";
  const key=`${!!c}|${desktop}|${where}`;if(row.dataset.key===key)return;row.dataset.key=key;
  row.innerHTML=`${esc(where)}
    ${c?`<button class="btn ghost" id="p-resume" type="button">Resume in the app</button>`:""}
    <button class="btn ghost" id="p-new" type="button">Start a new session</button>
    ${desktop?`<span>Close it in the desktop app first, so the same conversation doesn't run twice.</span>`:""}`;
  const go=mode=>async()=>{
    try{await invoke("session_start",{cardId:panel.id,mode});await refreshInfo();if(mode==="new"){panel.view="";panel.gen++;panel.reading=null;panel.lastMainUser=null;panel.subs=[];panel.subsKey="";panel.offset=0;panel.entries=[];panel.tools={};renderSubs()}}
    catch(e){foot(`<span class="err">Couldn't start it: ${esc(errText(e))}</span>`)}};
  const r=row.querySelector("#p-resume");if(r)r.onclick=go("resume");
  row.querySelector("#p-new").onclick=go("new");
}

async function stopSession(){
  if(!panel)return;const b=panel.el.querySelector("#p-stop");b.disabled=true;b.textContent="Stopping…";
  try{await invoke("session_stop",{cardId:panel.id})}catch(e){foot(`<span class="err">${esc(errText(e))}</span>`)}
  b.disabled=false;b.textContent="Stop session";refreshInfo();
}

async function pollTranscript(){
  if(!panel)return;const id=panel.id,view=panel.view,gen=panel.gen;
  if(panel.subsTick++%2===0)refreshSubs();
  // One read at a time, so two reads from the same offset can't both add their entries.
  if(panel.reading===gen)return;panel.reading=gen;
  let chunk;
  try{chunk=await invoke("transcript_read",{cardId:id,offset:panel.offset,agent:view||null})}
  catch(e){if(panel&&panel.id===id&&panel.gen===gen){panel.reading=null;if(view)showView("")}return}
  if(!panel||panel.id!==id||panel.gen!==gen)return;
  panel.reading=null;
  if(chunk.offset<panel.offset){panel.entries=[];panel.tools={};panel.thought=null;panel.queued=[]} // the file was replaced: start over
  if(chunk.thought){panel.thought=chunk.thought;renderBusy()}
  const first=panel.offset===0;
  panel.offset=chunk.offset;
  if(!chunk.entries.length&&!first)return;
  for(const e of chunk.entries){
    // Claude Code's own queue: a message sent mid-turn waits there until Claude picks it up.
    if(e.kind==="queued"){panel.queued.push(e);continue}
    if(e.kind==="unqueued"){if(!e.text)panel.queued=[];else{const i=panel.queued.findIndex(q=>q.text===e.text);if(i>=0)panel.queued.splice(i,1)}continue}
    // A result whose call came in an earlier read: put it on that call.
    if(e.kind==="result"){const t=e.tool_id&&panel.tools[e.tool_id];if(t&&!t.output)t.output=e.output;continue}
    if(e.kind==="tool"&&e.tool_id)panel.tools[e.tool_id]=e;
    if(e.kind==="user"&&!view)panel.lastMainUser=e;
    panel.entries.push(e);
  }
  renderChat(first);
}

function groups(entries){
  const out=[];
  for(const e of entries){
    const last=out[out.length-1];
    if(e.kind==="tool"&&e.ask)out.push({kind:"ask",e});
    else if(e.kind==="tool"){if(last&&last.kind==="tools")last.items.push(e);else out.push({kind:"tools",items:[e]})}
    else out.push(e);
  }
  return out;
}
// A message's time: "16:43" today, "4 Nov 16:43" this year, "4 Nov 2025 16:43" before;
// hovering shows the full date and time. Nothing when the transcript line has no timestamp.
function stamp(at){
  const d=at?new Date(at):null;
  if(!d||isNaN(d))return"";
  const now=new Date(),hm=d.toLocaleTimeString("en-GB",{hour:"2-digit",minute:"2-digit"});
  const day=d.toDateString()===now.toDateString()?"":d.toLocaleDateString("en-GB",d.getFullYear()===now.getFullYear()?{day:"numeric",month:"short"}:{day:"numeric",month:"short",year:"numeric"})+" ";
  const full=d.toLocaleString("en-GB",{weekday:"short",day:"numeric",month:"short",year:"numeric",hour:"2-digit",minute:"2-digit",second:"2-digit"});
  return`<span class="ts" title="${esc(full)}">${esc(day+hm)}</span>`;
}
function renderChat(first){
  const box=panel.el.querySelector("#p-msgs");
  const near=box.scrollHeight-box.scrollTop-box.clientHeight<120;
  const all=groups(panel.entries);
  // While searching, everything is drawn, so a match in older messages counts too.
  const list=panel.find.q?all:all.slice(-panel.shown);
  const sub=panel.view?panel.subs.find(s=>s.id===panel.view):null;
  const waiting=panel.view?[]:pendingQueued(),ob=panel.view?[]:outbox(panel.info);
  // The /btw side questions (main view only), each placed after the last message before it.
  const btws=panel.view?[]:((panel.info&&panel.info.btw)||[]);
  if(btws.length){
    const at=g=>g.at||(g.items&&g.items[0].at)||(g.e&&g.e.at)||"";
    for(const b of btws){const t=Date.parse(b.at);let i=list.length;while(i>0){const a=Date.parse(at(list[i-1]));if(isNaN(a)||a<=t)break;i--}list.splice(i,0,{kind:"btw",b})}
  }
  if(panel.view&&!all.length){box.innerHTML=subNote(sub)+`<div class="quiet">This subagent hasn't written anything yet.</div>`;runFind(true);return}
  if(!all.length&&!waiting.length&&!ob.length&&!btws.length){
    const c=panel.info&&panel.info.conversation;
    box.innerHTML=`<div class="quiet">${c?"The conversation's transcript isn't on this machine yet.":"No conversation yet. Send a message below to start a session."}</div>`;runFind(true);return}
  const html=[];
  if(panel.view)html.push(subNote(sub));
  if(all.length>list.length)html.push(`<button class="ibtn earlier" id="p-earlier" type="button">Show ${Math.min(SHOW,all.length-list.length)} earlier</button>`);
  for(const g of list){
    if(g.kind==="tools"){
      const names=[...new Set(g.items.map(i=>i.text.split("\t")[0]))];
      const k="g:"+(g.items[0].tool_id||g.items[0].at||names.join());
      const errs=g.items.filter(i=>i.output&&i.output.is_error).length;
      const gs=panel.view?[]:g.items.map(i=>i.tool_id&&panel.subs.find(s=>s.tool_id===i.tool_id)).filter(Boolean);
      html.push(`<details class="tools" data-k="${esc(k)}"${panel.open.has(k)?" open":""}><summary>${stamp(g.items[0].at)}<b>Ran ${g.items.length} tool${g.items.length===1?"":"s"}</b> · ${esc(names.slice(0,4).join(", "))}${names.length>4?"…":""}${errs?` <em class="terr">${errs} failed</em>`:""}${gs.map(s=>` <button class="subopen" type="button" data-agent="${esc(s.id)}" title="Show ${esc(s.agent_type)}'s own conversation: ${esc(s.description)}">${subDot(s)}View ${esc(s.agent_type)}</button>`).join("")}</summary><div class="tlist">${g.items.map(toolHtml).join("")}</div></details>`);
    }else if(g.kind==="ask")html.push(askHtml(g.e));
    else if(g.kind==="task")html.push(taskHtml(g));
    else if(g.kind==="btw")html.push(btwHtml(g.b));
    else if(g.kind==="cmdout")html.push(`<div class="m cmdout">${stamp(g.at)}${esc(g.text)}</div>`);
    else if(g.kind==="assistant")html.push(`<div class="m assistant">${stamp(g.at)}${md(g.text,g.at)}</div>`);
    else{const im=attachedImages(g.text);html.push(`<div class="m user">${stamp(g.at)}${esc(g.text)}${im.length?`<div class="shots">${im.map(p=>shotImg(p,g.at)).join("")}</div>`:""}</div>`)}
  }
  // Sent but not yet picked up: faded, until Claude reads it at its next step (or Send now).
  waiting.forEach((q,i)=>html.push(`<div class="m user pending" title="Sent: Claude reads it at its next step"><div class="ptag">Sent · waiting for Claude to pick it up</div>${esc(q.text)}${i===waiting.length-1?`<div class="pacts"><button class="btn ghost" type="button" id="p-sendnow" title="Interrupt the turn and send it now, as Ctrl+X Ctrl+S in the terminal">Send now</button><span>or Ctrl+X Ctrl+S</span></div>`:""}</div>`));
  // The app's outbox (outbox()): queued before the session is up, typing, or not taken.
  ob.forEach(o=>html.push(`<div class="m user pending${o.kind==="untaken"?" untaken":""}" data-ob="${o.kind}"><div class="ptag">${esc(outboxTag(o,panel.info))}</div><div class="ptxt">${esc(o.text)}</div>${o.kind==="untaken"?`<div class="pacts"><button class="btn ghost" type="button" id="p-again">Send again</button><button class="btn ghost" type="button" id="p-dismiss">Dismiss</button></div>`:""}</div>`));
  const fo=document.activeElement,typing=fo&&fo.classList&&fo.classList.contains("qtext")&&box.contains(fo)?{k:fo.closest("[data-ask]").dataset.ask,q:fo.dataset.q,s:fo.selectionStart,e:fo.selectionEnd}:null;
  box.innerHTML=html.join("");
  showShots(box);
  if(typing){const t=box.querySelector(`[data-ask="${CSS.escape(typing.k)}"] .qtext[data-q="${typing.q}"]`);if(t){t.focus();t.setSelectionRange(typing.s,typing.e)}}
  const sn=box.querySelector("#p-sendnow");if(sn)sn.onclick=()=>sendNow(true);
  const ag=box.querySelector("#p-again");if(ag)ag.onclick=async()=>{const id=panel.id,t=panel.info&&panel.info.untaken;if(!t)return;ag.disabled=true;
    try{await invoke("session_send",{cardId:id,text:t});if(panel&&panel.id===id)refreshInfo()}catch(e){ag.disabled=false;if(panel&&panel.id===id)foot(`<span class="err">Not sent: ${esc(errText(e))}</span>`)}};
  const dm=box.querySelector("#p-dismiss");if(dm)dm.onclick=()=>{const id=panel.id,t=panel.info&&panel.info.untaken;if(t)invoke("session_dismiss_untaken",{cardId:id,text:t}).then(()=>panel&&panel.id===id&&refreshInfo()).catch(()=>{})};
  const more=box.querySelector("#p-earlier");if(more)more.onclick=()=>{panel.shown+=SHOW;const h=box.scrollHeight;renderChat(false);box.scrollTop=box.scrollHeight-h};
  if(first||near)box.scrollTop=box.scrollHeight;
  runFind(true);
}

// A question Claude asked (AskUserQuestion): each question with its options. While it is the
// dialog on screen (session_info's question is its tool id), Hugh answers here: a click on an
// option answers a lone single-select question at once; otherwise he picks for each and
// presses Submit. The app types the dialog's keys into the terminal (session_answer). Once
// answered, here or in the terminal, the card shows the answers from the tool's result.
function askState(k,n){
  if(!panel.ask[k])panel.ask[k]={picks:Array.from({length:n},()=>[]),text:Array(n).fill(""),sent:false};
  return panel.ask[k];
}
function askLive(e){
  const info=panel.info||{};
  if(e.output||!info.running||info.state!=="permission"||info.question==null)return false;
  if(info.question!=="")return info.question===e.tool_id;
  const last=[...panel.entries].reverse().find(x=>x.ask);return last===e; // no id from the hook: the latest question
}
const askAuto=qs=>qs.length===1&&!qs[0].multiSelect; // one click answers it, as in the terminal
function askReady(qs,st){return qs.every((q,i)=>q.multiSelect?st.picks[i].length>0:(st.picks[i].length===1||!!st.text[i].trim()))}
// The answers from the tool's result, which Claude Code words more than one way, e.g.
//   Your questions have been answered: "Q"="A", "Q2"="B". You can now continue...
//   The user answered: "Q"="A". Read the answers carefully...
function askAnswers(text,qs){
  const at=qs.map(q=>text.indexOf(`"${q.question}"="`));
  if(at.some(i=>i<0))return null;
  return qs.map((q,i)=>{
    const from=at[i]+q.question.length+4,to=i+1<qs.length?at[i+1]:text.length,v=text.slice(from,to);
    if(i+1<qs.length)return v.replace(/",\s*$/,"");
    const end=v.search(/"\.(\s|$)/);return end>=0?v.slice(0,end):v.replace(/"\s*$/,"");
  });
}
function askHtml(e){
  const qs=e.ask,k=e.tool_id||"",live=askLive(e),st=askState(k,qs.length),o=e.output;
  const answers=o&&!o.is_error?askAnswers(o.text,qs):null;
  const tag=o?(o.is_error?"Claude asked · dismissed":"Claude asked · answered"):st.sent&&live?"Sending your answer…":live?"Claude is asking you":"Claude asked";
  let h=`<div class="m ask${live?" live":""}${o?" done":""}" data-ask="${esc(k)}">${stamp(e.at)}<div class="ptag">${esc(tag)}</div>`;
  qs.forEach((q,i)=>{
    h+=`<div class="q"><div class="qq">${q.header?`<span class="qh">${esc(q.header)}</span>`:""}${esc(q.question)}${q.multiSelect?` <span class="qm">pick any</span>`:""}</div>`;
    if(answers)h+=`<div class="qa">→ ${esc(answers[i])}</div>`;
    else if(!o){
      const off=!live||st.sent?" disabled":"";
      h+=`<div class="opts">${q.options.map((op,j)=>{const on=st.picks[i].includes(j);
        return`<button type="button" class="opt${on?" on":""}" data-q="${i}" data-o="${j}" aria-pressed="${on}"${off}><b>${esc(op.label)}</b>${op.description&&op.description!==op.label?`<span class="od">${esc(op.description)}</span>`:""}</button>`}).join("")}</div>`;
      if(live&&!q.multiSelect)h+=`<input class="qtext" data-q="${i}" type="text" autocomplete="off" placeholder="Or type your own answer${askAuto(qs)?" and press Enter":""}" aria-label="Your own answer to: ${esc(q.question)}" value="${esc(st.text[i])}"${off}>`;
    }
    h+=`</div>`;
  });
  if(o&&o.is_error)h+=`<div class="tnote">Dismissed in the terminal, so Claude stopped there.</div>`;
  else if(o&&!answers)h+=`<div class="qa">${esc(o.text)}</div>`;
  if(live&&!st.sent)h+=`<div class="pacts">${askAuto(qs)?"":`<button class="btn" type="button" data-ask-submit${askReady(qs,st)?"":" disabled"}>Submit answers</button>`}<span>${askAuto(qs)?"Click an answer, or answer in the terminal":"or answer in the terminal"}</span></div>`;
  else if(!o&&!live)h+=`<div class="tnote">${panel.info&&panel.info.running&&panel.info.state==="permission"?"Answer it in the terminal.":"Not answered yet."}</div>`;
  return h+`</div>`;
}
async function askSubmit(k){
  if(!panel)return;const e=panel.entries.find(x=>x.ask&&(x.tool_id||"")===k);if(!e||!askLive(e))return;
  const st=askState(k,e.ask.length);if(st.sent||!askReady(e.ask,st))return;
  const answers=e.ask.map((q,i)=>!q.multiSelect&&st.text[i].trim()?{picks:[],text:st.text[i].trim()}:{picks:st.picks[i]});
  st.sent=true;renderChat(false);const id=panel.id;
  try{await invoke("session_answer",{cardId:id,toolId:k,answers})}
  catch(err){if(!panel||panel.id!==id)return;st.sent=false;foot(`<span class="err">Not answered: ${esc(errText(err))}</span>`);renderChat(false)}
}
function setupAsk(el){
  const box=el.querySelector("#p-msgs");
  const at=t=>{const c=t.closest("[data-ask]");if(!c)return null;const k=c.dataset.ask,e=panel.entries.find(x=>x.ask&&(x.tool_id||"")===k);return e?{k,e,st:askState(k,e.ask.length)}:null};
  box.addEventListener("click",ev=>{
    const b=ev.target.closest(".opt,[data-ask-submit]");if(!b||b.disabled||!panel)return;
    const a=at(b);if(!a||!askLive(a.e)||a.st.sent)return;
    if(b.hasAttribute("data-ask-submit")){askSubmit(a.k);return}
    const i=+b.dataset.q,j=+b.dataset.o,q=a.e.ask[i];
    if(q.multiSelect){const p=a.st.picks[i],n=p.indexOf(j);if(n>=0)p.splice(n,1);else p.push(j)}
    else{a.st.picks[i]=[j];a.st.text[i]=""}
    if(askAuto(a.e.ask)){askSubmit(a.k);return}
    renderChat(false);
  });
  box.addEventListener("input",ev=>{
    const t=ev.target;if(!t.classList||!t.classList.contains("qtext")||!panel)return;
    const a=at(t);if(!a)return;const i=+t.dataset.q;a.st.text[i]=t.value;
    if(t.value.trim()&&a.st.picks[i].length){a.st.picks[i]=[];t.closest(".q").querySelectorAll(".opt.on").forEach(o=>{o.classList.remove("on");o.setAttribute("aria-pressed","false")})}
    const s=t.closest("[data-ask]").querySelector("[data-ask-submit]");if(s)s.disabled=!askReady(a.e.ask,a.st);
  });
  box.addEventListener("keydown",ev=>{
    const t=ev.target;if(ev.key!=="Enter"||ev.isComposing||!t.classList||!t.classList.contains("qtext")||!panel)return;
    ev.preventDefault();const a=at(t);if(a&&a.st.text[+t.dataset.q].trim())askSubmit(a.k);
  });
}

// Subagents (card #91): the strip above the chat holds Main and each subagent the session ran,
// newest first, read from Claude Code's subagents folder beside the transcript (subagent_list).
// A click shows that subagent's own conversation, read-only; Main goes back. An Agent call in
// the main chat links to the subagent it started. The box below always talks to the main session.
async function refreshSubs(){
  if(!panel)return;const id=panel.id;
  let list;try{list=await invoke("subagent_list",{cardId:id})}catch(e){return}
  if(!panel||panel.id!==id)return;
  const key=JSON.stringify(list);if(key===panel.subsKey)return;
  const sig=l=>JSON.stringify(l.map(s=>[s.id,s.tool_id,s.finished,s.agent_type,s.description]));
  const relink=sig(list)!==sig(panel.subs);
  panel.subsKey=key;panel.subs=list;
  if(panel.view&&!list.some(s=>s.id===panel.view)){showView("");return}
  renderSubs();
  if(relink)renderChat(false);
}
// Running: Claude Code hasn't marked it finished and the card's session is up. One whose
// session ended before it finished reads Stopped.
function subRunning(s){
  const run=panel&&panel.info&&panel.info.running;if(s.finished||!run)return false;
  // Unfinished from an earlier run of a resumed session: it isn't coming back.
  const since=Date.parse(run.started_at||run.startedAt||""),at=Date.parse(s.updated||"");
  return isNaN(since)||isNaN(at)||at>=since-5000;
}
function subState(s){return s.finished?"done":subRunning(s)?"running":"stopped"}
const SUB_LABEL={done:"Finished",running:"Running",stopped:"Stopped"};
function subDot(s){const st=subState(s);return`<span class="subdot ${st}" title="${SUB_LABEL[st]}"></span>`}
function subDur(s){const a=Date.parse(s.started||""),b=subRunning(s)?Date.now():Date.parse(s.updated||"");return isNaN(a)||isNaN(b)?"":fmtDur(b-a)}
function subNote(s){
  const back=`<button class="ibtn" type="button" data-agent="">Back to main</button>`;
  if(!s)return`<div class="subnote"><span>Reading this subagent's details…</span>${back}</div>`;
  const d=subDur(s),st=subState(s);
  return`<div class="subnote">${subDot(s)}<span><b>${esc(s.agent_type)}</b>${s.description?` · ${esc(s.description)}`:""} · ${SUB_LABEL[st]}${d?` · ${esc(d)}`:""}<br><small>Its own conversation, read-only. The box below still talks to the main session.</small></span>${back}</div>`;
}
function renderSubs(){
  if(!panel)return;const el=panel.el.querySelector("#p-subs");if(!el)return;
  if(!panel.subs.length&&!panel.view){panel.subsHtml="";if(el.innerHTML)el.innerHTML="";return}
  const tab=(id,html,title)=>`<button class="sub${panel.view===id?" on":""}" type="button" role="tab" aria-selected="${panel.view===id}" data-agent="${esc(id)}" title="${esc(title)}">${html}</button>`;
  const run=!!(panel.info&&panel.info.running);
  const out=[tab("",`<span class="subdot ${run?"running":"stopped"}"></span><b>Main</b>`,"The card's own conversation")];
  for(const s of [...panel.subs].reverse()){const d=subDur(s);
    out.push(tab(s.id,`${subDot(s)}<b>${esc(s.agent_type)}</b>${s.description?`<span class="sd">${esc(s.description)}</span>`:""}${d?`<span class="sdur">${esc(d)}</span>`:""}`,`${s.agent_type}: ${s.description||""} (${SUB_LABEL[subState(s)]})`))}
  const html=out.join("");if(html!==panel.subsHtml){panel.subsHtml=html;el.innerHTML=html}
  // In a subagent's view, its note's time moves on too.
  if(panel.view){const n=panel.el.querySelector("#p-msgs .subnote"),s=panel.subs.find(x=>x.id===panel.view);if(n&&s)n.outerHTML=subNote(s)}
}
function showView(id){
  if(!panel||panel.view===id)return;
  panel.view=id;panel.gen++;panel.reading=null;panel.entries=[];panel.tools={};panel.offset=0;panel.queued=[];panel.shown=SHOW;panel.outboxKey=null;
  if(!id)panel.lastMainUser=null;
  renderSubs();
  panel.el.querySelector("#p-msgs").innerHTML=`<div class="quiet">Reading the conversation…</div>`;
  pollTranscript();
}
// A /btw side question and its answer (session_info's btw, read off the session's screen). Not
// part of the conversation: Claude Code keeps it out of the transcript, and the app keeps it
// only until it restarts.
function btwHtml(b){
  const body=b.state==="asking"?`<div class="btwa dim">Answering…</div>`
    :b.state==="failed"?`<div class="btwa err">No answer: ${esc(b.text)}</div>`
    :`<div class="btwa">${esc(b.text)}</div>`;
  return`<div class="m btw">${stamp(b.at)}<div class="btwq"><span class="btwtag" title="A side question: Claude answered it without interrupting its work, and it isn't part of the conversation">/btw</span> ${esc(b.question)}</div>${body}</div>`;
}
// A background task's notice in the main chat (transcript::task_notice): one line, and a link
// to the subagent when it was one.
function taskHtml(e){
  const s=e.tool_id&&panel.subs.find(x=>x.id===e.tool_id);
  const bad=e.input&&e.input!=="completed";
  return`<div class="m task${bad?" bad":""}">${stamp(e.at)}<span>${s?subDot(s):""}${esc(e.text)}${bad?` <em>(${esc(e.input)})</em>`:""}</span>${s?` <button class="subopen" type="button" data-agent="${esc(s.id)}" title="Show this subagent's own conversation">View subagent</button>`:""}</div>`;
}
function setupSubs(el){
  // The strip, the note's Back button, and the links on Agent calls (inside a summary, so the
  // click mustn't also fold the tool open or shut).
  el.addEventListener("click",e=>{
    const b=e.target.closest("#p-subs [data-agent], .subnote [data-agent], .subopen[data-agent]");
    if(!b||!panel)return;
    e.preventDefault();e.stopPropagation();showView(b.dataset.agent);
  },true);
}

// One tool call: its name and detail; opened, its key input and the first lines of its result.
const OUT_LINES=8;
function toolHtml(i){
  const [n,d]=i.text.split("\t");
  const k=i.tool_id||"";
  const o=i.output;
  let body=i.input?`<pre class="tin">${esc(i.input)}</pre>`:"";
  if(o){
    const shown=o.text.split("\n").length;
    const long=shown>OUT_LINES||o.text.length>OUT_LINES*120;
    const open=panel.more.has(k);
    body+=`<pre class="tout${o.is_error?" err":""}${long&&!open?" clip":""}">${o.text?esc(o.text):"<i>(no output)</i>"}</pre>`;
    if(long)body+=`<button class="more" type="button" data-more="${esc(k)}">${open?"Show less":`Show more (${o.lines} line${o.lines===1?"":"s"})`}</button>`;
    if(o.truncated)body+=`<div class="tnote">The result is cut to its first 4 KB.</div>`;
  }else body+=`<div class="tnote">No result yet.</div>`;
  if(i.images&&i.images.length&&!(o&&o.is_error))body+=`<div class="shots">${i.images.map(p=>shotImg(p,i.at)).join("")}</div>`;
  const tk="t:"+k;
  return`<details class="tool" data-k="${esc(tk)}"${k&&panel.open.has(tk)?" open":""}><summary><b>${esc(n)}</b>${d?` <span>${esc(d)}</span>`:""}${o&&o.is_error?` <em class="terr">error</em>`:""}</summary><div class="tbody">${body}</div></details>`;
}
// Which tool groups and tools are open, and which results are shown in full, survive a redraw.
function setupToolToggles(el){
  const box=el.querySelector("#p-msgs");
  box.addEventListener("toggle",e=>{const k=e.target.dataset&&e.target.dataset.k;if(!k||!panel)return;if(e.target.open){panel.open.add(k);showShots(e.target)}else panel.open.delete(k)},true);
  box.addEventListener("click",e=>{
    const shot=e.target.closest("img.shot[src]");if(shot){e.preventDefault();openShot(shot);return}
    const b=e.target.closest("button.more");if(!b||!panel)return;
    const k=b.dataset.more,pre=b.previousElementSibling;
    const out=panel.tools[k]&&panel.tools[k].output;
    if(pre.classList.toggle("clip")){panel.more.delete(k);b.textContent=out?`Show more (${out.lines} line${out.lines===1?"":"s"})`:"Show more"}
    else{panel.more.add(k);b.textContent="Show less"}
  });
}

// Search: marks every match in the drawn conversation; Enter / Shift+Enter or the arrows move
// between them, opening any folded tool group a match is in. Esc clears.
function setupFind(el){
  const q=el.querySelector("#p-q");
  q.addEventListener("input",()=>{clearTimeout(panel.findTimer);panel.findTimer=setTimeout(()=>setQuery(q.value),120)});
  q.addEventListener("keydown",e=>{
    if(e.key==="Escape"){e.preventDefault();e.stopPropagation();if(q.value){q.value="";setQuery("")}else q.blur();return}
    if(e.key==="Enter"||e.key==="ArrowDown"||e.key==="ArrowUp"){
      e.preventDefault();
      if(q.value!==panel.find.q){clearTimeout(panel.findTimer);setQuery(q.value);return}
      step(e.key==="ArrowUp"||(e.key==="Enter"&&e.shiftKey)?-1:1);
    }
  });
  el.querySelector("#p-prev").onclick=()=>step(-1);
  el.querySelector("#p-next").onclick=()=>step(1);
}
function setQuery(v){
  if(!panel)return;
  const was=!!panel.find.q;panel.find.q=v.trim();panel.find.i=-1;
  // Searching draws the whole conversation; clearing goes back to the newest part.
  if(!!panel.find.q!==was&&panel.entries.length)renderChat(!panel.find.q);else runFind(false);
  if(panel.find.hits.length)goHit(panel.find.hits.length-1,true); // the newest match first
}
function step(d){if(panel&&panel.find.hits.length)goHit(panel.find.i<0?(d>0?0:panel.find.hits.length-1):panel.find.i+d,true)}
function clearMarks(box){
  const marks=box.querySelectorAll("mark.hit");if(!marks.length)return;
  for(const m of marks)m.replaceWith(document.createTextNode(m.textContent));
  box.normalize();
}
// Marks the matches in what is drawn. `keep` holds the current match's place after a redraw.
function runFind(keep){
  if(!panel)return;
  const box=panel.el.querySelector("#p-msgs"),f=panel.find;
  clearMarks(box);f.hits=[];
  if(f.q){
    const re=new RegExp(f.q.replace(/[.*+?^${}()|[\]\\]/g,"\\$&"),"gi");
    const walk=document.createTreeWalker(box,NodeFilter.SHOW_TEXT,{acceptNode:n=>n.parentElement.closest("button,.ts")?NodeFilter.FILTER_REJECT:NodeFilter.FILTER_ACCEPT});
    const nodes=[];while(walk.nextNode())nodes.push(walk.currentNode);
    for(const n of nodes){
      const s=n.nodeValue;re.lastIndex=0;let m,last=0;const frag=document.createDocumentFragment();let any=false;
      while((m=re.exec(s))){any=true;
        if(m.index>last)frag.append(s.slice(last,m.index));
        const mk=document.createElement("mark");mk.className="hit";mk.textContent=m[0];frag.append(mk);
        last=m.index+m[0].length;if(!m[0].length)re.lastIndex++;
      }
      if(any){if(last<s.length)frag.append(s.slice(last));n.replaceWith(frag)}
    }
    f.hits=[...box.querySelectorAll("mark.hit")];
  }
  if(keep&&f.i>=0&&f.hits.length)goHit(Math.min(f.i,f.hits.length-1),false);
  else{f.i=-1;findCount()}
}
function goHit(i,scroll){
  const f=panel.find,n=f.hits.length;if(!n)return;
  f.i=((i%n)+n)%n;
  f.hits.forEach(h=>h.classList.remove("cur"));
  const m=f.hits[f.i];m.classList.add("cur");
  if(scroll){
    // Open whatever hides the match: its tool group, its tool, a clipped result.
    const box=panel.el.querySelector("#p-msgs");
    for(let p=m.parentElement;p&&p!==box;p=p.parentElement){
      if(p.tagName==="DETAILS"&&!p.open)p.open=true;
      if(p.classList.contains("clip")){p.classList.remove("clip");const b=p.nextElementSibling;if(b&&b.dataset.more){panel.more.add(b.dataset.more);b.textContent="Show less"}}
    }
    m.scrollIntoView({block:"center"});
  }
  findCount();
}
function findCount(){
  if(!panel)return;const f=panel.find,el=panel.el;
  el.querySelector("#p-cnt").textContent=f.q?(f.hits.length?`${f.i+1>0?f.i+1:0} of ${f.hits.length}`:"No matches"):"";
  el.querySelector("#p-prev").disabled=el.querySelector("#p-next").disabled=!f.hits.length;
}

// Attachments: pasted images and dropped files are saved in the app's data folder. On send, the message ends with an "[Attached: <path>]" line for each, for Claude to Read.
const IMG_EXT=/\.(png|jpe?g|gif|webp|bmp)$/i;
function withAttachments(text,paths){
  if(!paths.length)return text;
  const tail=paths.map(p=>`[Attached: ${p}]`).join("\n");
  return text.trim()?`${text.replace(/\s+$/,"")}\n\n${tail}`:tail;
}
const hasFiles=e=>!!(e.dataTransfer&&[...e.dataTransfer.types].includes("Files"));
const baseName=p=>p.split(/[\\/]/).pop()||p;
function readDataUrl(file){return new Promise((ok,bad)=>{const r=new FileReader();r.onload=()=>ok(r.result);r.onerror=()=>bad(r.error);r.readAsDataURL(file)})}
function onPaste(e){
  const files=[...(e.clipboardData?e.clipboardData.files:[])];
  if(!files.length)return;
  // Keep any text that came with the image; only an image-only paste is taken over.
  if(!e.clipboardData.getData("text/plain"))e.preventDefault();
  addFiles(files);
}
// Files with no path on disk (pasted, or dropped from the page): their bytes are saved first.
async function addFiles(files){
  if(!panel)return;const id=panel.id;
  for(const f of files){
    const a={path:null,name:f.name||"screenshot.png",thumb:null};
    panel.att.push(a);renderAtts();
    try{
      if(f.size>25*1024*1024)throw new Error("it is over 25 MB");
      const data=await readDataUrl(f);
      if((f.type||"").startsWith("image/"))a.thumb=data;
      const name=f.name&&f.name!=="image.png"?f.name:`screenshot${(f.type==="image/jpeg")?".jpg":".png"}`;
      a.path=await invoke("attachment_save",{cardId:id,name,data});a.name=baseName(a.path);
    }catch(err){if(panel&&panel.id===id){panel.att=panel.att.filter(x=>x!==a);foot(`<span class="err">Couldn't attach ${esc(a.name)}: ${esc(errText(err))}</span>`)}}
    if(panel&&panel.id===id)renderAtts();
  }
}
function renderAtts(){
  if(!panel)return;const box=panel.el.querySelector("#p-atts");if(!box)return;
  box.innerHTML=panel.att.map((a,i)=>`<span class="att${a.path?"":" saving"}" title="${esc(a.path||"Saving…")}">${a.thumb?`<img src="${esc(a.thumb)}" alt="">`:`<span class="ficon" aria-hidden="true">${IMG_EXT.test(a.name)?"IMG":"FILE"}</span>`}<span class="nm">${esc(a.name)}</span><button type="button" class="rm" data-i="${i}" aria-label="Remove ${esc(a.name)}">✕</button></span>`).join("");
  box.querySelectorAll(".rm").forEach(b=>b.onclick=()=>{panel.att.splice(+b.dataset.i,1);renderAtts();const t=panel.el.querySelector("#p-text");if(t)t.focus()});
}
// Files dragged in from Explorer: the window's own drop handling is off (it broke the board's
// card drag), so they arrive as HTML5 drops with no path. Their bytes are saved like a paste.
// Anywhere on the panel takes the drop (the box is always there, running session or not);
// off the panel a dropped file is refused rather than opened in the window.
function setupDrop(id){
  const el=panel.el,back=panel.back,cbox=el.querySelector("#p-compose");
  const can=()=>!!cbox.querySelector("#p-text");
  el.addEventListener("dragover",e=>{if(!hasFiles(e))return;e.preventDefault();
    e.dataTransfer.dropEffect=can()?"copy":"none";cbox.classList.toggle("dropping",can())});
  el.addEventListener("dragleave",e=>{if(!el.contains(e.relatedTarget))cbox.classList.remove("dropping")});
  el.addEventListener("drop",e=>{if(!hasFiles(e))return;e.preventDefault();cbox.classList.remove("dropping");
    if(can()&&panel&&panel.id===id)addFiles([...e.dataTransfer.files]);
    else foot(`<span class="err">Start or resume the session to attach files.</span>`)});
  for(const n of["dragover","drop"])back.addEventListener(n,e=>{if(hasFiles(e)){e.preventDefault();e.dataTransfer.dropEffect="none"}});
}

// The "Working…" line under the conversation, as the terminal shows it while Claude is on a turn:
// how long the turn has run (session_info's turnMs, moved on by the clock) and how long the model
// last thought (the transcript's latest thinking block, if it came in this turn). While Claude is
// asking something it says so instead. Gone once the turn ends.
const BUSY_VERBS=["Working","Thinking","Mustering","Pondering","Brewing","Crafting","Musing","Percolating","Cogitating","Tinkering"];
const BUSY_GLYPHS=["·","✢","✳","✶","✻","✽","✻","✶","✳","✢"];
function noteTurn(info){
  if(!panel)return;
  if(!info.running||info.turnMs==null){panel.turn=null;renderBusy();return}
  const start=Date.now()-info.turnMs,t=panel.turn;
  // Same turn (its start agrees to within a few seconds): keep its word, take the fresher start.
  if(t&&Math.abs(t.start-start)<3000){t.start=start;t.state=info.state}
  else panel.turn={start,state:info.state,verb:BUSY_VERBS[Math.floor(Math.random()*BUSY_VERBS.length)]};
  renderBusy();
}
function renderBusy(){
  if(!panel)return;const el=panel.el.querySelector("#p-busy"),t=panel.turn;if(!el)return;
  if(!t){if(!el.hidden){el.hidden=true;el.innerHTML=""}return}
  const ms=Math.max(0,Date.now()-t.start),parts=[fmtDur(ms)];
  const th=panel.thought,at=th&&Date.parse(th.at);
  if(th&&th.ms!=null&&at>=t.start-2000)parts.push(`thought for ${Math.max(1,Math.round(th.ms/1000))}s`);
  const asking=t.state==="permission";
  const glyph=asking?"?":BUSY_GLYPHS[Math.floor(ms/1000)%BUSY_GLYPHS.length];
  const inChat=asking&&!panel.view&&panel.askOn!=null&&panel.entries.some(e=>e.ask&&!e.output);
  const what=asking?(inChat?"Waiting for you: answer the question above":"Waiting for you: answer in the terminal"):`${t.verb}…`;
  el.innerHTML=`<span class="g" aria-hidden="true">${glyph}</span> <span class="v">${esc(what)}</span> <span class="d">(${esc(parts.join(" · "))})</span>`;
  el.title=asking?(inChat?"Claude is asking you a question: answer it in the chat or the terminal":"Claude is asking you something in the session's terminal"):"Claude is working on a turn";
  el.classList.toggle("asking",asking);el.hidden=false;
}

// The session footer: context used, cost, time running, model and Claude Code version, from the
// session's latest statusLine report.
function refreshStatusSoon(){if(!panel||panel.statusSoon)return;panel.statusSoon=setTimeout(()=>{if(panel){panel.statusSoon=null;refreshStatus()}},800)}
async function refreshStatus(){
  if(!panel)return;const id=panel.id;
  let s;try{s=await invoke("session_status",{cardId:id})}catch(e){return}
  if(!panel||panel.id!==id)return;
  s.fetched=Date.now();panel.status=s;renderStatus();
}
function fmtTokens(n){return n>=1e6?(n/1e6).toFixed(1)+"M":n>=1e3?Math.round(n/1e3)+"k":String(Math.round(n))}
function fmtDur(ms){const s=Math.max(0,Math.floor(ms/1000)),h=Math.floor(s/3600),m=Math.floor(s%3600/60);return h?`${h}h ${m}m`:m?`${m}m ${s%60}s`:`${s}s`}
function renderStatus(){
  if(!panel)return;const el=panel.el.querySelector("#p-sfoot"),s=panel.status;
  if(!s||!s.row){el.hidden=true;return}
  const st=s.status||{},info=panel.info||{},parts=[];
  if(st.contextPct!=null){
    const pct=Math.round(st.contextPct);
    const tip=st.contextTokens!=null?`${fmtTokens(st.contextTokens)}${st.contextSize?` of ${fmtTokens(st.contextSize)}`:""} tokens`:"";
    parts.push(`<span class="ctx${pct>=80?" hot":""}" title="Context used${tip?": "+esc(tip):""}"><span class="bar"><i style="width:${Math.min(100,pct)}%"></i></span>${pct}% context</span>`);
  }else if(st.contextTokens!=null)parts.push(`<span title="Context used">${fmtTokens(st.contextTokens)} tokens</span>`);
  if(st.costUsd!=null)parts.push(`<span title="Cost so far">$${st.costUsd.toFixed(2)}</span>`);
  // Time running: the session's own count, moved on by the clock since the report while it runs.
  let ms=st.durationMs!=null?st.durationMs+(s.running&&st.at?Math.max(0,Date.now()-Date.parse(st.at)):0):null;
  if(ms==null&&s.running&&s.startedAt){const t=Date.parse(s.startedAt);if(!isNaN(t))ms=Date.now()-t}
  if(ms!=null)parts.push(`<span title="Time running">${fmtDur(ms)}</span>`);
  const model=st.model||s.model||info.model;if(model)parts.push(`<span title="${esc(st.modelId||"Model")}">${esc(model)}</span>`);
  const ver=st.version||info.claudeVersion;if(ver)parts.push(`<span title="Claude Code version">v${esc(String(ver).replace(/^v/,""))}</span>`);
  if(!s.status)parts.push(`<span class="dim">${s.running?"waiting for the session's first status report":"no status reported this run of the app"}</span>`);
  el.innerHTML=parts.join("");el.hidden=false;
}

// The terminal: xterm.js on the session's PTY. Opening it replays what the session has drawn so far.
function toggleTerm(){
  const btn=panel.el.querySelector("#p-term"),body=panel.el.querySelector("#p-work");
  if(panel.term){termUnlink(panel.link);panel.link=null;panel.term.dispose();panel.term=null;panel.el.querySelector(".term").remove();const sp=body.querySelector(".split");if(sp)sp.remove();body.classList.remove("with-term");btn.setAttribute("aria-pressed","false");return}
  body.classList.add("with-term");btn.setAttribute("aria-pressed","true");
  const host=document.createElement("div");host.className="term";body.appendChild(host);addSplit(body);
  const term=new Terminal({fontFamily:'"IBM Plex Mono", Consolas, monospace',fontSize:13,cursorBlink:true,scrollback:5000,macOptionIsMeta:IS_MAC,
    theme:{background:"#0f1117",foreground:"#e6e8ef"}});
  const fit=new FitAddon.FitAddon();term.loadAddon(fit);term.open(host);panel.term=term;panel.fit=fit;
  panel.link=termLink(term,panel.id,"No session is running for this card. Start or resume one below.");
  new ResizeObserver(()=>{if(panel&&panel.term===term){try{fit.fit()}catch(e){}if(panel.link&&panel.link.run)invoke("session_resize",{cardId:panel.id,cols:term.cols,rows:term.rows}).catch(()=>{})}}).observe(host);
  term.focus(); // so typing goes straight into it (card #95)
  attachTerm();
}
// termlink.js: replays the session once per open (or per new run), then follows its live output.
function attachTerm(){
  if(!panel||!panel.term||!panel.link)return;
  try{panel.fit.fit()}catch(e){}
  termAttach(panel.link);
}

// Session state from the hooks, shown as a dot on each card and in the panel's header.
let activity={};
const STATE_LABEL={starting:"Starting",idle:"Ready",working:"Working",permission:"Needs you",failed:"Turn failed",ended:"Ended"};
// "Building" on a card while its session works (Hugh, card #75); it stands in for the plan chip then.
// "Testing" instead while the session runs a review or a test: /code-review, or a subagent that
// tests, reviews or verifies (card #86); a background one shows it after the turn ends too.
let testing=new Set();
async function loadTesting(){try{testing=new Set(await invoke("session_testing"))}catch(e){}}
function buildingChip(id){const s=activity[id];
  if(testing.has(id)&&(s==="working"||s==="idle"))return`<span class="tag chip testing" title="Session: running a review or tests">Testing</span>`;
  return s==="working"?`<span class="tag chip building" title="Session: working">Building</span>`:""}
function stateDot(id){const s=activity[id];return s?`<span class="sdot s-${esc(s)}" title="Session: ${esc(STATE_LABEL[s]||s)}"></span>`:""}
(async()=>{
  const ev=window.__TAURI__.event;
  ev.listen("testing",e=>{if(e.payload.on)testing.add(e.payload.card);else testing.delete(e.payload.card);render()});
  try{activity=await invoke("session_states")}catch(e){}
  await loadTesting();
  ev.listen("activity",e=>{activity[e.payload.card]=e.payload.state;render();if(panel&&panel.id===e.payload.card)refreshInfo()});
  ev.listen("session-changed",async()=>{try{activity=await invoke("session_states")}catch(e){}await loadTesting();render()});
  ev.listen("ship-request",e=>shipRequest(e.payload));
  ev.listen("open-card",e=>openPanel(e.payload)); // a toast was clicked
  render();
})();

// "Ship #n?" from `kb ship <n> --asked-by-hugh` in a session outside the app. Nothing ships
// until Hugh clicks Ship, which makes the same move as his drag, so the normal dispatch runs.
function shipRequest(p){
  if(!p||!p.card)return;
  const old=document.querySelector(`.shipask[data-card="${CSS.escape(p.card)}"]`);if(old)old.remove();
  const b=document.createElement("div");b.className="banner shipask";b.dataset.card=p.card;b.setAttribute("role","alert");
  b.innerHTML=`<span class="txt">Ship #${esc(p.number)} "${esc(p.title||"")}"? (asked from a session outside the app)</span><button class="btn" type="button" data-a="ship">Ship</button><button class="btn ghost" type="button" data-a="dismiss">Dismiss</button><span class="msg"></span>`;
  const anchor=$("#copyBanner");anchor?anchor.after(b):document.body.prepend(b);
  b.querySelector('[data-a="dismiss"]').onclick=()=>b.remove();
  b.querySelector('[data-a="ship"]').onclick=async()=>{
    const msg=t=>{b.querySelector(".msg").textContent=t};
    let c=cards.find(x=>x.id===p.card);
    if(!c){await load();c=cards.find(x=>x.id===p.card)}
    if(!c){msg("That card isn't on the board any more.");return}
    if(colOf(c)==="Ship it"){b.remove();status(`#${p.number} is already in Ship it`);return}
    const btns=b.querySelectorAll("button");btns.forEach(x=>x.disabled=true);
    let r=null;
    try{r=await call("move_card",{id:c.id,version:c.version,column:"Ship it",extra:{}},true)}
    catch(e){await load();msg("The card changed meanwhile; click Ship again.");btns.forEach(x=>x.disabled=false);return}
    if(!r){msg("It didn't move; try again.");btns.forEach(x=>x.disabled=false);return}
    b.remove();status(r.shipWaitFor?`Moved #${p.number} to Ship it: it waits for #${r.shipWaitFor} to finish shipping`:`Moved #${p.number} to Ship it: the card's session is told to ship it`);
  };
}

// The command bar: Ctrl+K, then "34", "34 ship it", "34 plan it" or "34 to Blocked".
function openCommand(){
  if($("#cmdbar"))return;
  const bar=document.createElement("div");bar.id="cmdbar";bar.className="cmdbar";
  bar.innerHTML=`<input id="cmd-in" placeholder="34 · 34 ship it · 34 plan it · 34 to Blocked" aria-label="Card command" autocomplete="off"><span class="msg" id="cmd-msg"></span>`;
  document.body.appendChild(bar);const inp=$("#cmd-in");inp.focus();
  const close=()=>bar.remove();
  inp.addEventListener("keydown",async e=>{
    if(e.key==="Escape"){close();return}
    if(e.key!=="Enter")return;e.preventDefault();
    const msg=t=>{$("#cmd-msg").textContent=t;$("#cmd-msg").className="msg err"};
    if(/^(settings?|prefs|preferences)$/i.test(inp.value.trim())&&typeof openSettings==="function"){close();openSettings();return}
    const m=inp.value.trim().match(/^(\d+)\s*(.*)$/i);if(!m){msg("Start with a card number.");return}
    const c=cards.find(x=>cardNum(x)===+m[1]);if(!c){msg(`There's no card ${m[1]}.`);return}
    const rest=m[2].trim().toLowerCase();
    let col=null;
    if(!rest){close();openPanel(c.id);return}
    if(/^ship( it)?$/.test(rest))col="Ship it";
    else if(/^(plan|start)( it)?$/.test(rest))col="Planning";
    else{const t=rest.replace(/^(move )?to /,"");col=COLUMNS.find(x=>x.toLowerCase()===t)||null}
    if(!col){msg(`I don't know "${m[2]}". Try ship it, plan it, or to <column>.`);return}
    if(colOf(c)===col){msg(`#${m[1]} is already in ${col}.`);return}
    close();await moveCard(c.id,col,{});
  });
}
document.addEventListener("keydown",e=>{if(modKey(e)&&e.key.toLowerCase()==="k"){e.preventDefault();openCommand()}});

// Moving and resizing the panel: drag its header to move it, any edge or corner to resize it,
// and double-click the header to fill the window again. Its place is remembered as shares of
// the window ("panel.rect"), so it keeps its shape when the window changes size. The bar
// between chat and terminal sets the chat's width.
const store_=(k,v)=>{try{if(v===undefined)return localStorage.getItem(k);localStorage.setItem(k,v)}catch(e){return null}};
const P_GAP=14,P_MINW=480,P_MINH=320;
function panelRect(){
  let r=null;try{r=JSON.parse(store_("panel.rect")||"null")}catch(e){}
  if(!r){const f=parseFloat(store_("panel.left")||"0");if(f>0)r={x:Math.min(f,0.75),y:0,w:1-Math.min(f,0.75),h:1,edgeOnly:true}}
  return r;
}
function fitRect(x,y,w,h){
  const W=innerWidth,H=innerHeight;
  w=Math.max(Math.min(P_MINW,W-2*P_GAP),Math.min(w,W-2*P_GAP));h=Math.max(Math.min(P_MINH,H-2*P_GAP),Math.min(h,H-2*P_GAP));
  x=Math.max(P_GAP,Math.min(x,W-P_GAP-w));y=Math.max(P_GAP,Math.min(y,H-P_GAP-h));return {x,y,w,h};
}
function setPanelBox(el,b){
  if(!b){el.style.left=el.style.top=el.style.width=el.style.height="";el.style.right=el.style.bottom="";return}
  Object.assign(el.style,{left:b.x+"px",top:b.y+"px",width:b.w+"px",height:b.h+"px",right:"auto",bottom:"auto"});
}
function applyPanelLeft(el){
  const r=panelRect();if(!r){setPanelBox(el,null);return}
  const W=innerWidth,H=innerHeight;
  const b=r.edgeOnly?fitRect(r.x*W,P_GAP,W-P_GAP-r.x*W,H-2*P_GAP):fitRect(r.x*W,r.y*H,r.w*W,r.h*H);
  setPanelBox(el,b);
}
function savePanelBox(b){const W=innerWidth,H=innerHeight;store_("panel.rect",JSON.stringify({x:b.x/W,y:b.y/H,w:b.w/W,h:b.h/H}))}
function dragging(onMove,cursor){
  document.body.classList.add("resizing");document.body.style.cursor=cursor;
  const move=e=>onMove(e.clientX,e.clientY),up=()=>{document.body.classList.remove("resizing");document.body.style.cursor="";
    removeEventListener("mousemove",move);removeEventListener("mouseup",up)};
  addEventListener("mousemove",move);addEventListener("mouseup",up);
}
const P_CURSORS={n:"ns-resize",s:"ns-resize",e:"ew-resize",w:"ew-resize",ne:"nesw-resize",sw:"nesw-resize",nw:"nwse-resize",se:"nwse-resize"};
function addPanelEdge(el){
  for(const d of Object.keys(P_CURSORS)){
    const h=document.createElement("div");h.className="rz rz-"+d;h.title="Drag to resize";
    h.addEventListener("mousedown",e=>{e.preventDefault();e.stopPropagation();
      const r0=el.getBoundingClientRect(),x0=e.clientX,y0=e.clientY;
      dragging((x,y)=>{const dx=x-x0,dy=y-y0;let {left:l,top:t,width:w,height:hh}=r0;
        if(d.includes("e"))w=r0.width+dx;
        if(d.includes("s"))hh=r0.height+dy;
        if(d.includes("w")){w=r0.width-dx;l=r0.left+dx;if(w<P_MINW){l-=P_MINW-w;w=P_MINW}}
        if(d.includes("n")){hh=r0.height-dy;t=r0.top+dy;if(hh<P_MINH){t-=P_MINH-hh;hh=P_MINH}}
        const b=fitRect(l,t,w,hh);setPanelBox(el,b);savePanelBox(b)},P_CURSORS[d])});
    el.appendChild(h);
  }
  const head=el.querySelector(".ph");
  if(head){
    head.title="Drag to move. Double-click to fill the window.";
    head.addEventListener("mousedown",e=>{
      if(e.button!==0||e.target.closest("button,a,input,select,textarea,label"))return;e.preventDefault();
      const r0=el.getBoundingClientRect(),x0=e.clientX,y0=e.clientY;
      dragging((x,y)=>{const b=fitRect(r0.left+x-x0,r0.top+y-y0,r0.width,r0.height);setPanelBox(el,b);savePanelBox(b)},"move")});
    head.addEventListener("dblclick",e=>{if(e.target.closest("button,a,input,select,textarea,label"))return;
      store_("panel.rect","");store_("panel.left","0");setPanelBox(el,null)});
  }
  applyPanelLeft(el);
}
function addSplit(body){
  const chat=body.querySelector(".chat"),term=body.querySelector(".term");if(!chat||!term)return;
  const bar=document.createElement("div");bar.className="split";bar.title="Drag to change the chat's width";body.insertBefore(bar,term);
  const saved=parseFloat(store_("panel.chat")||"0");if(saved>0)body.style.setProperty("--chatw",Math.round(saved*body.clientWidth)+"px");
  bar.addEventListener("mousedown",e=>{e.preventDefault();bar.classList.add("on");const r=body.getBoundingClientRect();
    dragging(x=>{const w=Math.max(220,Math.min(x-r.left,r.width-226));body.style.setProperty("--chatw",w+"px");store_("panel.chat",String(w/r.width))},"col-resize");
    addEventListener("mouseup",()=>bar.classList.remove("on"),{once:true})});
}
// The bar between the details and the chat sets the details' width, remembered in pixels.
const DET_MIN=260,WORK_MIN=320;
function applyDetailWidth(body){
  const saved=parseFloat(store_("panel.details")||"0");
  if(saved>0)body.style.setProperty("--detw",Math.round(Math.max(DET_MIN,Math.min(saved,body.clientWidth-WORK_MIN)))+"px");
  else body.style.removeProperty("--detw");
}
function addDetailSplit(body){
  const bar=body.querySelector(".dsplit");if(!bar)return;applyDetailWidth(body);
  bar.addEventListener("mousedown",e=>{e.preventDefault();bar.classList.add("on");const r=body.getBoundingClientRect();
    dragging(x=>{const w=Math.max(DET_MIN,Math.min(x-r.left,r.width-WORK_MIN));body.style.setProperty("--detw",w+"px");store_("panel.details",String(Math.round(w)))},"col-resize");
    addEventListener("mouseup",()=>bar.classList.remove("on"),{once:true})});
  bar.addEventListener("dblclick",()=>{store_("panel.details","0");applyDetailWidth(body)});
}
addEventListener("resize",()=>{if(panel){applyPanelLeft(panel.el);const b=panel.el.querySelector("#p-body");if(b)applyDetailWidth(b)}});
