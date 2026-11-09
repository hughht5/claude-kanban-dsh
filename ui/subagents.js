// Card #151: Hugh's choice of the app's six subagents for each card. The card keeps its own
// (`subagents`: {router, critic, reproducer, reviewer, tester, merger}, each true or false), set
// from the Settings switches when it is made; a card made before #151 has none and follows the
// switches as they are now. The Add card form's ticks, the card's details box (saved as soon as
// one changes), and the Settings switches (settings.js) all use this one list. Only Hugh changes
// it: card sessions and the Router can't (kanban_update_card refuses the field). Uses board.js's
// esc, invoke, cards, canWrite, write and say.
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
  {card:"merger",name:"card-merger",key:"sessions.merger_at_ship",label:"card-merger: bring the branch up to date at ship",
   help:"In Ship it, before the push, the card-merger rebases the card's branch onto the latest origin/main, resolves conflicts and reruns the tests. It holds nothing. Off: the Ship it session does that itself. A stack's lead still merges each child with it, whatever this says. Default on."},
];
const SUBS=window.NEW_CARD_SUBAGENTS;
// The Settings switches, as {router: true, ...}: read when the form opens and kept for the details.
let defaults=null;
async function loadDefaults(){
  const out={};
  await Promise.all(SUBS.map(async s=>{try{const g=await invoke("get_setting",{key:s.key});out[s.card]=g.value!=="off"}catch(e){out[s.card]=true}}));
  defaults=out;return out}
const own=c=>c&&c.subagents&&typeof c.subagents==="object"&&!Array.isArray(c.subagents)?c.subagents:{};
// Does the card have a choice of its own for every one of them?
const ownAll=c=>SUBS.every(s=>typeof own(c)[s.card]==="boolean");
// The card's subagents as they hold now: its own choice, else the default.
const subsOf=c=>{const o={};for(const s of SUBS){const v=own(c)[s.card];o[s.card]=typeof v==="boolean"?v:(defaults?defaults[s.card]!==false:true)}return o};
// Why a subagent is off on the card, for its row ("" when it is on): its own choice, or the
// default a card made before #151 follows.
window.subagentOffText=(c,card)=>subsOf(c)[card]!==false?"":typeof own(c)[card]==="boolean"?"off for this card":"off by the default in Settings";
// Which subagent a verdict field belongs to (board.js's CHECKS), for its "off" note.
window.SUBAGENT_OF_CHECK={test:"tester",review:"reviewer",critique:"critic",repro:"reproducer",merge:"merger"};

// The Add card form: six ticks, set from the defaults. `host` is an empty element in the form.
window.subagentTicks=async function(host){
  const d=await loadDefaults();
  host.innerHTML=`<fieldset class="subs-set"><legend>Subagents</legend><div class="subs-row">${SUBS.map(s=>`<label class="st-radio" title="${esc(s.help)}"><input type="checkbox" data-sub="${esc(s.card)}"${d[s.card]?" checked":""}> ${esc(s.name)}</label>`).join("")}</div>
  <p class="meta">Ticked as Settings &gt; Sessions sets them for a new card. Only you change them later, in the card's details.</p></fieldset>`};
// The ticks' choice, as the card's `subagents`.
window.subagentChoice=host=>{const o={};host.querySelectorAll("input[data-sub]").forEach(x=>{o[x.dataset.sub]=x.checked});return o};

// The card's details: the six, each saved as soon as it changes.
let pend=null; // {id, subs}: a change being saved, so a redraw meanwhile doesn't put the old one back
window.subagentsBox=c=>{
  const now=pend&&pend.id===c.id?pend.subs:subsOf(c);
  const off=SUBS.filter(s=>!now[s.card]).map(s=>s.name);
  const from=ownAll(c)?"This card's own choice.":"This card was made before the per-card choice, so it follows Settings &gt; Sessions (Subagents on a new card) until you change one here.";
  return`<div class="shipbox subs-box" data-subs="${esc(c.id)}"><h3>Subagents</h3>
  <div class="subs-row">${SUBS.map(s=>`<label class="st-radio" title="${esc(s.help)}"><input type="checkbox" data-sub="${esc(s.card)}"${now[s.card]?" checked":""}${typeof canWrite!=="undefined"&&!canWrite?" disabled":""}> ${esc(s.name)}</label>`).join("")}</div>
  <span class="meta">${from} ${off.length?`Off: ${esc(off.join(", "))}: its session skips them and nothing waits for them.`:"All on."} Saved as soon as you change one; its session is told.</span>
  <span class="msg subs-msg"></span></div>`};
document.addEventListener("change",async e=>{
  const x=e.target;if(!x.matches||!x.matches(".subs-box input[data-sub]"))return;
  const box=x.closest(".subs-box"),id=box.dataset.subs,c=cards.find(k=>k.id===id);if(!c)return;
  const subs=Object.assign(subsOf(c),window.subagentChoice(box));
  const m=box.querySelector(".subs-msg");
  pend={id,subs};box.querySelectorAll("input").forEach(i=>i.disabled=true);
  const ok=await write(id,{subagents:subs},true);pend=null;
  const now=document.querySelector(`.subs-box[data-subs="${CSS.escape(id)}"]`);
  if(now){now.querySelectorAll("input").forEach(i=>i.disabled=typeof canWrite!=="undefined"&&!canWrite);const mm=now.querySelector(".subs-msg");
    if(ok){mm.textContent="Saved";mm.className="msg subs-msg ok"}
    else{const cur=subsOf(cards.find(k=>k.id===id)||c);now.querySelectorAll("input[data-sub]").forEach(i=>{i.checked=!!cur[i.dataset.sub]});mm.textContent="Not saved. Try again.";mm.className="msg subs-msg err"}}
  else if(m&&!ok)status("The subagents weren't saved. Try again.",true);
});
// The defaults, for the details of a card made before #151; again whenever Hugh changes a switch.
loadDefaults().then(()=>{if(typeof renderDetail==="function")renderDetail()});
try{window.__TAURI__.event.listen("settings-changed",async p=>{if(p&&SUBS.some(s=>s.key===p.payload)){await loadDefaults();if(typeof renderDetail==="function")renderDetail()}})}catch(e){}
})();
