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
  const why=n.reason==="permission"?"waiting at a permission prompt"
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
