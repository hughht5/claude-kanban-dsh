// Human test review (card #102): open a Human test card and step through the column with
// Previous and Next, without closing the panel; Pass moves it on as Hugh's own move and Fail
// sends his note back to its session. Keys: P, F, Left, Right, U (undo the newest Pass).
// Card #120: the review mode, over the whole panel (review-mode.css), with a Chat tab back.
// Uses board.js (cards, colOf, isArch, byOrder, matches, cardNum, esc, safe, invoke, errText,
// status, load, checkOf, checkChip, CHECKS, CHECK_LABEL, ago, render, renderDetail), stacks.js
// (stackKids, isStackLead, stackHidden, stackMatches, stkLead, kidState, stkMd, stkKidOpen,
// stkDlgClose), needs.js (pinFirst) and panel.js (panel, openPanel,
// closePanel, showShots, openShot, renderAtts). panel.js calls reviewOpened, reviewClosed,
// reviewOwnsFocus, reviewFailing, reviewFailSend and reviewFailEnd.
const HT="Human test",PASS_UNDO_MS=5000;
let rq=null;           // the queue while Hugh steps: {ids, leaving, focusBar}; null when there is none
let htConfirm=null;    // the card id whose "Ship #n?" waits for Enter
let htFail=null;       // the card id whose message box is in fail mode
let htClearOwed=false; // the panel closed on an empty queue: say "Human test is clear" once nothing is pending
const htPending=[];    // Passes waiting out their PASS_UNDO_MS: {id, num, title, until, timer}, newest last
let htTick=null;

// The Human test column as the board draws it: its filter, needs-you first, then Hugh's order.
// A stack is one stop (the router, for card #116).
// Card #0 (the router's own system card) is never in the queue, so it never counts.
const htSystem=c=>typeof isSystemCard==="function"?isSystemCard(c):!!c&&c.number===0;
function htLive(){
  return pinFirst(cards.filter(c=>!htSystem(c)&&!isArch(c)&&colOf(c)===HT&&!stackHidden(c)&&stackMatches(c)).sort(byOrder)).map(c=>c.id);
}
// Review mode (card #120): a card opened in the queue shows the review view over the whole panel
// (shots, verdicts, checks, the bar). "Chat" (or Open the card) goes back to the normal panel.
let rvView="review";
const htReviewing=()=>!!htOpen()&&rvView==="review";
// Keeps the queue's order: cards that left drop out, new ones go on the end. A card the panel
// is leaving (its Fail is on its way) keeps its place until the panel steps on.
function rqSync(){
  if(!rq)return;
  const live=htLive(),now=new Set(live);
  const ids=rq.ids.filter(id=>now.has(id)||id===rq.leaving);
  for(const id of live)if(!ids.includes(id))ids.push(id);
  rq.ids=ids;
  // The open card left the queue some other way (a plain Send moved it back, say): the queue ends.
  if(panel&&!ids.includes(panel.id))htEnd();
}
function htEnd(){rq=null;htConfirm=null;htFail=null}
const htCard=id=>cards.find(x=>x.id===id)||null;
const htOpen=()=>panel&&rq&&rq.ids.includes(panel.id)?htCard(panel.id):null;
const htPend=id=>htPending.find(p=>p.id===id)||null;
const htLeadOf=c=>{const l=stkLead(c);return l&&!isArch(l)?l:null};
// What kind of Pass a card takes: a stack's child is marked Passed; a lead ships its stack; any
// other card goes to Ship it.
const htKind=c=>htLeadOf(c)?"child":isStackLead(c)?"lead":"card";

// Hooks from panel.js.
function reviewOpened(id,step){
  htConfirm=null;htFail=null;rvView="review";
  if(!step){const live=htLive();rq=live.includes(id)?{ids:live,leaving:null}:null}
  else if(rq)rq.leaving=null;
  if(rq)rq.focusBar=true;
}
function reviewClosed(step){if(!step)htEnd()}
function reviewOwnsFocus(){return !!htOpen()}
function reviewFailing(){return !!(htFail&&htOpen()&&htFail===panel.id)}

// Stepping: the panel switches card in place (openPanel's step flag), so the queue stays. A
// stack is one stop. Here that stop is the lead's own panel, whose bar ships the stack once its
// cards are passed. Card #116's stack overlay takes the stop over by setting reviewStackStop to
// a function(lead) that opens the overlay and returns true; it steps on with reviewStepFrom.
let reviewStackStop=null;
const htPanelOpen=()=>!!panel;
function htToStack(lead){
  if(typeof reviewStackStop!=="function")return false;
  let took=false;try{took=reviewStackStop(lead)===true}catch(e){took=false}
  if(took&&htPanelOpen()){closePanel(true);render()}
  return took;
}
function htGo(id){
  if(!id||!rq)return;
  const c=htCard(id);
  if(c&&isStackLead(c)&&typeof reviewStackStop==="function"){stkDlgClose();if(htToStack(c))return}
  if(htPanelOpen()&&panel.id===id)return;
  stkDlgClose();
  openPanel(id,{step:true});
}
function htNeighbour(d){if(!rq||!htPanelOpen())return null;const i=rq.ids.indexOf(panel.id);return i<0?null:rq.ids[i+d]||null}

// For card #116's stack overlay (the router's interface):
// - reviewStartAt(id): starts the queue (or goes on with it) at card `id`, a card or a stack in
//   Human test. Returns false when `id` isn't in the queue. A stack goes to reviewStackStop when
//   it is set, else to the lead's panel.
// - reviewStackStop: null, or #116's function(lead) that opens the overlay and returns true.
// - reviewStepFrom(id, d): the stop before (d -1) or after (d 1) card `id`; false at either end.
// - reviewBackToStack(leadId, kidId): closes the panel, keeps the queue, and opens the stack's
//   overlay on that child.
// - reviewQueue(): the queue's card ids, in order.
function reviewStartAt(id){
  const live=htLive();if(!live.includes(id))return false;
  rqSync();if(!rq||!rq.ids.includes(id))rq={ids:live,leaving:null};
  if(htPanelOpen()&&panel.id===id){rq.focusBar=true;htDraw();return true}
  htGo(id);return true;
}
function reviewStepFrom(id,d){
  if(!rq)rq={ids:htLive(),leaving:null};
  const i=rq.ids.indexOf(id),t=i<0?null:rq.ids[i+(d<0?-1:1)];
  if(!t)return false;htGo(t);return true;
}
function reviewBackToStack(leadId,kidId){
  if(htPanelOpen()){closePanel(true);render()}
  stkKidOpen(leadId,kidId);
}
function reviewQueue(){if(!rq)return htLive();rqSync();return rq?[...rq.ids]:htLive()}
// After a Pass or a Fail: the next card that isn't waiting out a Pass, else the one before, else
// none: the panel closes.
function htAfter(id){
  if(!rq)return;
  const i=rq.ids.indexOf(id),free=x=>x!==id&&!htPend(x);
  const next=rq.ids.slice(i+1).find(free)||rq.ids.slice(0,Math.max(i,0)).reverse().find(free);
  if(next){htGo(next);return}
  closePanel();
  if(htPending.length)htClearOwed=true;else status("Human test is clear");
}

// Pass is P then Enter for every card (the router): "Ship #n?" (or "Pass #n?" for a stack's
// child), where Enter confirms and any other key cancels. A card or a stack then waits
// PASS_UNDO_MS, to undo, before the move; a child is marked Passed at once, which ships nothing
// and can be undone.
function htPassState(c){
  const p=htPend(c.id);if(p)return{ok:false,why:`Passing in ${htLeft(p)} s`};
  const k=htKind(c);
  if(k==="child")return c.passed===true?{ok:false,passed:true,why:"Passed"}:{ok:true,label:"Pass"};
  if(k==="lead"){const why=htStackHold(c);return why?{ok:false,why}:{ok:true,label:"Pass: ship the stack"}}
  return{ok:true,label:"Pass: to Ship it"};
}
// Why a stack can't ship yet, or "": a card in it still to pass in Human test, or still being
// built. A card past Human test, or passed in it, counts as done (stacks.js kidState).
function htStackHold(lead){
  const kids=stackKids(lead),test=kids.filter(k=>colOf(k)===HT&&kidState(k).k!=="pass"),build=kids.filter(k=>colOf(k)!==HT&&kidState(k).k!=="pass");
  const n=l=>l.map(x=>"#"+(cardNum(x)||"")).join(", ");
  return [test.length?`Pass ${n(test)} first`:"",build.length?`${n(build)} still being built`:""].filter(Boolean).join("; ");
}
function htPass(){
  const c=htOpen();if(!c||htFail)return;
  const st=htPassState(c);if(!st.ok)return;
  htConfirm=c.id;htDraw();
}
async function htPassChild(c){
  try{await invoke("set_passed",{id:c.id,on:true});status(`#${cardNum(c)} passed`)}
  catch(e){status(`Couldn't mark #${cardNum(c)} passed: ${errText(e)}`,true);return}
  htAfter(c.id);
}
async function htUnpass(){
  const c=htOpen();if(!c)return;
  try{await invoke("set_passed",{id:c.id,on:false});status(`#${cardNum(c)} is no longer marked passed`)}
  catch(e){status(`Couldn't take Passed back on #${cardNum(c)}: ${errText(e)}`,true)}
}
function htConfirmYes(){
  const c=htCard(htConfirm);htConfirm=null;
  if(!c||colOf(c)!==HT||htPend(c.id)){htDraw();return}
  if(htKind(c)==="child"){htDraw();htPassChild(c);return}
  htStartPending(c);htAfter(c.id);
}
function htStartPending(c){
  const p={id:c.id,num:cardNum(c),title:c.title||"",lead:htKind(c)==="lead",until:Date.now()+PASS_UNDO_MS};
  p.timer=setTimeout(()=>htFire(p),PASS_UNDO_MS);
  htPending.push(p);htTicker();htBanner();
}
// For card #116: its overlay's own "Ship stack #n?" confirm calls reviewShip(lead id), which
// starts the stack's pending Pass (banner, Undo, then the move) and moves the queue on. False
// when the stack can't ship now (not in Human test, already pending, or htStackHold says why).
function reviewShip(id){
  const c=htCard(id);
  if(!c||isArch(c)||colOf(c)!==HT||htPend(c.id)||htKind(c)!=="lead"||htStackHold(c))return false;
  htStartPending(c);
  if(rq&&rq.ids.includes(c.id)){const nx=reviewStepFrom(c.id,1)||reviewStepFrom(c.id,-1);if(!nx&&htPanelOpen())closePanel()}
  return true;
}
function htConfirmNo(){if(htConfirm){htConfirm=null;htDraw()}}
const htLeft=p=>Math.max(0,Math.ceil((p.until-Date.now())/1000));
function htUndo(id){
  const p=id?htPend(id):htPending[htPending.length-1];if(!p)return false;
  clearTimeout(p.timer);htPending.splice(htPending.indexOf(p),1);
  htTicker();htBanner();htDraw();htClearOwed=false; // it is back in the column, so Human test isn't clear
  status(`#${p.num} stays in Human test: its Pass is undone`);return true;
}
// The move itself, as Hugh's drag makes it, with the card's version as it is now. A move refused
// because the card changed meanwhile is tried once more on the reloaded card.
async function htFire(p){
  const i=htPending.indexOf(p);if(i<0)return;
  htPending.splice(i,1);htTicker();htBanner();htDraw();
  const what=p.lead?`stack #${p.num}`:`#${p.num}`;let said="",bad=false;
  for(let tries=0;tries<2;tries++){
    const c=htCard(p.id);
    if(!c||isArch(c)||colOf(c)!==HT){said=`${what} left Human test meanwhile, so it wasn't moved to Ship it`;bad=true;break}
    const hold=p.lead?htStackHold(c):"";
    if(hold){said=`${what} wasn't moved to Ship it: ${hold}`;bad=true;break}
    try{
      const r=await invoke("move_card",{id:c.id,version:c.version,column:"Ship it",extra:{}});
      said=r&&r.shipWaitFor?`Passed ${what}: in Ship it, waiting for #${r.shipWaitFor} to finish shipping`:`Passed ${what}: in Ship it, and its session is told to ship it`;
      break;
    }catch(e){
      if(e&&e.kind==="conflict"&&tries===0){await load();continue}
      said=`Couldn't move ${what} to Ship it: ${errText(e)}. It is still in Human test.`;bad=true;break;
    }
  }
  status(said+(htClearCheck()?" · Human test is clear":""),bad);
}
// True once (and the flag cleared) when the panel closed on an empty queue and no Pass is left.
function htClearCheck(){if(htClearOwed&&!htPending.length){htClearOwed=false;return !htPanelOpen()}return false}
function htTicker(){
  if(htPending.length&&!htTick)htTick=setInterval(()=>{htBanner();const c=htOpen();if(c&&htPend(c.id))htDraw()},1000);
  if(!htPending.length&&htTick){clearInterval(htTick);htTick=null}
}
// The banner at the foot of the window: every Pass still waiting, each with its own Undo.
function htBanner(){
  let b=document.getElementById("ht-pending");
  if(!htPending.length){if(b)b.remove();return}
  if(!b){b=document.createElement("div");b.id="ht-pending";b.className="ht-pending";b.setAttribute("role","status");
    b.addEventListener("click",e=>{const u=e.target.closest("[data-undo]");if(u)htUndo(u.dataset.undo)})}
  // Inside the open stack overlay when there is one (card #116), so it shows above its top layer.
  const host=document.querySelector("dialog.stk-kdlg[open]")||document.body;if(b.parentElement!==host)host.append(b);
  b.innerHTML=htPending.map(p=>`<div class="ht-prow"><span>Passed <b>${p.lead?"stack ":""}#${esc(p.num)}</b> ${safe(p.title)}: to Ship it in ${htLeft(p)} s</span><button class="btn ghost sm" type="button" data-undo="${esc(p.id)}">Undo</button></div>`).join("")
    +`<div class="ht-phint">U undoes the newest</div>`;
}

// Fail: the message box takes Hugh's note (attachments and pasted screenshots too). Send fails
// the card: human_test_fail, or a stack child's Found a problem.
function htFailStart(){
  const c=htOpen();if(!c||htPend(c.id))return;
  const ta=panel.el.querySelector("#p-text");
  if(!ta){status("The message box isn't ready yet: try again in a moment",true);return}
  htConfirm=null;htFail=c.id;htDraw();htFailRow();ta.focus();
}
function htFailRow(){
  if(!panel)return;const box=panel.el.querySelector("#p-compose"),ta=box&&box.querySelector("#p-text"),send=box&&box.querySelector("#p-send");if(!ta)return;
  let row=box.querySelector(".ht-failrow");const c=htCard(panel.id);
  if(!reviewFailing()||!c){if(row)row.remove();box.classList.remove("ht-failing");if(send)send.textContent="Send";ta.placeholder=ta.dataset.htWas||ta.placeholder;return}
  const what=htKind(c)==="child"?`#${cardNum(c)} goes back to In progress and the stack's session gets your note (Found a problem)`
    :htKind(c)==="lead"?`stack #${cardNum(c)} goes back to In progress and its session gets your note`
    :`#${cardNum(c)} goes back to In progress and its session gets your note`;
  if(!row){row=document.createElement("div");row.className="ht-failrow";ta.before(row)}
  row.innerHTML=`<b>Failing #${esc(cardNum(c))}.</b> Say what's wrong. Enter sends it: ${esc(what)}. Esc cancels.`;
  box.classList.add("ht-failing");if(send)send.textContent="Send and fail";
  if(!ta.dataset.htWas)ta.dataset.htWas=ta.placeholder;ta.placeholder="What's wrong? Paste or drop screenshots here too";
}
function reviewFailEnd(){htFail=null;htFailRow();htDraw();const b=panel&&panel.el.querySelector("#p-rbar");if(b)b.focus()}
let htSending=false;
async function reviewFailSend(text){
  const c=htOpen();if(!c||htSending)return;
  const note=String(text||"").trim();if(!note)return;
  if(SECRET.test(note)){status("Not sent: the note looks like it holds a token, key or password.",true);return}
  htSending=true;rq.leaving=c.id;
  const n=cardNum(c),child=htKind(c)==="child";
  let r=null;
  try{
    if(child)r=await invoke("stack_problem",{id:c.id,note});else r=await invoke("human_test_fail",{cardId:c.id,note});
  }catch(e){htSending=false;if(rq)rq.leaving=null;status(`Not sent: ${errText(e)}`,true);return}
  htSending=false;
  const here=panel&&panel.id===c.id;
  if(here){const ta=panel.el.querySelector("#p-text");if(ta)ta.value="";panel.att=[];renderAtts()}
  htFail=null;htFailRow();
  // human_test_fail's sendError: the card failed and went back, and the note is on its log, but
  // its session didn't get it.
  status(r&&r.sendError?`Failed #${n}: it's back in In progress and your note is on its log, but its session didn't get it: ${r.sendError}`
    :`Failed #${n}: it's back in In progress, and ${child?"the stack's":"its"} session has your note`,!!(r&&r.sendError));
  if(here)htAfter(c.id);else if(rq)rq.leaving=null;
}

// The review bar under the panel's header, for a card in the queue.
// board.js's chips, plus "Test not run" / "Review not run" for a check still needed, which the
// board leaves off; no chip at all when the card has no record of the check.
function htChip(c,ch){
  const t=checkOf(c,ch.key);if(!t)return"";
  if(t.state!=="needed")return checkChip(c,ch);
  return`<span class="tag chip test-needed" data-check="${esc(ch.key)}">${esc((ch.key==="test"?"Test":ch.key==="review"?"Review":ch.row)+" not run")}</span>`;
}
// The bar is one element (role toolbar, one focus target): its head (Previous, the title, "i of
// N", the Review/Chat tabs, Next) and its actions (the check-size chip, the diff size, the chips
// still needed, and the buttons). In review mode review-mode.css puts the head at the top of the
// panel and the actions at its foot (a subgrid), so the bar keeps one DOM place.
function htTagsHtml(c){
  const lead=htLeadOf(c);
  const size=typeof checkSizeText==="function"?checkSizeText(c):"";
  const sz=size?`<span class="tag chip rv-size${typeof checkSizeOf==="function"&&checkSizeOf(c)&&checkSizeOf(c).class==="quick"?" quick":""}">${esc(size)}</span>`:"";
  let diff="";
  try{
    const b=(typeof CHG!=="undefined"&&CHG.briefs&&CHG.briefs[c.id])||(typeof chgLocalBrief==="function"?chgLocalBrief(c):null);
    if(b&&Number(b.files)>0&&typeof chgFiles==="function"&&typeof chgFigHtml==="function")diff=`<span class="rv-diff" title="What changed on this card's branch">${esc(chgFiles(b.files))} ${chgFigHtml(b.added,b.removed)}</span>`;
  }catch(e){diff=""}
  return`${sz}${diff}${CHECKS.map(ch=>htChip(c,ch)).join("")}${lead?`<span class="tag">In stack #${esc(cardNum(lead))}</span>`:""}`;
}
function htBarHtml(c){
  const i=rq.ids.indexOf(c.id),n=rq.ids.length,st=htPassState(c),p=htPend(c.id),kind=htKind(c),rv=rvView==="review";
  const head=`<div class="rv-head"><button class="ibtn abtn" type="button" data-ht="prev" aria-keyshortcuts="ArrowLeft" title="Previous Human test card (Left)"${i>0?"":" disabled"}>← Previous</button>
    <span class="rv-ttl">${cardNum(c)?`<b class="rv-num">#${esc(cardNum(c))}</b> `:""}${safe(c.title||"Untitled card")}</span>
    <span class="ht-pos">${i+1} of ${n} in Human test</span>
    <span class="rv-tabs" role="group" aria-label="View"><button class="ibtn" type="button" data-ht="view-review" aria-pressed="${rv}" title="The review view: shots, verdicts and checks">Review</button><button class="ibtn" type="button" data-ht="view-chat" aria-pressed="${!rv}" title="The card's chat and details">Chat</button></span>
    <button class="ibtn abtn" type="button" data-ht="next" aria-keyshortcuts="ArrowRight" title="Next Human test card (Right)"${i<n-1?"":" disabled"}>Next →</button></div>`;
  const open=`<button class="abtn acc" type="button" data-ht="open" title="Leave the review view and write in the card's chat">Open the card</button>`;
  let acts;
  if(htConfirm===c.id)acts=`<span class="ht-ask" role="alert">${kind==="lead"?"Ship stack":kind==="child"?"Pass":"Ship"} #${esc(cardNum(c))}? <b>Enter</b> confirms, any other key cancels.</span>
    <button class="abtn pri" type="button" data-ht="yes">${kind==="child"?"Pass":"Ship it"}</button><button class="abtn" type="button" data-ht="no">Cancel</button>`;
  else if(p)acts=`<span class="ht-wait">Passing in ${htLeft(p)} s</span><button class="abtn" type="button" data-ht="undo" aria-keyshortcuts="U">Undo (U)</button>${open}`;
  else{
    const failOn=htFail===c.id;
    const fail=`<button class="abtn ht-fail" type="button" data-ht="fail" aria-keyshortcuts="F" aria-pressed="${failOn}"${failOn?" disabled":""}>Fail with a note <kbd>F</kbd></button>`;
    const pass=st.passed?`<span class="ht-done">Passed</span><button class="abtn" type="button" data-ht="unpass">Undo passed</button>`
      :st.ok?`<button class="abtn pri ht-pass" type="button" data-ht="pass" aria-keyshortcuts="P"${failOn?" disabled":""}>${esc(st.label)} <kbd>P</kbd></button>`
      :`<button class="abtn pri ht-pass" type="button" disabled title="${esc(st.why)}">Pass <kbd>P</kbd></button><span class="ht-why">${esc(st.why)}</span>`;
    acts=pass+fail+open;
  }
  const tags=htTagsHtml(c);
  return`${head}<div class="rv-acts">${tags?`<span class="ht-chips rv-tags">${tags}</span>`:""}<span class="ht-acts">${acts}</span><span class="ht-keys">← → step · P pass · F fail · U undo</span></div>`;
}
function htDraw(){
  if(!panel)return;
  const el=panel.el,c=htOpen();let bar=el.querySelector("#p-rbar");
  const rv=!!c&&rvView==="review";
  el.classList.toggle("rv",rv);el.classList.toggle("rv-failing",rv&&!!c&&htFail===c.id);
  if(!c){if(bar){bar.remove();el.classList.remove("has-rbar")}htFailRow();return}
  if(!bar){
    bar=document.createElement("div");bar.className="rbar";bar.id="p-rbar";bar.tabIndex=-1;bar.setAttribute("role","toolbar");bar.setAttribute("aria-label","Human test review");
    el.querySelector(".ph").after(bar);el.classList.add("has-rbar");
    bar.addEventListener("click",e=>{const b=e.target.closest("[data-ht]");if(!b||b.disabled)return;e.preventDefault();htAct(b.dataset.ht)});
    // The terminal lives beside the chat: asking for it leaves the review view.
    const tb=el.querySelector("#p-term");if(tb)tb.addEventListener("click",()=>{if(htReviewing())htView("chat")},true);
  }
  const html=htBarHtml(c);if(bar.dataset.html===html&&!rq.focusBar)return;
  const had=bar.contains(document.activeElement),fk=had&&document.activeElement.dataset?document.activeElement.dataset.ht:null;
  bar.innerHTML=html;bar.dataset.html=html;
  // Focus stays on the button Hugh used when it is still there (a tab, Next), else on the bar.
  const back=fk&&!rq.focusBar?bar.querySelector(`[data-ht="${CSS.escape(fk)}"]:not(:disabled)`):null;
  if(back)back.focus({preventScroll:true});
  else if(had||rq.focusBar){rq.focusBar=false;bar.focus({preventScroll:true})}
}
// Review or Chat. The details are drawn again for the view (the review box, or the old box).
function htView(v,focusChat){
  if(!htOpen())return;
  rvView=v==="chat"?"chat":"review";
  htBox();htDraw();
  if(typeof navNote==="function")navNote(); // nav.js: review and chat are two views (card #187)
  if(focusChat&&panel){const ta=panel.el.querySelector("#p-text");if(ta)ta.focus()}
}
function htAct(a){
  if(a==="prev")htGo(htNeighbour(-1));else if(a==="next")htGo(htNeighbour(1));
  else if(a==="pass")htPass();else if(a==="yes")htConfirmYes();else if(a==="no")htConfirmNo();
  else if(a==="fail")htFailStart();else if(a==="undo"){const c=htOpen();if(c)htUndo(c.id)}
  else if(a==="unpass")htUnpass();
  else if(a==="view-review")htView("review");else if(a==="view-chat")htView("chat");
  else if(a==="open")htView("chat",true);
}

// The "Human test" box at the top of the card's details: the verdicts, the hand-over, then the
// evidence (card #101, evidenceHtml) and what changed (card #108, changesHtml) when they exist.
function htBoxHtml(c){
  const rows=CHECKS.map(ch=>{const t=checkOf(c,ch.key);if(!t)return"";const at=Date.parse(t.at||"");
    const who=ch.key==="test"?"Card-tester":ch.key==="review"?"Reviewer":ch.row;
    const v=t.state==="needed"?"Not run yet":Object.hasOwn(CHECK_LABEL,t.state)?CHECK_LABEL[t.state]:t.state;
    return`<dt>${esc(who)}</dt><dd><span class="ht-v test-${esc(t.state)}">${esc(v)}</span>${at?` · <span title="${esc(t.at)}">${esc(ago(at))}</span>`:""}${t.detail?`<br>${safe(t.detail)}`:""}</dd>`}).join("");
  const steps=htStepsHtml(c);
  // typeof on the names themselves, so a const or a function from #101 or #108 both count.
  const part=f=>{try{return f?f(c)||"":""}catch(e){return""}};
  const ev=part(typeof evidenceHtml==="function"?evidenceHtml:null),ch=part(typeof changesHtml==="function"?changesHtml:null);
  const pf=part(typeof proofSectionHtml==="function"?x=>proofSectionHtml(x,true):null);
  return`<div class="shipbox htbox"><h3>Human test</h3>${rows?`<dl>${rows}</dl>`:""}<h4>To try</h4>${steps}${ev}${pf}${ch}</div>`;
}
// The hand-over steps (card #125): through proof.js, so each path shows as its file name (a chip
// that shows the picture or report); stkMd alone when proof.js isn't loaded.
function htStepsHtml(c){
  if(!c.nextStep)return`<p class="note">No hand-over steps on this card yet.</p>`;
  let html="";
  try{html=typeof proofSteps==="function"?proofSteps(c):stkMd(c.nextStep)}catch(e){html=stkMd(c.nextStep)}
  return`<div class="ht-steps">${html}</div>`;
}
// The review view's box (card #120): before and after shots side by side, one verdict table,
// then (review-mode.css orders them) done-when.js's checks and the hand-over steps. Its parts sit
// in #p-det beside the "Done when" block, so the checks are drawn once, by done-when.js.
let rvTryOpen=true; // the "To try" fold, as Hugh last left it while the panel is open
document.addEventListener("toggle",e=>{const t=e.target;if(t&&t.matches&&t.matches(".rv-try"))rvTryOpen=t.open},true);
const RV_ROWS=[["critique","Critic"],["test","Tester"],["review","Reviewer"],["repro","Reproducer"]];
const RV_WORD={passed:"PASS",failed:"FAIL",unclear:"UNCLEAR",needed:"NOT RUN",reproduced:"REPRODUCED",fixed:"FIXED","not-reproduced":"NOT REPRODUCED"};
const RV_TONE={passed:"ok",fixed:"fix",failed:"bad",unclear:"warn",reproduced:"warn","not-reproduced":"warn",needed:"none"};
function rvFigures(c){
  const figs=[];
  const runs=Array.isArray(c.evidence)?c.evidence.filter(r=>r&&typeof r==="object"):[];
  const r=runs[0];
  if(r){
    const shots=(Array.isArray(r.shots)?r.shots:[]).filter(x=>x&&typeof x.path==="string"&&x.path&&!SECRET.test(x.path)).sort((a,b)=>(Number(a.n)||0)-(Number(b.n)||0));
    for(const x of shots)figs.push({path:x.path,at:r.at,cap:`${x.kind==="before"?"Before":x.kind==="after"?"After":"Shot"} · tester's shot`,note:x.caption||""});
  }
  const rp=checkOf(c,"repro");
  if(rp)for(const [k,w] of [["before","Before the fix"],["after","After the fix"]]){const x=rp[k];
    if(x&&typeof x==="object"&&typeof x.shot==="string"&&x.shot&&!SECRET.test(x.shot))figs.push({path:x.shot,at:x.at,cap:`${w} · reproducer's shot`,note:""})}
  return figs;
}
function rvShotsHtml(c){
  const figs=typeof shotImg==="function"?rvFigures(c):[];
  // The card's proof pictures (card #125) follow the agents' shots; proof.js loads them itself.
  let pf=[];try{pf=typeof proofStripFigs==="function"?proofStripFigs(c):[]}catch(e){pf=[]}
  if(!figs.length&&!pf.length)return`<p class="rv-noshots">No shots</p>`;
  const n=figs.length+pf.length;
  return`<div class="rv-ev${n===1?" one":""}">${figs.map(f=>`<figure><figcaption>${esc(f.cap)}</figcaption><div class="rv-shot">${shotImg(f.path,f.at)}</div>${f.note?`<p class="rv-note">${safe(f.note)}</p>`:""}</figure>`).join("")}${pf.join("")}</div>`;
}
function rvVerdictsHtml(c){
  const rows=RV_ROWS.map(([k,who])=>{const t=checkOf(c,k);if(!t)return"";const at=Date.parse(t.at||"");
    const word=RV_WORD[t.state]||String(t.state).toUpperCase();
    return`<tr><th scope="row" class="who">${esc(who)}</th><td class="v ${RV_TONE[t.state]||"warn"}">${esc(word)}</td><td class="d">${t.detail?safe(t.detail):""}${at?` <span class="rv-at" title="${esc(t.at)}">${esc(ago(at))}</span>`:""}</td></tr>`}).join("");
  return rows?`<table class="rv-verdict"><caption class="rv-sr">What the agents found</caption><tbody>${rows}</tbody></table>`:"";
}
function rvBoxHtml(c){
  const steps=htStepsHtml(c);
  let pf="";try{pf=typeof proofSectionHtml==="function"?proofSectionHtml(c,false):""}catch(e){pf=""}
  return`<div class="htbox rv-box"><div class="rv-shots">${rvShotsHtml(c)}</div>${pf}${rvVerdictsHtml(c)}<details class="rv-try"${rvTryOpen?" open":""}><summary>To try</summary>${steps}</details></div>`;
}
document.addEventListener("click",e=>{const s=e.target.closest(".rv-ev img.shot[src]");if(s&&!document.querySelector(".shotview")){e.preventDefault();openShot(s)}});
function htBox(){
  if(!panel)return;
  const d=panel.el.querySelector("#p-det");if(!d)return;
  const old=d.querySelector(".htbox"),c=htOpen();
  if(!c){if(old)old.remove();return}
  const tmp=document.createElement("div");tmp.innerHTML=rvView==="review"?rvBoxHtml(c):htBoxHtml(c);const box=tmp.firstElementChild;
  if(old)old.replaceWith(box);
  else{const at=d.querySelector(":scope > .shipbox, :scope > label.f");at?at.before(box):d.append(box)}
  showShots(box);
  if(typeof showEvidence==="function")try{showEvidence(box)}catch(e){}
  if(typeof proofFill==="function")try{proofFill(box)}catch(e){}
}
document.addEventListener("click",e=>{const s=e.target.closest(".htbox .ht-steps img.shot[src]");if(s){e.preventDefault();openShot(s)}});

// renderDetail redraws the details on every board change: the queue follows the cards, and the
// box and the bar are drawn again.
// The details keep their scroll: the box comes back after board.js has drawn the rest.
{const draw=renderDetail;renderDetail=function(){
  const d=typeof panel!=="undefined"&&panel?panel.el.querySelector("#p-det"):null,top=d?d.scrollTop:0;
  draw.apply(this,arguments);rqSync();htBox();htDraw();
  if(d&&d.isConnected&&htOpen()&&d.scrollTop!==top)d.scrollTop=top}}

// One key handler for Human test review. Card #116's stack overlay adds a target of its own with
// htKeys.add(target) (and removes it with htKeys.remove); the newest active target gets the keys.
// A target: {active(), root(), prev(), next(), pass(), fail(), undo(), confirming(), confirm(),
// cancel()}, and optionally keys(): the keys it takes now ("p", "f", "u", "arrowleft",
// "arrowright"); others pass through to the page.
const htKeys={targets:[],add(t){if(!this.targets.includes(t))this.targets.push(t)},remove(t){this.targets=this.targets.filter(x=>x!==t)}};
const HT_MODS=["Shift","Control","Alt","Meta","CapsLock","AltGraph"];
// True when the key isn't review's: Hugh is typing, or something else is on top.
function htBlocked(t,arrow){
  const a=document.activeElement;
  if(a&&a!==document.body&&a.matches&&(a.matches("input,textarea,select,[contenteditable],[contenteditable] *")||a.closest(".term,#p-q,.shipask,.xterm")))return true;
  // The Settings overlay, a full-size screenshot, the Ctrl+K bar, and the stack's own confirms
  // (Detach, drop to stack) are never review's, even over the stack overlay.
  if(document.querySelector(".st-wrap,.shotview,#cmdbar,dialog.stk-dlg[open]"))return true;
  const root=t.root&&t.root();
  if([...document.querySelectorAll("dialog[open]")].some(d=>!(root&&(d===root||d.contains(root)))))return true;
  if(arrow)for(let n=a;n&&n!==document.body&&n!==root;n=n.parentElement){
    if(n.scrollWidth>n.clientWidth+1&&/(auto|scroll)/.test(getComputedStyle(n).overflowX))return true;
  }
  return false;
}
document.addEventListener("keydown",e=>{
  const t=[...htKeys.targets].reverse().find(x=>{try{return x.active()}catch(err){return false}});if(!t)return;
  if(t.confirming()){
    if(HT_MODS.includes(e.key))return;
    e.preventDefault();e.stopImmediatePropagation();
    if(e.key==="Enter"&&!e.repeat&&!e.ctrlKey&&!e.altKey&&!e.metaKey&&!e.shiftKey)t.confirm();else t.cancel();
    return;
  }
  if(e.ctrlKey||e.altKey||e.metaKey||e.repeat||e.isComposing)return;
  const k=e.key,arrow=k==="ArrowLeft"||k==="ArrowRight";
  if(!(arrow||/^[pfuPFU]$/.test(k)))return;
  const allow=t.keys?t.keys():null;if(allow&&!allow.includes(k.toLowerCase()))return;
  if(htBlocked(t,arrow))return;
  e.preventDefault();e.stopPropagation();
  if(k==="ArrowLeft")t.prev();else if(k==="ArrowRight")t.next();
  else if(k==="p"||k==="P")t.pass();else if(k==="f"||k==="F")t.fail();else t.undo();
});
// A click anywhere but the confirm's own buttons cancels "Ship #n?".
document.addEventListener("mousedown",e=>{if(htConfirm&&!e.target.closest('[data-ht="yes"],[data-ht="no"]'))htConfirmNo()},true);
// The panel's target: the open card in the queue. U also works with only a Pass pending.
htKeys.add({
  active:()=>!!htOpen()||htPending.length>0,
  keys:()=>htOpen()?null:["u"], // with only a Pass pending, U alone; the board keeps its keys
  root:()=>panel?panel.el:null,
  prev:()=>{if(htOpen())htGo(htNeighbour(-1))},
  next:()=>{if(htOpen())htGo(htNeighbour(1))},
  pass:()=>htPass(),
  fail:()=>htFailStart(),
  undo:()=>htUndo(),
  confirming:()=>!!htConfirm&&!!htOpen(),
  confirm:()=>htConfirmYes(),
  cancel:()=>htConfirmNo(),
});
