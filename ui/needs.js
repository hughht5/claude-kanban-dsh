// "Needs you" cards and the 7-day usage badge. Uses board.js's helpers ($, esc, invoke, render, ago).
// A card needs Hugh when its session waits at a permission prompt, failed, or ended a turn he
// hasn't opened the card since (needs.rs). Such cards go to the top of their own column, newest
// first, with an icon. The header shows each account's 7-day usage (never the 5-hour figure).
let needs={}; // cardId -> {reason, detail, at, unread}

const NEED_ICON={
  permission:`<svg viewBox="0 0 16 16" aria-hidden="true"><circle cx="5" cy="8" r="3" fill="none" stroke="currentColor" stroke-width="1.8"/><path d="M8 8h6.5M12.2 8v2.6M14.5 8v2" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"/></svg>`,
  failed:`<svg viewBox="0 0 16 16" aria-hidden="true"><path d="M8 1.6 15.2 14.2H.8z" fill="currentColor"/><path d="M8 6v3.8" style="stroke:var(--panel)" stroke-width="1.7" stroke-linecap="round"/><circle cx="8" cy="12" r="1" style="fill:var(--panel)"/></svg>`,
  unread:`<svg viewBox="0 0 16 16" aria-hidden="true"><circle cx="8" cy="8" r="4.2" fill="currentColor"/></svg>`,
};
function needText(n){
  const why=n.reason==="permission"?(n.ask&&n.ask.kind==="question"?"asking you a question":"waiting at a permission prompt")
    :n.reason==="failed"?(n.detail==="crashed"?"its session crashed":"its session's turn failed")
    :"a reply you haven't read";
  const t=Date.parse(n.at||"");
  return `Needs you: ${why}${t?` (${ago(t)})`:""}${n.unread&&n.reason!=="unread"?"; also a reply you haven't read":""}`;
}
function needIcon(id){const n=needs[id];if(!n||!NEED_ICON[n.reason])return"";const t=esc(needText(n));
  return`<span class="need n-${esc(n.reason)}" role="img" aria-label="${t}" title="${t}">${NEED_ICON[n.reason]}</span>`}
const needClass=id=>needs[id]?" needs":"";
// Cards that need Hugh first (newest event first), the rest in the column's own order.
function pinFirst(list){
  const at=c=>Date.parse(needs[c.id].at||"")||0;
  const p=list.filter(c=>needs[c.id]).sort((a,b)=>at(b)-at(a));
  return p.length?p.concat(list.filter(c=>!needs[c.id])):list;
}

let needsTimer=null;
async function loadNeeds(){
  let n;try{n=await invoke("needs_you")}catch(e){return}
  if(JSON.stringify(n)===JSON.stringify(needs))return;
  needs=n||{};render();
}
// Several events often arrive together (a Stop is an activity and a needs change): one read for them.
const needsSoon=()=>{clearTimeout(needsTimer);needsTimer=setTimeout(loadNeeds,120)};

// The 7-day usage badge: "Main 42% · Account 2 17% (7-day)", amber above 80%, red above 95%.
const ACCTS=[["main","Main"],["account2","Account 2"]];
function usageEl(){
  let el=$("#usage");if(el)return el;
  el=document.createElement("span");el.id="usage";el.className="usage";el.hidden=true;
  const st=$("#status");st?st.after(el):$("header").appendChild(el);return el;
}
async function loadUsage(){
  let u;try{u=await invoke("usage_summary")}catch(e){return}
  const el=usageEl();
  const has=ACCTS.some(([k])=>u&&u[k]&&typeof u[k].sevenDay==="number");
  el.hidden=!has;if(!has)return;
  const part=([k,name])=>{const a=u[k]||{},p=a.sevenDay;
    if(typeof p!=="number")return`<span class="u">${name} —</span>`;
    const cls=p>95?" max":p>80?" hot":"";
    return`<span class="u${cls}">${name} ${Math.round(p)}%</span>`};
  const when=([k,name])=>{const t=Date.parse((u[k]||{}).at||"");return`${name}: ${t?"reported "+ago(t):"no report yet"}`};
  el.innerHTML=`${ACCTS.map(part).join(`<span class="sep">·</span>`)}<span class="win">(7-day)</span>`;
  el.title=`Each account's 7-day usage, from its sessions' status lines. ${ACCTS.map(when).join("; ")}.`;
}

(async()=>{
  const ev=window.__TAURI__&&window.__TAURI__.event;
  await Promise.all([loadNeeds(),loadUsage()]);
  if(ev)for(const name of ["activity","session-changed","cards-changed","needs-changed"])ev.listen(name,needsSoon);
  if(ev)ev.listen("usage-changed",()=>loadUsage());
  setInterval(loadUsage,60000);
  // Ages in the tooltips, and anything missed while the window slept.
  setInterval(loadNeeds,60000);
  // Back to the window with a card open: Hugh is reading it now.
  window.addEventListener("focus",()=>{if(typeof panel!=="undefined"&&panel)invoke("mark_card_read",{cardId:panel.id}).catch(()=>{})});
})();

// What a card waits on Hugh for (card #107): one line on its face, a box in its panel, the
// is:waiting filter term and the header's "Waiting on you · N" button. Through boardHooks only.
// The server's texts (needs.rs `ask`) come with secrets already hidden; they still go through safe().
// A card counts when it needs him (a question, a permission prompt, a failed session, an unread
// reply that ends in a question or says the plan is ready), or by its own fields: Blocked with a
// reason, a stack card asking a question, a ship that needs him, or Human test not yet passed.
// An unread reply that is a plain statement keeps only its dot.
const WAIT_LABEL={question:"Asks you",permission:"Permission",failed:"Failed",reply:"Replied",plan:"Plan ready",asks:"Asks you",blocked:"Blocked",ship:"Ship",test:"To test"};
// Secrets hidden in the page too, by the same rules as the server's needs::redact (keep the two
// alike): the card's own fields (a blocked reason, a stack card's question and choices, a ship's
// detail) never pass the server. A link keeps its host and first path segment; tokens, key/value
// secrets, Authorization headers and long opaque strings become [hidden]; anything still caught by
// the board's SECRET filter hides the whole text.
const HIDE_RE={
  url:/\b(https?:\/\/)(?:[^\s\/?#<>"'()\[\]@]+@)?([^\s\/?#<>"'()\[\]@]+)(\/[^\s\/?#<>"'()\[\]]*)?(\/[^\s?#<>"'()\[\]]*)?([?#][^\s<>"'()\[\]]*)?/gi,
  authorization:/\b(authorization)(["']?\s*[:=]\s*["']?)(?:[A-Za-z]+\s+)?[^\s"',;&<>}]+/gi,
  bearer:/\bbearer\s+[A-Za-z0-9._~+\/=-]+/gi,
  named:/(["']?)\b([A-Za-z0-9_.-]*(?:token|secret|key|password|passwd|auth|credential)[A-Za-z0-9_.-]*)(["']?)(\s*[:=]\s*)(["']?)[^\s"',;&<>}]+/gi,
  pair:/\b([A-Za-z0-9_-]*(?:oobcode|ct|sig|signature))=[^\s&"'<>]+/gi,
  token:/\b(?:gh[pousr]_[A-Za-z0-9]{10,}|github_pat_[A-Za-z0-9_]{10,}|glpat-[A-Za-z0-9_-]{10,}|AKIA[0-9A-Z]{16}|AIza[0-9A-Za-z_-]{10,}|sk-ant-[A-Za-z0-9_-]+|xox[abprs]-[A-Za-z0-9-]+|eyJ[A-Za-z0-9_-]{6,}(?:\.[A-Za-z0-9_-]*)*)/g,
  opaque:/[A-Za-z0-9_-]{32,}/g,
};
function hideSecrets(s){
  let t=String(s??"");
  t=t.replace(HIDE_RE.url,(m,sch,host,first,rest,query)=>`${sch}${host}${first||""}${rest&&rest.length>1?"/…":rest||""}${query?"?…":""}`);
  t=t.replace(HIDE_RE.authorization,"$1$2[hidden]").replace(HIDE_RE.bearer,"Bearer [hidden]").replace(HIDE_RE.named,"$1$2$3$4$5[hidden]")
    .replace(HIDE_RE.pair,"$1=[hidden]").replace(HIDE_RE.token,"[hidden]").replace(HIDE_RE.opaque,m=>/[A-Za-z]/.test(m)&&/\d/.test(m)?"[hidden]":m);
  t=t.replace(/\S+/g,w=>SECRET.test(w)?"[hidden]":w);
  return SECRET.test(t)?"details hidden":t;
}
const waitStr=v=>typeof v==="string"?hideSecrets(v.trim()):"";
function waitFor(c){
  if(!c||isArch(c)||colOf(c)==="Done")return null;
  const n=needs[c.id];
  if(n&&n.ask&&Object.hasOwn(WAIT_LABEL,n.ask.kind))
    return{kind:n.ask.kind,text:waitStr(n.ask.text),options:Array.isArray(n.ask.options)?n.ask.options.map(waitStr).filter(Boolean):[],more:Number(n.ask.more)>0?Number(n.ask.more):0};
  if(n&&n.reason==="failed")return{kind:"failed",text:n.detail==="crashed"?"its session crashed":"its turn failed",options:[],more:0};
  if(n&&n.reason==="permission")return{kind:"permission",text:"waiting at a permission prompt",options:[],more:0};
  const col=colOf(c),reason=waitStr(c.blockedReason),kid=Number(c.stackOf)>0;
  if(reason&&(col==="Blocked"||(kid&&!STK_READY.includes(col)))){
    const ch=Array.isArray(c.choices)?c.choices.map(waitStr).filter(Boolean):[];
    return{kind:ch.length?"asks":"blocked",text:reason,options:ch,more:0};
  }
  if(col==="Ship it"&&shipState(c)==="needs-hugh")return{kind:"ship",text:waitStr(c.ship&&c.ship.detail)||"it needs you to ship it",options:[],more:0};
  if(col==="Human test"&&c.passed!==true){
    // Card #103's estimate of how long it takes to check, when that card is in.
    let t="";if(typeof checkSizeText==="function"){const v=hookTry(checkSizeText,c);if(typeof v==="string")t=v.trim()}
    return{kind:"test",text:t,options:[],more:0};
  }
  return null;
}
// One line of text: "Asks you: Which colour? · Red / Blue and 2 more questions". `who` names the
// stack's cards it is about, straight after the label ("Asks you (#169 and 1 more card): ..."), so
// the card's two-line clip never cuts it off.
function waitPlain(w,who){
  const opts=w.options.length?` · ${w.options.join(" / ")}`:"";
  const more=w.more?` and ${w.more} more question${w.more===1?"":"s"}`:"";
  return`${WAIT_LABEL[w.kind]}${who?` (${who})`:""}${w.text?": "+w.text:""}${opts}${more}`;
}
function waitHtml(w,who,cls){
  const full=waitPlain(w,who);
  const tip=SECRET.test(full)?"":` title="${esc(full)}"`;
  const opts=w.options.length?`<span class="opt"> · ${w.options.map(safe).join(" / ")}</span>`:"";
  const more=w.more?`<span class="opt"> and ${w.more} more question${w.more===1?"":"s"}</span>`:"";
  return`<span class="${cls} w-${esc(w.kind)}"${tip}><b>${esc(WAIT_LABEL[w.kind])}</b>${who?` <span class="who">(${esc(who)})</span>`:""}${w.text?": "+safe(w.text):""}${opts}${more}</span>`;
}
const kidRef=k=>`#${cardNum(k)||""}`;
// A stack card with nothing of its own: its first card that waits, and how many more do. A card to
// test counts only when none of them asks anything else.
function stackWait(c){
  if(typeof isStackLead!=="function"||!isStackLead(c))return null;
  const all=stackKids(c).map(k=>({k,w:waitFor(k)})).filter(x=>x.w);
  const asks=all.filter(x=>x.w.kind!=="test"),ws=asks.length?asks:all;
  if(!ws.length)return null;
  const{k,w}=ws[0],n=ws.length-1;
  return{w,who:`${kidRef(k)}${n?` and ${n} more card${n===1?"":"s"}`:""}`};
}
function waitLine(c){
  const own=waitFor(c);
  if(own)return waitHtml(own,"","wait");
  const s=stackWait(c);
  return s?waitHtml(s.w,s.who,"wait"):"";
}
// The card face (card #120): a question, a permission prompt, a failed session or a plan to read is
// a box (the kind word as its label, the choices as plain text, not buttons); a card to test gets a
// "To test" chip in its chip row; the rest keep the one line. A stack card keeps the line.
const WAIT_BOX=["question","asks","permission","failed","plan"];
function waitBoxHtml(w){
  const full=waitPlain(w,"");
  const tip=SECRET.test(full)?"":` title="${esc(full)}"`;
  const opts=w.options.length?`<span class="wb-opts">${w.options.map(o=>`<span class="wb-opt">${safe(o)}</span>`).join("")}${w.more?`<span class="wb-more">and ${w.more} more question${w.more===1?"":"s"}</span>`:""}</span>`
    :w.more?`<span class="wb-opts"><span class="wb-more">and ${w.more} more question${w.more===1?"":"s"}</span></span>`:"";
  return`<div class="wbox w-${esc(w.kind)}"${tip}><span class="wb-t"><b class="wb-k">${esc(WAIT_LABEL[w.kind])}</b>${w.text?" "+safe(w.text):""}</span>${opts}</div>`;
}
function waitTestChip(w){
  const full=waitPlain(w,"");
  return`<span class="tag chip to-test" data-wait="test"${SECRET.test(full)?"":` title="${esc(full)}"`}>${esc(WAIT_LABEL.test)}</span>`;
}
boardHooks.decorate.push((b,c)=>{
  if(isArch(c))return;
  const top=b.querySelector(":scope > .stk-top"),t=top||b.querySelector(":scope > .t");if(!t)return;
  const own=top?null:waitFor(c);
  if(own&&WAIT_BOX.includes(own.kind)){t.insertAdjacentHTML("afterend",waitBoxHtml(own));return}
  if(own&&own.kind==="test"){const tags=b.querySelector(":scope > .tags");
    if(tags){const last=tags.lastElementChild;if(last&&last.tagName==="SPAN"&&last.dataset.meta==="ago")last.insertAdjacentHTML("beforebegin",waitTestChip(own));else tags.insertAdjacentHTML("beforeend",waitTestChip(own));return}}
  const html=waitLine(c);if(html)t.insertAdjacentHTML("afterend",html);
});
boardHooks.filters.push((c,t)=>t==="is:waiting"?!!waitFor(c):null);
boardHooks.details.push(c=>{
  const own=waitFor(c);
  const rows=own?[waitHtml(own,"","wrow")]:typeof isStackLead==="function"&&isStackLead(c)?stackKids(c).map(k=>{const w=waitFor(k);return w?waitHtml(w,kidRef(k),"wrow"):""}).filter(Boolean):[];
  if(!rows.length)return"";
  const how=own&&["question","permission","reply","plan"].includes(own.kind)?`<span class="meta">Answer it in the chat.</span>`:"";
  return`<div class="shipbox waitbox"><h3>Waiting on you</h3>${rows.join("")}${how}</div>`;
});

// The "Waiting on you · N" pill (card #120 moved it from the header into filterbar.js's row; same
// id, title and aria-pressed). It counts what is:waiting shows: a stack is one card however many of
// its cards wait. Queued here, since filterbar.js loads after this file.
(window.kbPills=window.kbPills||[]).push({key:"waiting",id:"waitBtn",label:"Waiting on you",tone:"ask",term:"is:waiting",order:10,
  title:"Show only the cards waiting on you. Click again to show them all. It combines with other filter terms.",
  deps:()=>needs,
  count:list=>list.filter(c=>fbTop(c)&&(waitFor(c)||(typeof isStackLead==="function"&&isStackLead(c)&&stackKids(c).some(waitFor)))).length});
