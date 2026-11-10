// A card's flow (cards #151 and #162): Hugh's per-card choice of the app's five subagents (card #162 took the card-merger off: every
// ship merges in its repo's merge lane, and an old card's `merger` is ignored) and of
// skipping Planning or Human test. Each has a default in Settings > Sessions, and each card is
// three-way: Default (follows Settings as it changes), On or Off (the card's own), shown as a
// switch with Default or Reset beside it (card #178). A card's
// `subagents` holds only its own overrides ({critic: false}); `skipPlanning` and `skipHumanTest`
// are true, false or absent. The Add card form, the card's "Card flow" box in its details (saved
// as soon as one changes, only that one), a chip on the card for each override that differs from
// the default, and the Settings defaults (settings.js) all use these lists. Only Hugh changes them:
// card sessions and the Router can't (kanban_update_card refuses the fields). Uses board.js's
// esc, invoke, cards, canWrite, write, status, render, renderDetail and boardHooks.
(()=>{
window.NEW_CARD_SUBAGENTS=[
  {card:"router",name:"card-router",key:"sessions.router_in_planning",label:"card-router: size the card first in Planning",
   help:"At the start of Planning the card's session has its card-router size the card (small, normal or split) and pick its model and effort. Off: the session plans with no route. Default on."},
  {card:"critic",name:"card-critic",key:"sessions.critic_before_build",label:"card-critic: critique the plan before building",
   help:"At the end of Planning, the card-critic checks the plan against the card and the code. It fails a plan only for a real problem (a dropped deliverable, a plan that would come out wrong, anything unsafe); smaller points come as notes after a pass, and go to the build with the In progress prompt. The session can move the card into In progress only after a pass, and not while the critic's question for you (a HUGH: line) is open; your own moves are never held. A card routed small gets one critique: after its FAIL it waits in Planning for you. On any other card, its third FAIL sends the card to Blocked. Off: the session skips the step. Default on."},
  {card:"reproducer",name:"card-reproducer",key:"sessions.repro_before_human_test",label:"card-reproducer: prove a reported bug before and after the fix",
   help:"A bug report on the card (Found a problem, Fail in Human test, Report a bug, or a tester's FAIL that names a bug) has the card-reproducer reproduce it before the fix and rerun the same steps after it. The session can move the card to Human test only once the latest run says Fixed (or Not reproduced). Off: reports are only logged. Default on."},
  {card:"reviewer",name:"card-reviewer",key:"sessions.review_before_human_test",label:"card-reviewer: review the diff before Human test",
   help:"The session can move the card from In progress to Human test only after the card-reviewer has passed its diff: mistakes, risky changes, work the card didn't ask for, the repo's hard rules. Its third FAIL sends the card to Blocked. Off: the session skips it. Default on."},
  {card:"tester",name:"card-tester",key:"sessions.test_before_human_test",label:"card-tester: test the card before Human test",
   help:"The session can move the card from In progress to Human test only after the card-tester has tested the real thing and passed it. The app records each verdict on the card. Off: the session tests it itself. Default on."},
];
const SUBS=window.NEW_CARD_SUBAGENTS;
// Card #162: skip Planning and skip Human test, each with its Settings default (off unless turned on).
window.CARD_FLOW_SKIPS=[
  {card:"skipPlanning",key:"sessions.skip_planning",label:"Skip Planning",chip:"No plan",offChip:"Planning on",
   help:"A move into Planning from Backlog or Blocked (your drag, Add card's Take it to, a blocked card starting by itself, the Router) goes straight to In progress instead: no plan request, no Planning session and no plan critique. The session is told to read the card, make its worktree if it has none, write a short approach into the summary, and build. Your own move into Planning from a later column still plans it. A stack's cards are planned by their lead, whatever this says. Default off."},
  {card:"skipHumanTest",key:"sessions.skip_human_test",label:"Skip Human test",chip:"No human test",offChip:"Human test on",
   help:"When the card's session (or the stack rules) moves it into Human test, after its subagent checks passed, the app moves it on into Ship it for you, as Take it to: Deploy complete does: that is your approval to ship it with no human test. It waits its turn in the ship queue like any ship, and a stack waits until every card in it is ready. Your own drag into Human test never skips, and Take it to: Human test still stops it there. A stack's cards go with their lead. Default off."},
];
const SKIPS=window.CARD_FLOW_SKIPS;
const isSkip=k=>SKIPS.some(s=>s.card===k);
// The Settings defaults, as {router: true, ..., skipPlanning: false, ...}: read at the start, and
// again whenever Hugh changes one.
let defaults=null;
async function loadDefaults(){
  const out={};
  await Promise.all([
    ...SUBS.map(async s=>{try{const g=await invoke("get_setting",{key:s.key});out[s.card]=g.value!=="off"}catch(e){out[s.card]=true}}),
    ...SKIPS.map(async s=>{try{const g=await invoke("get_setting",{key:s.key});out[s.card]=g.value==="on"}catch(e){out[s.card]=false}})]);
  defaults=out;return out}
const dflt=k=>defaults?!!defaults[k]:!isSkip(k);
const own=c=>c&&c.subagents&&typeof c.subagents==="object"&&!Array.isArray(c.subagents)?c.subagents:{};
// The card's own value for one option: true, false, or undefined (it follows the default).
const ownOf=(c,k)=>{const v=isSkip(k)?(c?c[k]:undefined):own(c)[k];return typeof v==="boolean"?v:undefined};
// A stack's card never skips: its lead's flow covers it.
const isChild=c=>!!c&&c.stackOf!=null&&c.stackOf!=="";
// The value that holds now: the card's own, else the default.
const nowOf=(c,k)=>{if(isSkip(k)&&isChild(c))return false;const v=ownOf(c,k);return v===undefined?dflt(k):v};
// The card's subagents as they hold now.
const subsOf=c=>{const o={};for(const s of SUBS)o[s.card]=nowOf(c,s.card);return o};
// Why a subagent is off on the card, for its row ("" when it is on): its own choice, or the default.
window.subagentOffText=(c,card)=>subsOf(c)[card]!==false?"":ownOf(c,card)!==undefined?"off for this card":"off by the default in Settings";
// Which subagent a verdict field belongs to (board.js's CHECKS), for its "off" note.
// (Card #162: the merge chip has none: a ship always merges in its lane.)
window.SUBAGENT_OF_CHECK={test:"tester",review:"reviewer",critique:"critic",repro:"reproducer"};

// The chip text for a subagent's override: "Critic off", "Tester on".
const subChip=(s,on)=>{const n=s.name.replace(/^card-/,"");return n.charAt(0).toUpperCase()+n.slice(1)+(on?" on":" off")};
// The card's overrides that differ from the default as it is now, as [{key, text, title}], in
// the board's order: the two skips, then the five subagents.
window.flowChips=c=>{
  if(!c||!defaults)return[];
  const out=[];
  for(const s of SKIPS){const v=ownOf(c,s.card);if(v===undefined||v===dflt(s.card)||isChild(c))continue;
    out.push({key:s.card,text:v?s.chip:s.offChip,title:`${s.label}: ${v?"on":"off"} for this card (the default is ${dflt(s.card)?"on":"off"})`})}
  for(const s of SUBS){const v=ownOf(c,s.card);if(v===undefined||v===dflt(s.card))continue;
    out.push({key:s.card,text:subChip(s,v),title:`${s.name}: ${v?"on":"off"} for this card (the default is ${dflt(s.card)?"on":"off"})`})}
  return out};
// On the card's face, in its chip row (face.js lays the row out after this).
if(typeof boardHooks!=="undefined")boardHooks.decorate.push((b,c)=>{
  const tags=b.querySelector(":scope > .tags");if(!tags)return;
  for(const x of window.flowChips(c)){const el=document.createElement("span");el.className="tag flow-chip";el.dataset.flowChip=x.key;el.title=x.title;el.textContent=x.text;tags.appendChild(el)}});

// One option as a toggle (card #178, was a three-way pick): the switch shows what holds for the card
// (its own value, else the default); flipping it makes that the card's own. Beside it, "Default"
// while it follows Settings, or a "Reset" button that puts it back on the default. `cur` is the
// card's own value (true, false or undefined); the switch keeps it in data-own (default|on|off).
const ownWord=v=>v===undefined||v===null?"default":v?"on":"off";
const tog=(k,cur,dis)=>{const on=cur===undefined?dflt(k):cur,d=dflt(k)?"on":"off";
  return`<span class="flow-ctl"><button type="button" role="switch" class="flow-tog" data-flow="${esc(k)}" data-own="${ownWord(cur)}" aria-checked="${on}" aria-label="${esc(k)}"${dis?" disabled":""}><span class="flow-knob"></span></button>${cur===undefined
    ?`<span class="flow-src" title="Follows Settings &gt; Sessions (${d} now), as it changes">Default</span>`
    :`<button type="button" class="flow-reset" data-flow-reset="${esc(k)}" title="Back to the default (${d} now), which follows Settings &gt; Sessions"${dis?" disabled":""}>Reset</button>`}</span>`};
const rows=(cur,dis)=>`<div class="flow-rows">${SKIPS.map(s=>`<div class="flow-row" title="${esc(s.help)}"><span>${esc(s.label)}</span>${tog(s.card,cur(s.card),dis)}</div>`).join("")}
  ${SUBS.map(s=>`<div class="flow-row" title="${esc(s.help)}"><span>${esc(s.name)}</span>${tog(s.card,cur(s.card),dis)}</div>`).join("")}</div>`;
const valueOf=v=>v==="on"?true:v==="off"?false:null;
// Redraw one option in place, as the card's own value `v` (true, false, or null for Default).
const setCtl=(ctl,k,v,dis)=>{ctl.outerHTML=tog(k,v===null?undefined:v,dis)};

// The Add card form: every option at Default. `host` is an empty element in the form.
window.subagentTicks=async function(host){
  await loadDefaults();
  host.innerHTML=`<fieldset class="subs-set flow-set"><legend>Card flow</legend>${rows(()=>undefined,false)}
  <p class="meta">Default follows Settings &gt; Sessions, now and when it changes. Flip a switch to make it this card's own; Reset puts it back on the default. Only you change them later, in the card's details.</p></fieldset>`};
// The form's choice: only what isn't Default, as card fields ({skipPlanning, subagents: {...}}).
window.flowChoice=host=>{const o={},subs={};
  host.querySelectorAll(".flow-tog[data-flow]").forEach(x=>{const v=valueOf(x.dataset.own);if(v===null)return;if(isSkip(x.dataset.flow))o[x.dataset.flow]=v;else subs[x.dataset.flow]=v});
  if(Object.keys(subs).length)o.subagents=subs;return o};
// The subagents part of it alone (card #151's name).
window.subagentChoice=host=>window.flowChoice(host).subagents||{};

// The card's details: one "Card flow" box, each option saved as soon as it changes, only that one.
let pend=null; // {id, k, v}: a change being saved, so a redraw meanwhile doesn't put the old one back
window.subagentsBox=c=>{
  const cur=k=>pend&&pend.id===c.id&&pend.k===k?(pend.v===null?undefined:pend.v):ownOf(c,k);
  const dis=typeof canWrite!=="undefined"&&!canWrite;
  const off=SUBS.filter(s=>!nowOf(c,s.card)).map(s=>s.name);
  const skips=SKIPS.filter(s=>nowOf(c,s.card)).map(s=>s.label);
  const child=isChild(c)?" This card is in a stack, so it never skips Planning or Human test: its lead's flow covers it.":"";
  return`<div class="shipbox subs-box flow-box" data-subs="${esc(c.id)}"><h3>Card flow</h3>
  ${rows(cur,dis)}
  <span class="meta">Now: ${skips.length?esc(skips.join(", "))+"; ":""}${off.length?`subagents off: ${esc(off.join(", "))}`:"all subagents on"}.${child} Default follows Settings &gt; Sessions as it changes; flip a switch to make it this card's own, and Reset puts it back on the default. Saved as soon as you change one; its session is told. Only you change these, not its session or the Router.</span>
  <span class="msg subs-msg"></span></div>`};
// A click on a switch or a Reset: the new own value for that option (null for Default).
const clicked=x=>{const t=x.closest&&x.closest(".flow-tog,.flow-reset");if(!t||t.disabled)return null;
  if(t.matches(".flow-reset"))return{t,k:t.dataset.flowReset,v:null};
  return{t,k:t.dataset.flow,v:t.getAttribute("aria-checked")!=="true"}};
document.addEventListener("click",async e=>{
  const x=e.target;if(!x||!x.closest)return;
  const form=x.closest(".flow-set");
  if(form){const h=clicked(x);if(!h)return;e.preventDefault();setCtl(h.t.closest(".flow-ctl"),h.k,h.v,false);
    const n=form.querySelector(`.flow-tog[data-flow="${h.k}"]`);if(n&&n.focus)n.focus();return}
  const box=x.closest(".flow-box");if(!box)return;
  const h=clicked(x);if(!h)return;e.preventDefault();
  const id=box.dataset.subs,c=cards.find(k=>k.id===id);if(!c)return;
  const {k,v}=h;
  const patch=isSkip(k)?{[k]:v}:{subagents:{[k]:v}};
  pend={id,k,v};setCtl(h.t.closest(".flow-ctl"),k,v,true);box.querySelectorAll(".flow-tog,.flow-reset").forEach(i=>i.disabled=true);
  const ok=await write(id,patch,true);pend=null;
  const now=document.querySelector(`.flow-box[data-subs="${CSS.escape(id)}"]`);
  const dis=typeof canWrite!=="undefined"&&!canWrite;
  if(now){const mm=now.querySelector(".subs-msg");
    const cc=cards.find(q=>q.id===id)||c;
    now.querySelectorAll(".flow-ctl").forEach(ctl=>{const t=ctl.querySelector(".flow-tog");if(!t)return;const kk=t.dataset.flow;
      const o=ok&&kk===k?v:ownOf(cc,kk);setCtl(ctl,kk,o===undefined?null:o,dis)});
    const back=now.querySelector(`.flow-tog[data-flow="${k}"]`);if(back&&back.focus)back.focus(); // keys go on from here
    if(ok){mm.textContent="Saved";mm.className="msg subs-msg ok"}
    else{mm.textContent="Not saved. Try again.";mm.className="msg subs-msg err"}}
  else if(!ok&&typeof status==="function")status("The card's flow wasn't saved. Try again.",true);
});
// The defaults, for the details and the chips; again whenever Hugh changes one in Settings.
const redraw=()=>{if(typeof render==="function")render();if(typeof renderDetail==="function")renderDetail()};
loadDefaults().then(redraw);
try{window.__TAURI__.event.listen("settings-changed",async p=>{if(p&&[...SUBS,...SKIPS].some(s=>s.key===p.payload)){await loadDefaults();redraw()}})}catch(e){}
window.cardFlow={loadDefaults,ownOf,nowOf,valueOf};
})();
