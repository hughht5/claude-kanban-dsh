// "Done when..." and its acceptance checks (card #105). A card's `doneWhen` is one line, its goal;
// `doneWhenChecks` is a list of {text, state, note?, by?, at?}, state "open", "met" or "failed".
// The store sets by and at when a state changes. This draws one line on the card face and a block
// in the panel, through board.js's addCardPart (slot 30). Uses board.js's colOf, isArch, esc, safe,
// ago, SECRET, cards, canWrite, write, verOf, said and loaded, and panel.js's activity.
const DW_DONE_COLS=["Human test","Ship it","Deploy complete","Done"];
const DW_STATES=["open","met","failed"];
const DW_WORD={blocked:"blocked",done:"done",waiting:"waiting on Hugh",working:"working",paused:"paused"};
const DW_MAX_CHECKS=10,DW_MAX_TEXT=160,DW_MAX_LINE=200;
const dwChecks=c=>Array.isArray(c&&c.doneWhenChecks)?c.doneWhenChecks.filter(k=>k&&typeof k==="object"):[];
const dwGoal=c=>typeof (c&&c.doneWhen)==="string"?c.doneWhen.trim():"";
const dwSession=c=>typeof activity!=="undefined"&&activity?activity[c.id]:undefined;
const dwState=k=>DW_STATES.includes(k.state)?k.state:"open";

// The card's status from its checks, column and session (Hugh's router fixed the rules): first match wins.
// sessionState is panel.js's activity[card.id]. Null when the card has no checks.
function doneWhenStatus(card,sessionState){
  const ks=dwChecks(card);if(!ks.length)return null;
  const col=colOf(card);
  if(col==="Blocked")return"blocked";
  if(ks.every(k=>k.state==="met")&&(DW_DONE_COLS.includes(col)||isArch(card)))return"done";
  if(col==="Human test")return"waiting";
  if(sessionState==="starting"||sessionState==="working")return"working";
  return"paused";
}
const dwDot=st=>st?`<span class="dw-dot dw-s-${esc(st)}" aria-hidden="true"></span>`:"";
const dwCount=ks=>({met:ks.filter(k=>k.state==="met").length,bad:ks.filter(k=>k.state==="failed").length,all:ks.length});

// The card face: one line, cut with an ellipsis; the counts never cut. Nothing on an archived card or a stack's lead.
function dwLine(c){
  if(isArch(c)||(Array.isArray(c.stack)&&c.stack.length))return"";
  const goal=dwGoal(c),ks=dwChecks(c);if(!goal&&!ks.length)return"";
  const st=doneWhenStatus(c,dwSession(c)),n=dwCount(ks);
  const plain=s=>SECRET.test(s)?"hidden: looks like a secret":s;
  const tip=[goal?"Done when "+plain(goal):"",st?`Status: ${DW_WORD[st]} · ${n.met} of ${n.all} checks met${n.bad?` · ${n.bad} failed`:""}`:""].filter(Boolean).join("\n");
  return`<span class="dw-line" title="${esc(tip)}">${dwDot(st)}<span class="dw-t">${goal?`<b>Done when</b> ${safe(goal)}`:"<b>Checks</b>"}</span>${n.all?`<span class="dw-n">· ${n.met}/${n.all}</span>`:""}${n.bad?`<span class="dw-n dw-bad">· ${n.bad} failed</span>`:""}</span>`;
}

// The panel block. renderDetail rebuilds it on every live update, so what Hugh is typing (and where
// his focus and cursor are) is read from the old block first and put back by dwWire.
let dwKeep=null;   // {id, dw (the "Done when" draft, or null when unchanged), add, focus (a data-dwk), s0, s1}
let dwSaid=null;   // {id, t, kind, at}: the block's last message, put back for a few seconds after the redraw
let dwRedraw=false; // true from dwPanel to dwWire: a blur then is the redraw, not Hugh leaving the box
let dwDrop=null;    // a card id: its "Done when" draft was a no-change save, so the next draw shows the saved line
function dwPanel(c){
  dwRedraw=true;queueMicrotask(()=>{dwRedraw=false}); // and if renderDetail stops before dwWire, after this task
  const old=document.getElementById("dw-box"),ae=document.activeElement;
  if(old&&old.dataset.id===c.id){
    const t=old.querySelector("#dw-text"),a=old.querySelector("#dw-add");
    const f=ae&&old.contains(ae)&&ae.dataset&&ae.dataset.dwk||null;
    let s0=null,s1=null;if(f&&(ae===t||ae===a)){try{s0=ae.selectionStart;s1=ae.selectionEnd}catch(e){}}
    // A draft is kept only while the saved line is the one it was typed over: once a save (Hugh's, or
    // anyone's) changes the line, the box shows the line as the store cleaned it.
    const newBase=SECRET.test(dwGoal(c))?"":dwGoal(c),drop=dwDrop===c.id;
    dwKeep={id:c.id,dw:!drop&&t&&t.value!==t.dataset.base&&t.dataset.base===newBase?t.value:null,add:a?a.value:"",focus:f,s0,s1};
  }else dwKeep=null;
  dwDrop=null;
  const ks=dwChecks(c),goal=dwGoal(c),st=doneWhenStatus(c,dwSession(c)),n=dwCount(ks),dis=canWrite?"":" disabled";
  const base=SECRET.test(goal)?"":goal; // a line that looks like a secret isn't put in the box, as with the summary
  const val=dwKeep&&dwKeep.dw!==null?dwKeep.dw:base;
  const rows=ks.map((k,i)=>{const s=dwState(k),at=Date.parse(k.at||"");
    const who=(s==="met"||s==="failed")&&(k.by||at)?`<div class="dw-by">${s}${k.by?` by ${safe(k.by)}`:""}${at?`, <span title="${esc(k.at)}">${esc(ago(at))}</span>`:""}</div>`:"";
    // The mark shows only in the Human test review view (review-mode.css, card #120): one per state, with its word for screen readers.
    const mk=`<span class="dw-mk" role="img" aria-label="${s}">${s==="met"?"✓":s==="failed"?"✗":"○"}</span>`;
    return`<li class="dw-ck dw-${s}"><label class="dw-lab">${mk}<input type="checkbox" data-ck="${i}" data-dwk="ck:${i}"${s==="met"?" checked":""}${dis}><span class="dw-ct">${safe(k.text)}</span>${s==="failed"?`<span class="dw-fail">failed</span>`:""}</label>`+
      `<button class="dw-x" type="button" data-rm="${i}" data-dwk="rm:${i}" aria-label="Remove this check" title="Remove this check"${dis}>✕</button>${who}${k.note?`<div class="dw-note">${safe(k.note)}</div>`:""}</li>`}).join("");
  return`<div class="shipbox dw-box" id="dw-box" data-id="${esc(c.id)}"><h3>Done when</h3>
   ${st?`<div class="dw-st">${dwDot(st)}<span>Status: ${esc(DW_WORD[st])} · ${n.met} of ${n.all} met${n.bad?` · <span class="dw-bad">${n.bad} failed</span>`:""}</span></div>`:""}
   ${!goal&&!ks.length?`<div class="dw-none">Nothing written yet</div>`:""}
   <input class="tx dw-text" id="dw-text" data-dwk="dw" data-base="${esc(base)}" value="${esc(val)}" placeholder="What finished looks like, in one line" aria-label="Done when" autocomplete="off"${dis}>
   ${goal&&!base?`<span class="hidden-secret">The saved line is hidden: it looks like a secret</span>`:""}
   ${ks.length?`<ul class="dw-list">${rows}</ul>`:""}
   <div class="dw-addrow"><input class="tx" id="dw-add" data-dwk="add" value="${esc(dwKeep?dwKeep.add:"")}" placeholder="Add a check" aria-label="Add a check" autocomplete="off"${dis}><button class="btn ghost" id="dw-addbtn" data-dwk="addbtn" type="button"${dis}>Add</button></div>
   <span class="msg dw-msg" id="dw-msg" aria-live="polite"></span></div>`;
}

function dwWire(c,d){
  dwRedraw=false;
  const box=d.querySelector("#dw-box");if(!box||box.dataset.id!==c.id)return;
  if(dwKeep&&dwKeep.id===c.id&&dwKeep.focus){const el=box.querySelector(`[data-dwk="${CSS.escape(dwKeep.focus)}"]`);
    if(el&&!el.disabled){el.focus({preventScroll:true});if(dwKeep.s0!==null&&el.type==="text"){try{el.setSelectionRange(dwKeep.s0,dwKeep.s1)}catch(e){}}}}
  if(dwSaid&&dwSaid.id===c.id&&Date.now()-dwSaid.at<5000)dwShow(dwSaid.t,dwSaid.kind);
  const t=box.querySelector("#dw-text"),a=box.querySelector("#dw-add");
  t.addEventListener("keydown",e=>{if(e.key==="Enter"){e.preventDefault();dwSaveLine(c.id,t)}});
  t.addEventListener("blur",()=>{if(!dwRedraw)dwSaveLine(c.id,t)});
  t.addEventListener("input",()=>dwMsg(c.id,""));
  a.addEventListener("keydown",e=>{if(e.key==="Enter"){e.preventDefault();dwAdd(c.id,a)}});
  a.addEventListener("input",()=>dwMsg(c.id,""));
  box.querySelector("#dw-addbtn").onclick=()=>{a.focus({preventScroll:true});dwAdd(c.id,a)}; // focus stays in the box, for the next check
  const shown=dwSend(c).map(k=>k.text); // each check's text as drawn, to find it if the list moves meanwhile
  box.querySelectorAll("input[data-ck]").forEach(k=>k.onchange=()=>dwEdit(c.id,+k.dataset.ck,shown[+k.dataset.ck],(list,j)=>{
    const s=list[j].state;
    if(k.checked&&s!=="met")list[j].state="met";        // an open or failed check is met
    else if(!k.checked&&s==="met")list[j].state="open"; // a met check is open again
    else return false;
    delete list[j].note})); // an older note (a tester's "why it failed") no longer fits the new state
  box.querySelectorAll("[data-rm]").forEach(b=>b.onclick=()=>dwEdit(c.id,+b.dataset.rm,shown[+b.dataset.rm],(list,j)=>{list.splice(j,1)}));
}

const dwShow=(t,kind)=>{const m=document.getElementById("dw-msg");if(m){m.textContent=t||"";m.className="msg dw-msg"+(kind?" "+kind:"")}};
function dwMsg(id,t,kind){dwSaid=t?{id,t,kind,at:Date.now()}:null;const b=document.getElementById("dw-box");if(b&&b.dataset.id===id)dwShow(t,kind)}
// write() says why a write was refused in the panel's message line; the block repeats it beside the box.
const dwWhy=()=>said&&said.kind==="err"&&Date.now()-said.at<3000?said.t:"Not saved. Try again.";
const dwCard=id=>cards.find(x=>x.id===id);
// The list as it is sent: {text, state} and the note if it has one. The store sets by and at.
const dwSend=c=>dwChecks(c).map(k=>{const o={text:String(k.text??""),state:dwState(k)};if(k.note)o.note=String(k.note);return o});

// One write at a time, each from the card as the board has it after the last: a second tick while
// the first is saving waits for the first's live update, so it never writes over it or meets a conflict.
let dwChain=Promise.resolve();
const dwQueue=fn=>(dwChain=dwChain.then(fn).catch(e=>console.error("Done when:",e)));
// Returns false when refused, "same" when the store found nothing to change (a repeat, or a line it
// had already cleaned), else true once the board has the new version (or after 3 s).
async function dwWrite(id,patch){
  const v0=verOf(id),r=await write(id,patch);
  if(!r)return false;
  const v=typeof r==="object"&&typeof r.version==="number"?r.version:null;
  if(v===v0)return"same";
  const t0=Date.now();while(verOf(id)===v0&&Date.now()-t0<3000)await new Promise(res=>setTimeout(res,40));
  return true;
}

function dwSaveLine(id,t){
  const v=t.value.trim();if(v===(t.dataset.base||"").trim()||t.dataset.sent===v)return;
  if(v.length>DW_MAX_LINE){dwMsg(id,`"Done when" is one line of at most ${DW_MAX_LINE} characters; this is ${v.length}.`,"err");return}
  t.dataset.sent=v;
  dwQueue(async()=>{const ok=await dwWrite(id,{doneWhen:v||null});
    if(ok==="same"){dwDrop=id;dwMsg(id,"No change","ok");renderDetail()}
    else if(ok)dwMsg(id,v?"Saved":"Removed","ok");else{delete t.dataset.sent;dwMsg(id,dwWhy(),"err")}});
}

// A change to one check: found by its place, or by its text if the list moved meanwhile. change(list, j)
// edits list[j] and returns false when there is nothing to write. A refused write redraws the block,
// so the tick box shows the card as it is.
function dwEdit(id,i,text,change){
  dwQueue(async()=>{const c=dwCard(id);if(!c)return;
    const list=dwSend(c),j=list[i]&&list[i].text===text?i:list.findIndex(k=>k.text===text);
    if(j<0){dwMsg(id,"That check changed meanwhile: look again.","err");renderDetail();return}
    if(change(list,j)===false)return;
    const ok=await dwWrite(id,{doneWhenChecks:list.length?list:null});
    if(ok==="same")renderDetail();else if(ok)dwMsg(id,"Saved","ok");else{dwMsg(id,dwWhy(),"err");renderDetail()}});
}

function dwAdd(id,a){
  const v=a.value.trim().replace(/\s+/g," ");if(!v)return;
  const c=dwCard(id);if(!c)return;
  if(v.length>DW_MAX_TEXT){dwMsg(id,`A check is at most ${DW_MAX_TEXT} characters; this one is ${v.length}.`,"err");return}
  if(dwChecks(c).length>=DW_MAX_CHECKS){dwMsg(id,`A card takes at most ${DW_MAX_CHECKS} checks.`,"err");return}
  if(SECRET.test(v)){dwMsg(id,"Not saved: that looks like it holds a token, key or password. Sign-ins and secrets never go on this board.","err");return}
  if(dwChecks(c).some(k=>String(k.text||"").toLowerCase()===v.toLowerCase())){dwMsg(id,"That check is already there","ok");a.value="";return}
  a.value="";
  dwQueue(async()=>{const k=dwCard(id);if(!k)return;const list=dwSend(k);
    if(list.length>=DW_MAX_CHECKS){dwMsg(id,`A card takes at most ${DW_MAX_CHECKS} checks.`,"err");dwPutBack(id,v);return}
    list.push({text:v,state:"open"});
    const ok=await dwWrite(id,{doneWhenChecks:list});
    if(ok==="same")dwMsg(id,"That check is already there","ok");else if(ok)dwMsg(id,"Added","ok");else{dwPutBack(id,v);dwMsg(id,dwWhy(),"err")}});
}
// A check that wasn't added goes back in the box, unless Hugh has started another.
function dwPutBack(id,v){const b=document.getElementById("dw-box"),a=b&&b.dataset.id===id?b.querySelector("#dw-add"):null;if(a&&!a.value)a.value=v}

addCardPart({at:30,line:dwLine,panel:dwPanel,wire:dwWire});
// If the board has already drawn its cards, draw them again with this part.
if(typeof loaded!=="undefined"&&loaded)render();
