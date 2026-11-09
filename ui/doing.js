// What each card is really doing (Hugh, card #150): ONE status chip on each tile, in place of the
// old "Building" / "Testing" chip, with the detail in its tooltip. The first that holds wins:
//   Needs you (a question, a permission prompt, a failed turn) · Installing (the test app install of this card) · Reviewer / Tester / Critic / ... running (the app's
//   subagents), Subagent running (any other), Reviewing (a review skill) · In the test app (its
//   turn) · Test app: 2nd in line · Working 12m · Starting · Waiting on #n (blockedBy not ready) ·
//   Shipped, session open · Idle 25m · No session (an In progress card with none).
// A card the Monitor thinks may be stuck has face.js's "May be stuck" box already: its chip says
// what the session is doing under it, with the Monitor's reason first in the tooltip.
// It reads the session from session_doing (sessions.rs `doing`: state, since when, the turn's
// start, its subagents from doing.rs), and the rest from the other files: needs.js's `needs`,
// board.js's stuckOf/shipState, testapp.js's window.testappState, waits.js's window.Waits and
// steps.js's `steps`. The test app's own tile chip folded into this one. Times tick every 30 s in
// place, without a redraw. The pure helpers are on window.Doing for tools/ui-tests/doing.test.mjs.
let doing={}; // cardId -> {state, since, turnSince, running:[{agent, description, since, background}], testing}

const DOING_AGENT={"card-reviewer":"Reviewer","card-tester":"Tester","card-critic":"Critic","card-reproducer":"Reproducer","card-merger":"Merger","card-router":"Router"};
const doingWhen=v=>typeof v==="number"?v:Date.parse(v||"")||0;
// "<1m", "12m", "1h 05m", "2d 4h".
function doingDur(ms){
  ms=Number(ms);if(!Number.isFinite(ms)||ms<0)ms=0;
  const m=Math.floor(ms/60000);if(m<1)return"<1m";if(m<60)return m+"m";
  const h=Math.floor(m/60);if(h<24)return`${h}h ${String(m%60).padStart(2,"0")}m`;
  return`${Math.floor(h/24)}d ${h%24}h`;
}
const doingClock=t=>{try{return new Date(t).toLocaleTimeString("en-GB",{hour:"2-digit",minute:"2-digit"})}catch(e){return""}};
const doingOrd=n=>{n=Number(n);if(!Number.isFinite(n)||n<1)return"";const t=n%100,o=n%10;
  return n+(t>=11&&t<=13?"th":o===1?"st":o===2?"nd":o===3?"rd":"th")};
const doingPlain=s=>{s=String(s??"").trim();return typeof hideSecrets==="function"?hideSecrets(s):SECRET.test(s)?"details hidden":s};
// "since 10:42 (12m)".
const doingSince=(t,now)=>t?`since ${doingClock(t)} (${doingDur(now-t)})`:"";

// Is this test app entry (holder, line entry or install) this card's?
function tappIs(x,c){
  if(!x||typeof x!=="object"||!c)return false;
  if(x.cardId!=null&&x.cardId!=="")return String(x.cardId)===String(c.id);
  const n=Number(String(x.card??"").replace(/^#/,""));
  return n>0&&n===cardNum(c);
}
// One subagent as a tooltip line: "Reviewer (card-reviewer): Review the diff · 4m, in the background".
function subLine(s,now){
  const who=DOING_AGENT[s.agent]?`${DOING_AGENT[s.agent]} (${s.agent})`:String(s.agent||"subagent");
  const what=doingPlain(s.description);
  const t=doingWhen(s.since);
  return`${who}${what?": "+what:""}${t?` · ${doingDur(now-t)}`:""}${s.background?", in the background":""}`;
}
// The session's own line for a tooltip: "Its session is working, since 10:42 (12m)."
function sessionLine(d,now){
  if(!d)return"No session is running for it.";
  const word={working:"on a turn",permission:"at a prompt",idle:"idle, waiting for a message",starting:"starting",failed:"stopped: its turn failed"}[d.state]||d.state;
  const t=d.state==="working"||d.state==="permission"?doingWhen(d.turnSince)||doingWhen(d.since):doingWhen(d.since);
  return`Its session is ${word}${t?", "+doingSince(t,now):""}.`;
}

// What card c is doing: {k, label, tip} or null. ctx: {now, d (its session_doing entry), need (needs.js),
// stuck (board.js stuckOf), tapp (testapp_status), unmet (waits not ready: [{n, tip}]), step (the step line),
// queued, loaded (false until session_doing has answered: no "No session" before then)}.
function doingOf(c,ctx){
  const y=doingBase(c,ctx),st=ctx&&ctx.stuck,ta=ctx&&ctx.tchip;
  // The card's own test app (or its stack lead's) with nothing running on it: named at the end of
  // the chip, so the tile still says where to test it (card #147).
  const own=ta&&(ta.c==="tap-own"||ta.c==="tap-stack")&&!(c&&isArch(c))?ta:null;
  const x=own?(y&&y.k!=="tapp"&&y.k!=="tline"?{...y,label:`${y.label} · ${own.t}`,tip:`${y.tip}\n${doingPlain(own.title)}`}
    :y||{k:"own",label:own.t,tip:doingPlain(own.title)}):y;
  if(!x||!st)return x;
  const t=doingWhen(st.since),kind=typeof STUCK_KIND!=="undefined"&&STUCK_KIND[st.kind]?STUCK_KIND[st.kind]:st.kind||"";
  return{...x,stuck:true,tip:`The Monitor thinks it may be stuck${kind?" ("+kind+")":""}: ${doingPlain(st.reason)}${t?", for "+doingDur((ctx.now||Date.now())-t):""}.\n${x.tip}`};
}
function doingBase(c,ctx){
  if(!c||isArch(c)||colOf(c)==="Done"||(typeof isSystemCard==="function"&&isSystemCard(c)))return null;
  ctx=ctx||{};const now=ctx.now||Date.now(),d=ctx.d||null,col=colOf(c);
  const step=ctx.step?String(ctx.step):"";
  const tip=(...lines)=>lines.filter(Boolean).join("\n");
  const sess=sessionLine(d,now),stepT=step?`Its to-do list: ${doingPlain(step)}.`:"";
  // 1. It needs Hugh.
  const n=ctx.need;
  if(n&&(n.reason==="permission"||n.reason==="failed")){
    const a=n.ask&&typeof n.ask==="object"?n.ask:null,t=doingWhen(n.at);
    const why=n.reason==="failed"?(n.detail==="crashed"?"Its session crashed.":"Its turn failed."):a&&a.kind==="question"?"It asks you a question"+(a.text?": "+doingPlain(a.text):".")
      :"It waits at a permission prompt"+(a&&a.text?": "+doingPlain(a.text):".");
    return{k:n.reason==="failed"?"failed":"need",label:n.reason==="failed"?(n.detail==="crashed"?"Crashed":"Turn failed"):"Needs you",
      tip:tip(`Needs you${t?" ("+doingDur(now-t)+")":""}. ${why}`,"Answer it in the card's chat.")}
  }
  if(d&&d.state==="permission")return{k:"need",label:"Needs you",tip:tip("Needs you: its session waits at a prompt.",sess)};
  // 2. Its install in a test app (card #147: each slot has its own).
  const tp=ctx.tapp&&typeof ctx.tapp==="object"?ctx.tapp:null;
  const slots=!tp?[]:window.KBTestApp?window.KBTestApp.slotsOf(tp):Array.isArray(tp.slots)?tp.slots:tp.install?[{install:tp.install,holder:tp.holder}]:[];
  const ins=(slots.map(x=>x&&x.install).find(x=>x&&tappIs(x,c)&&(x.phase==="installing"||x.phase==="waiting")))||null;
  if(ins){
    const w=ins.waitingFor?[].concat(ins.waitingFor).map(x=>x&&typeof x==="object"?String(x.label||(x.card?"#"+String(x.card).replace(/^#/,""):"")||x.owner||""):String(x)).filter(Boolean).join(", "):"";
    return ins.phase==="installing"?{k:"install",label:"Installing",tip:tip("Installing its build in the test app.",sess)}
      :{k:"install",label:"Install waiting",tip:tip(`Its test app install waits${w?" for "+doingPlain(w):""}.`,sess)}}
  // 3. Its subagents: the app's own first, then any other; a review skill without one.
  const subs=d&&Array.isArray(d.running)?d.running.filter(s=>s&&typeof s==="object"):[];
  if(subs.length){
    const app=subs.filter(s=>DOING_AGENT[s.agent]);
    const head=app[0]||subs[0];
    const label=DOING_AGENT[head.agent]?`${DOING_AGENT[head.agent]} running`:subs.length>1?`${subs.length} subagents running`:"Subagent running";
    const more=app.length>1?` +${app.length-1}`:"";
    return{k:"agent",label:label+more,tip:tip(`Waiting for ${subs.length===1?"its subagent":subs.length+" subagents"}:`,...subs.map(s=>"- "+subLine(s,now)),sess,stepT)}
  }
  if(d&&d.testing)return{k:"agent",label:"Reviewing",tip:tip("Running a review or tests (a review skill such as /code-review).",sess,stepT)};
  // 4. Its test app (testapp.js's KBTestApp.tileChip, card #147): its turn, or its place in a line.
  const ta=ctx.tchip;
  if(ta&&/^tap-hold/.test(ta.c)){const sl=slots.find(x=>x&&x.holder&&tappIs(x.holder,c)),h=sl&&sl.holder,e=h?doingWhen(h.expiresAt):0;
    return{k:"tapp",label:ta.t,tip:tip(`${doingPlain(ta.title)}${e?` The turn ends at ${doingClock(e)}.`:""}`,sess,stepT)}}
  if(ta&&(ta.c==="tap-wait"||ta.c==="tap-paused"))return{k:"tline",label:ta.t,tip:tip(doingPlain(ta.title),sess)};
  // 5. The session on a turn.
  if(d&&d.state==="working"){const t=doingWhen(d.turnSince)||doingWhen(d.since);
    return{k:"working",label:t?`Working ${doingDur(now-t)}`:"Working",tip:tip(sess,stepT)}}
  if(d&&d.state==="failed")return{k:"failed",label:"Turn failed",tip:tip("Its last turn failed; it waits for a message.",sess)};
  if(d&&d.state==="starting")return{k:"starting",label:"Starting",tip:tip("Its session is starting.")};
  // 6. Waiting on other cards: for a card with a session, or In progress (a card in Backlog or
  // Blocked has waits.js's own "Waits on" chip, and nothing else to say).
  const un=Array.isArray(ctx.unmet)?ctx.unmet:[];
  if(un.length&&(d||col==="In progress")){const ns=un.map(u=>"#"+u.n);
    return{k:"waits",label:`Waiting on ${ns.length<=2?ns.join(", "):ns[0]+" +"+(ns.length-1)}`,tip:tip("Waits on:",...un.map(u=>"- "+doingPlain(u.tip||"#"+u.n)),sess)}}
  // 7. Shipped, with its session still open.
  if(d&&typeof shipState==="function"&&shipState(c)==="shipped")return{k:"shipped",label:"Shipped, session open",tip:tip("Shipped. Its session is still open.",sess)};
  // 8. Idle.
  if(d&&d.state==="idle"){const t=doingWhen(d.since);return{k:"idle",label:t?`Idle ${doingDur(now-t)}`:"Idle",tip:tip(sess,stepT)}}
  if(d)return{k:"idle",label:String(d.state||"Session open"),tip:sess};
  // 9. An In progress card with no session (a start waiting under the cap has its own badge).
  if(col==="In progress"&&ctx.loaded!==false&&!ctx.queued&&!(Number(c.stackOf)>0))return{k:"none",label:"No session",tip:"No Claude session is running for this card. Open it to start or resume one."};
  return null;
}

// The context for a card from the other files. panel.js's `activity` (session_states) is newer than
// `doing` for a moment after each change, and stands in for it until session_doing answers.
function doingSession(id){
  const d=doing[id]||null,a=typeof activity!=="undefined"&&activity?activity[id]:undefined;
  if(!a)return d;
  if(!d)return{state:a,since:null,turnSince:null,running:[],testing:false};
  return d.state===a?d:{...d,state:a,since:null,turnSince:null};
}
function doingCtx(c,now){
  const tp=typeof window.testappState==="function"?window.testappState():null;
  const all=typeof cards!=="undefined"&&Array.isArray(cards)?cards:[];
  const tchip=window.KBTestApp&&typeof window.KBTestApp.tileChip==="function"?hookTry(window.KBTestApp.tileChip,c,all,tp):null;
  const un=window.Waits&&typeof window.Waits.unmet==="function"?window.Waits.unmet(c):[];
  const s=typeof steps!=="undefined"&&steps?steps[c.id]:null;
  const queued=typeof startQueue!=="undefined"&&startQueue&&Array.isArray(startQueue.queued)&&startQueue.queued.some(x=>x.card===c.id);
  return{now:now||Date.now(),d:doingSession(c.id),loaded:doingLoaded,need:typeof needs!=="undefined"&&needs?needs[c.id]:null,
    stuck:typeof stuckOf==="function"?stuckOf(c):null,tapp:tp,tchip,unmet:un,step:s&&s.line?s.line:"",queued};
}
// The chip's HTML (board.js's cardEl puts it in the chip row; "" when there is none).
function doingHtml(x,id){
  if(!x)return"";
  const tip=SECRET.test(x.tip)?"":` title="${esc(x.tip)}"`;
  return`<span class="tag chip doing d-${esc(x.k)}" data-doing="${esc(x.k)}" data-doing-card="${esc(id)}"${tip}>${esc(x.label)}</span>`;
}
function doingChip(c){return doingHtml(hookTry(doingOf,c,doingCtx(c)),c.id)}

// A stack's tile has no chip row of its own: the chip goes on its sub-line. A tile whose chip says
// "Waiting on #n" drops waits.js's "Waits on #n" chip (its decorate hook ran first), so it isn't said twice.
boardHooks.decorate.push((b,c)=>{
  if(b.classList.contains("stk")){if(b.querySelector(".doing"))return;
    const sub=b.querySelector(":scope > .stk-sub"),h=doingChip(c);if(sub&&h)sub.insertAdjacentHTML("beforeend"," "+h);return}
  const own=b.querySelector(":scope > .tags > .doing");
  if(own&&own.dataset.doing==="waits")for(const w of b.querySelectorAll(":scope > .tags > .wt-chip"))w.remove();
});

// Every tile's chip again, in place: new words, tooltip and colour, with no board redraw (the
// clock every 30 s, and each new answer). Only a chip that comes or goes, or a change to or from
// "Waiting on" (which hides the "Waits on" chip), redraws the board.
function doingPaint(now){
  now=now||Date.now();
  const byId=new Map((typeof cards!=="undefined"?cards:[]).map(c=>[c.id,c]));
  for(const tile of document.querySelectorAll(".card[data-id]")){
    const c=byId.get(tile.dataset.id);if(!c||isArch(c))continue;
    const x=hookTry(doingOf,c,doingCtx(c,now));
    const el=tile.querySelector(":scope > .tags > .doing")||tile.querySelector(":scope > .stk-sub > .doing");
    if(!x!==!el||(x&&x.k!==el.dataset.doing&&(x.k==="waits"||el.dataset.doing==="waits"))){render();return}
    if(!x)continue;
    if(el.textContent!==x.label)el.textContent=x.label;
    const t=SECRET.test(x.tip)?"":x.tip;if(el.title!==t)el.title=t;
    if(el.dataset.doing!==x.k){el.dataset.doing=x.k;el.className=`tag chip doing d-${x.k}`}
  }
}
const doingTick=()=>doingPaint();

let doingTimer=null,doingKey="",doingLoaded=false;
async function loadDoing(){
  let d;try{d=await invoke("session_doing")}catch(e){return}
  if(!d||typeof d!=="object")return; // an older backend: Working and Idle from `activity` only
  const k=JSON.stringify(d),first=!doingLoaded;doingLoaded=true;
  if(k===doingKey&&!first)return;
  doingKey=k;doing=d;doingPaint();
}
const doingSoon=()=>{clearTimeout(doingTimer);doingTimer=setTimeout(loadDoing,150)};
(()=>{
  const ev=window.__TAURI__&&window.__TAURI__.event;
  loadDoing();
  if(ev)for(const name of ["doing","activity","testing","session-changed"])ev.listen(name,doingSoon);
  // The test app's line and turn (testapp.js).
  let tk="";document.addEventListener("testapp-status",()=>{const s=typeof window.testappState==="function"?window.testappState():null;
    const k=JSON.stringify(s&&[s.slots,s.slotLine,s.holder,s.line,s.install]);if(k!==tk){tk=k;doingPaint()}});
  setInterval(doingTick,30000);
  setInterval(loadDoing,60000); // anything missed
})();

window.Doing={doingOf,doingDur,doingChip,doingTick,doingPaint,subLine,AGENT:DOING_AGENT};
