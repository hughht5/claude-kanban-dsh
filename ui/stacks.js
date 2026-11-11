// Stacked cards (design/stacked-cards/SPEC.md, option B): a lead card (`stack: [n, …]`) shows as
// one stack card with a progress strip, the stack's links once, and its children as rows that
// open in an overlay (how to test, Passed, Found a problem, answers). Children (`stackOf: n`) whose lead
// is on the board and not archived leave the columns. Uses board.js's helpers ($, esc, safe,
// httpOk, loginsOf, cards, colOf, isArch, cardNum, matches, render, invoke, errText, status) and
// panel.js's openPanel. board.js calls stackHidden, stackMatches, stackDecorate and stackDetail.
// Card #116: a stack in Human test is reviewed in the overlay, a stop in card #102's queue
// (review.js, in this branch's base: reviewStackStop, reviewQueue, reviewStartAt, reviewStepFrom,
// reviewClosed, reviewShip, htKeys, htChip, htUndo, htPassState, htPending), with board.js's
// checkRows, checkOf and CHECKS, panel.js's showShots and openShot, and card #101's evidenceHtml
// and showEvidence when they exist.
const STK_READY=["Human test","Ship it","Deploy complete","Done"];
const STK_SHIPPED=["Ship it","Deploy complete","Done"];
const STK_ICON=`<svg class="stk-ic" viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 4 4 8l8 4 8-4-8-4"/><path d="m4 12 8 4 8-4"/><path d="m4 16 8 4 8-4"/></svg>`;
const stkOpen={},stkForm={};   // lead number -> expanded; child id -> {kind,text}
let stkDlg=null;              // the child open in the overlay: {lead: lead id, kid: child id (the lead's id for the stack page), el: the <dialog>, parts, rv: in review}
try{Object.assign(stkOpen,JSON.parse(localStorage.getItem("wb.stackOpen")||"{}"))}catch(e){}
const stkSave=()=>{try{localStorage.setItem("wb.stackOpen",JSON.stringify(stkOpen))}catch(e){}};
const STK_TA=".stk-kdlg textarea[data-f]";   // a child's problem or answer box (paste.js matches it too)

// The stacks on the board, worked out once per list of cards (load() replaces the array).
let stkIdx=null,stkIdxOf=null;
function stkIndex(){
  if(stkIdx&&stkIdxOf===cards)return stkIdx;
  const byNum=new Map();for(const c of cards)if(cardNum(c))byNum.set(cardNum(c),c);
  const kids=new Map(),hidden=new Set();
  for(const lead of cards){
    if(!Array.isArray(lead.stack)||!cardNum(lead))continue;
    const list=[],seen=new Set([lead.id]);
    const add=c=>{if(c&&!seen.has(c.id)&&!isArch(c)){seen.add(c.id);list.push(c)}};
    for(const n of lead.stack)add(byNum.get(Number(n)));
    // A child that names this lead but isn't in its list still belongs here, after the others.
    for(const c of cards)if(Number(c.stackOf)===cardNum(lead))add(c);
    if(!list.length)continue;
    kids.set(lead.id,list);
    if(!isArch(lead))for(const c of list)hidden.add(c.id);
  }
  stkIdx={kids,hidden};stkIdxOf=cards;return stkIdx;
}
const stackKids=c=>stkIndex().kids.get(c.id)||[];
const isStackLead=c=>!isArch(c)&&stackKids(c).length>0;
function stackHidden(c){return stkIndex().hidden.has(c.id)&&!isStackLead(c)}
function stackMatches(c){return matches(c)||(isStackLead(c)&&stackKids(c).some(matches))}
const stkLead=c=>{const n=Number(c.stackOf);return n?cards.find(x=>cardNum(x)===n):null};

// A child's state: what its dot, segment and right-hand text say.
function kidState(c){
  const col=colOf(c);
  if(STK_SHIPPED.includes(col))return{k:"pass",label:col==="Ship it"?"Shipping":"Done"};
  if(c.passed===true&&col==="Human test")return{k:"pass",label:"Passed"};
  if(col==="Blocked"||(c.blockedReason&&!STK_READY.includes(col)))return{k:"need",label:"Needs you"};
  if(col==="Human test"){const n=stepCount(c.nextStep);return{k:"test",label:n?`Test · ${n} step${n===1?"":"s"}`:"To test"}}
  return{k:"build",label:"Building"};
}
const stepCount=t=>(String(t||"").match(/^\s*\d+[.)]\s/gm)||[]).length;
const stkMd=t=>{const s=String(t||"");if(SECRET.test(s))return'<span class="hidden-secret">hidden: looks like a secret</span>';
  try{if(window.marked&&window.DOMPurify)return DOMPurify.sanitize(marked.parse(s,{gfm:true,breaks:true}))}catch(e){}
  return esc(s).replace(/\n/g,"<br>")};

// The dev stack: up or down from the first local login (launch.rs stack_up), checked at most once a minute.
const stkUp=new Map();   // url -> {up, at, pending}
function stackState(c){
  const l=loginsOf(c).find(x=>httpOk(x.url)),url=l?l.url:httpOk(c.devStack)?c.devStack:"";
  if(!url)return c.devStack?"stack":"no stack";
  const s=stkUp.get(url);
  if(!s||(!s.pending&&Date.now()-s.at>60000)){
    stkUp.set(url,{up:s?s.up:null,at:Date.now(),pending:true});
    invoke("stack_up",{url}).then(up=>{const was=s&&s.up;stkUp.set(url,{up:!!up,at:Date.now()});if(was!==!!up)render()},
      ()=>{stkUp.set(url,{up:null,at:Date.now()})});
  }
  const up=stkUp.get(url).up;
  return up===true?"stack up":up===false?`<span class="stk-down">stack down</span>`:"stack";
}

const LEG={test:"to test",pass:"passed",need:"needs you",build:"building"};
function legendText(kids,counts){
  const done=kids.filter(c=>STK_SHIPPED.includes(colOf(c))).length,passed=counts.pass-done;
  const green=!counts.pass?"":!done?`${counts.pass} passed`:!passed?`${counts.pass} done`:`${counts.pass} passed or done`;
  return["test","pass","build","need"].filter(k=>counts[k]).map(k=>({k,t:k==="pass"?green:`${counts[k]} ${LEG[k]}`}));
}

// The stack card: board.js's cardEl made the element (drag, keys, opening the lead); this fills it.
function stackDecorate(b,c){
  if(!isStackLead(c))return;
  const kids=stackKids(c),lead=cardNum(c),states=kids.map(kidState);
  const counts={test:0,pass:0,need:0,build:0};for(const s of states)counts[s.k]++;
  const ready=kids.filter(k=>STK_READY.includes(colOf(k))).length;
  const searchHit=q&&!matches(c)&&kids.some(matches);
  const open=!!stkOpen[lead]||searchHit;
  const legend=legendText(kids,counts);
  const logins=loginsOf(c).filter(l=>httpOk(l.url));
  const links=[...logins.map(l=>`<button class="stk-link" type="button" data-act="login" data-i="${loginsOf(c).indexOf(l)}">Sign in as ${safe(l.label||l.persona||"user")}</button>`),
    c.pr&&httpOk(c.pr)?`<a class="stk-link" href="${esc(c.pr)}" target="_blank" rel="noopener">PR</a>`:"",
    `<button class="stk-link" type="button" data-act="session">Open session</button>`].filter(Boolean);
  b.classList.add("stk");
  b.innerHTML=`<div class="stk-top"><span class="t">${STK_ICON}${typeof needIcon==="function"?needIcon(c.id):""}${typeof stateDot==="function"?stateDot(c.id):""}${numBadge(c)}${safe(c.title||"Untitled stack")}</span><span class="stk-ready">${ready} of ${kids.length} ready</span></div>
  ${c.summary?`<span class="s stk-sum">${safe(c.summary)}</span>`:""}
  <div class="stk-sub">${c.branch?`<code>${safe(c.branch)}</code> · `:""}${stackState(c)}${typeof shipChip==="function"?" "+shipChip(c):""}</div>
  <div class="stk-strip" role="img" aria-label="${esc(legend.map(x=>x.t).join(", "))}">${kids.map((k,i)=>`<span class="stk-seg s-${states[i].k}" title="${esc("#"+(cardNum(k)||"")+" "+(k.title||"")+" · "+states[i].label)}"></span>`).join("")}</div>
  <div class="stk-leg">${legend.map(x=>`<span><span class="stk-dot s-${x.k}"></span>${esc(x.t)}</span>`).join("")}</div>
  <div class="stk-x" data-noopen>
   <div class="stk-links">${links.join("")}</div>
   <button class="stk-tog" type="button" data-act="toggle" aria-expanded="${open}"><span class="chev">${open?"▾":"▸"}</span>${open?"Hide":"Show"} ${kids.length} card${kids.length===1?"":"s"}</button>
   ${open?`<div class="stk-kids">${kids.map((k,i)=>kidRow(c,k,states[i])).join("")}</div>`:""}
  </div>`;
  b.addEventListener("click",e=>stkClick(e,c));
  stkReviewEntry(b,c);   // card #116: in Human test, a click opens the stack's review
  b.addEventListener("input",e=>{const f=e.target.closest("[data-f]");if(f&&stkForm[f.dataset.f])stkForm[f.dataset.f].text=f.value});
  b.addEventListener("keydown",e=>{const f=e.target.closest("[data-f]");if(!f)return;
    if(e.key==="Escape"){e.stopPropagation();delete stkForm[f.dataset.f];render()}
    else if(e.key==="Enter"&&(e.ctrlKey||e.metaKey)){e.preventDefault();const s=f.parentNode.querySelector('[data-act="send"]');if(s)s.click()}});
  // Pressing in the links or the child rows never drags the stack (text in a box can be selected);
  // the stack's title, strip and legend still drag it like any card.
  const zone=t=>t&&t.closest&&t.closest("[data-noopen],button,a,textarea,input,select");
  b.addEventListener("pointerdown",e=>{if(b.draggable&&zone(e.target)){b.draggable=false;
    const back=()=>{b.draggable=canWrite&&!isArch(c);document.removeEventListener("pointerup",back,true);document.removeEventListener("pointercancel",back,true)};
    document.addEventListener("pointerup",back,true);document.addEventListener("pointercancel",back,true)}});
}

// A child's row in the stack card: its state, and a click opens it in the overlay.
function kidRow(lead,k,st){
  return`<div class="stk-kid"><button class="stk-row" type="button" data-act="kid" data-kid="${esc(k.id)}" aria-haspopup="dialog" title="Open #${esc(cardNum(k)||"")} to test it">
    <span class="stk-name"><span class="stk-dot s-${st.k}"></span><span class="num">${esc(cardNum(k)||"")}</span>${safe(k.title||"Untitled card")}</span>
    <span class="stk-state s-${st.k}">${esc(st.label)} <span class="chev">▸</span></span></button></div>`;
}

// What the overlay shows for a child: its how-to-test (scrolls), then what Hugh can do (stays put).
// In review (card #116: its stack is a stop in #102's Human test queue) the bar takes Pass and
// Fail, so the foot keeps only the problem box, Open card and Detach.
function kidParts(lead,k,st,rv){
  const f=stkForm[k.id],col=colOf(k),ready=STK_READY.includes(col);
  const steps=st.k==="need"?"":k.nextStep?`<div class="stk-md">${stkMd(k.nextStep)}</div>`:`<p class="stk-q">No how-to-test steps on this card yet.</p>`;
  let acts="";
  if(st.k==="need"){
    const choices=Array.isArray(k.choices)?k.choices.filter(x=>typeof x==="string"&&x.trim()):[];
    acts=`<div class="stk-ask"><b>Question:</b> ${safe(k.blockedReason||"The card is blocked; its session says why in the card.")}</div>
    ${choices.length?`<div class="stk-acts">${choices.map((x,i)=>`<button class="btn ghost sm" type="button" data-act="answer" data-kid="${esc(k.id)}" data-i="${i}">${safe(x)}</button>`).join("")}</div>`
      :`<div class="stk-form"><textarea rows="3" data-f="${esc(k.id)}" data-kind="answer" placeholder="Your answer, sent to the stack's session. Paste screenshots here too.">${esc(f&&f.kind==="answer"?f.text:"")}</textarea>
      <div class="stk-acts"><button class="btn sm" type="button" data-act="send" data-kid="${esc(k.id)}" data-kind="answer">Send</button></div></div>`}`;
  }else if(rv){
    if(f&&f.kind==="problem")acts=stkFailForm(lead,k,f);
  }else if(ready){
    const problem=f&&f.kind==="problem";
    acts=problem?`<div class="stk-form"><textarea rows="4" data-f="${esc(k.id)}" data-kind="problem" placeholder="What went wrong? Paste screenshots here too. It goes to the stack's session and the card goes back to In progress.">${esc(f.text||"")}</textarea>
      <div class="stk-acts"><button class="btn sm" type="button" data-act="send" data-kid="${esc(k.id)}" data-kind="problem">Send</button><button class="btn ghost sm" type="button" data-act="cancel" data-kid="${esc(k.id)}">Cancel</button></div></div>`
      :`<div class="stk-acts">${col==="Human test"?`<button class="btn sm${k.passed?" ghost":""}" type="button" data-act="passed" data-kid="${esc(k.id)}">${k.passed?"Undo passed":"Passed"}</button>`:""}<button class="btn ghost sm" type="button" data-act="problem" data-kid="${esc(k.id)}">Found a problem</button></div>`;
  }
  const foot=`<div class="stk-foot"><button class="stk-link" type="button" data-act="card" data-kid="${esc(k.id)}">Open card</button><span class="stk-col">${esc(col)}</span>${canWrite?`<button class="stk-quiet" type="button" data-act="detach" data-kid="${esc(k.id)}" title="Build it as its own card, with its own session">Detach</button>`:""}</div>`;
  return{steps,acts,foot};
}
// The Fail box in review: a child's Found a problem, or (on the stack page) the stack's Fail.
function stkFailForm(lead,k,f){
  const page=k.id===lead.id,n=esc(cardNum(k)||"");
  const what=page?`stack #${n} goes back to In progress and its session gets your note`
    :`#${n} goes back to In progress, the stack's session gets your note, and the stack can leave Human test with it`;
  return`<div class="stk-form"><p class="skd-failrow"><b>Failing ${page?"stack ":""}#${n}.</b> Say what's wrong. Enter sends it: ${what}. Esc cancels.</p>
    <textarea rows="4" data-f="${esc(k.id)}" data-kind="problem" placeholder="What's wrong? Paste screenshots here too.">${esc(f.text||"")}</textarea>
    <div class="stk-acts"><button class="btn sm" type="button" data-act="send" data-kid="${esc(k.id)}" data-kind="problem">Send and fail</button><button class="btn ghost sm" type="button" data-act="cancel" data-kid="${esc(k.id)}">Cancel</button></div></div>`;
}

// Card #116: reviewing a stack in Human test. #102's queue (review.js) holds the stack as one stop;
// in the overlay Hugh steps through its children in Human test, in stack order, then a stack page
// where the stack ships. The keys are #102's one handler: this file adds a target (STK_KEYS).
const STK_HT="Human test";
let stkArm=null;    // the child (or the lead, on the stack page) whose "Pass #n?" waits for Enter
let stkBusy=false;  // a Pass or a Fail is on its way
let stkQ=null;      // #102's queue, read once per draw (reviewQueue syncs and sorts every card)
let stkLeaving=null;   // the stack the overlay is stepping out of, so stepping back in lands on its last stop
const stkQueue=()=>stkQ||(typeof reviewQueue==="function"?reviewQueue():[]);
function stkRvOn(lead){return!!lead&&!isArch(lead)&&colOf(lead)===STK_HT&&stkQueue().includes(lead.id)}
const stkStops=lead=>[...stackKids(lead).filter(k=>!isArch(k)&&colOf(k)===STK_HT).map(k=>k.id),lead.id];
const stkPending=id=>typeof htPending!=="undefined"&&Array.isArray(htPending)?htPending.find(p=>p.id===id)||null:null;
function stkCur(){
  if(!stkDlg)return{};
  const lead=cards.find(x=>x.id===stkDlg.lead)||null;if(!lead||!isStackLead(lead))return{lead};
  const page=stkDlg.kid===lead.id;
  return{lead,page,k:page?lead:stackKids(lead).find(x=>x.id===stkDlg.kid)||null};
}

// A child's stage from card #113 (stack_stages): only a merged child can be passed. Read when the
// overlay draws a stack in review, once per list of cards; with no such command (before #113) there
// is no stage rule. #113's set_passed refuses an unmerged child as well.
const stkStg=new Map();let stkStgOff=false;
const STK_STAGE_WHY={waiting:"Waiting",building:"Building","to-merge":"Waiting to merge"};
function stkStgParse(r){
  const list=Array.isArray(r)?r:r&&Array.isArray(r.children)?r.children:r&&Array.isArray(r.stages)?r.stages
    :r&&typeof r==="object"?Object.entries(r).map(([n,v])=>typeof v==="string"?{number:n,stage:v}:{number:n,...v}):[];
  const m=new Map();
  for(const e of list){if(!e||typeof e.stage!=="string")continue;if(e.id)m.set(String(e.id),e);const n=Number(e.number??e.card??e.num);if(n>0)m.set("#"+n,e)}
  return m;
}
function stkStageOf(lead,k){
  if(stkStgOff||!lead)return null;
  const s=stkStg.get(lead.id);
  if((!s||s.of!==cards)&&!(s&&s.pending)){
    stkStg.set(lead.id,{of:cards,map:s?s.map:null,pending:true});
    const of=cards;
    invoke("stack_stages",{lead:cardNum(lead)}).then(r=>{stkStg.set(lead.id,{of,map:stkStgParse(r)});stkDlgDraw()},
      e=>{if(/command\s+stack_stages\s+not\s+found/i.test(errText(e)))stkStgOff=true;stkStg.set(lead.id,{of,map:null})});   // only "no such command" turns the rule off
  }
  const m=s&&s.map;return m?m.get(k.id)||m.get("#"+cardNum(k))||null:null;
}
function stkWaitsOn(g){const w=g.waitsOn??g.waits_on??g.blockedBy;return(Array.isArray(w)?w:w!=null?[w]:[]).map(Number).filter(n=>n>0).map(n=>"#"+n).join(", ")}

// Can Hugh pass this child (or, on the stack page, ship the stack)? {ok, why, passed}: #102's
// htPassState, plus, for a child, its column, its merge record and #113's stage.
function stkPassState(lead,k){
  if(k.id===lead.id)return htPassState(lead);
  const col=colOf(k);
  if(col!==STK_HT)return{ok:false,why:`It's in ${col}`};
  const base=htPassState(k);if(!base.ok)return base;
  const m=checkOf(k,"merge");
  if(m&&m.state!=="passed")return{ok:false,why:m.state==="failed"?"Merge failed: the lead merges it again first":"Merge unclear: the lead merges it again first"};
  const g=stkStageOf(lead,k);
  if(g&&Object.hasOwn(STK_STAGE_WHY,g.stage)){const w=stkWaitsOn(g);return{ok:false,why:g.stage==="waiting"&&w?`Waiting on ${w}`:STK_STAGE_WHY[g.stage]}}
  return{ok:true};
}

// The review bar, in #102's look (review.css's .rbar classes and its htChip): where Hugh is, the
// verdict chips, Fail and Pass. Its buttons go through the overlay's click handler (stkClick).
function stkBarHtml(lead,k){
  const stops=stkStops(lead),i=stops.indexOf(k.id),page=k.id===lead.id;
  const q=stkQueue(),qi=q.indexOf(lead.id);
  const prevOn=i>0||qi>0,nextOn=(i>=0&&i<stops.length-1)||(qi>=0&&qi<q.length-1);
  const L=esc(cardNum(lead)||""),n=esc(cardNum(k)||"");
  const pos=`Stack #${L} · ${page?"ship it":`${i+1} of ${stops.length-1}`}${qi>=0?` · Human test ${qi+1} of ${q.length}`:""}`;
  const chips=CHECKS.map(ch=>htChip(k,ch)).join("");
  const st=stkPassState(lead,k),pend=page&&stkPending(lead.id);
  let acts;
  if(stkArm===k.id)acts=`<span class="ht-ask" role="alert">${page?`Ship stack #${L}?`:`Pass #${n}?`} <b>Enter</b> confirms, any other key cancels.</span><button class="btn" type="button" data-act="rv-yes">${page?"Ship it":"Pass"}</button><button class="btn ghost" type="button" data-act="rv-no">Cancel</button>`;
  // This stack's own Undo (U, #102's key, undoes the newest pending Pass, whichever card it is).
  else if(pend)acts=`<span class="ht-wait">Shipping when the countdown ends</span><button class="btn ghost" type="button" data-act="rv-undo" data-kid="${esc(lead.id)}">Undo</button>`;
  else{
    const failOn=!!(stkForm[k.id]&&stkForm[k.id].kind==="problem");
    const fail=`<button class="btn ghost ht-fail" type="button" data-act="rv-fail" aria-keyshortcuts="F"${failOn?" disabled":""}>Fail… <kbd>F</kbd></button>`;
    const pass=st.passed?`<span class="ht-done">Passed</span><button class="btn ghost" type="button" data-act="rv-unpass" data-kid="${esc(k.id)}">Undo passed</button>`
      :st.ok?`<button class="btn ht-pass" type="button" data-act="rv-pass" aria-keyshortcuts="P"${failOn?" disabled":""}>${page?"Pass: ship the stack":"Pass"} <kbd>P</kbd></button>`
      :`<button class="btn ht-pass" type="button" disabled title="${esc(st.why||"")}">Pass <kbd>P</kbd></button><span class="ht-why">${esc(st.why||"")}</span>`;
    acts=fail+pass;
  }
  return`<button class="ibtn" type="button" data-act="rv-prev" aria-keyshortcuts="ArrowLeft" title="Previous (Left)"${prevOn?"":" disabled"}>‹ Previous</button>
  <span class="ht-pos">${pos}</span>
  <button class="ibtn" type="button" data-act="rv-next" aria-keyshortcuts="ArrowRight" title="Next (Right)"${nextOn?"":" disabled"}>Next ›</button>
  <span class="ht-chips">${chips}</span><span class="ht-acts">${acts}</span><span class="ht-keys">← → step · P pass · F fail</span>`;
}

// The overlay is drawn in parts, and each part is replaced only when its own HTML changes, so a
// redraw (the board's every few seconds, the "ago" times each minute) leaves the rest alone: the
// evidence keeps its clicks and the bar its focus.
const STK_PARTS=["head","bar","checks","ev","steps","foot"];
const STK_SKEL=`<div class="skd-head" data-p="head"></div><div class="rbar skd-rbar" data-p="bar" tabindex="-1" role="toolbar" aria-label="Human test review" hidden></div><div class="skd-body"><div data-p="checks" hidden></div><div data-p="ev" hidden></div><div data-p="steps" hidden></div></div><div class="skd-foot" data-p="foot"></div>`;
const STK_X=`<button class="skd-x" type="button" data-act="close" aria-label="Close" title="Close (Esc)">×</button>`;
const stkLogins=lead=>{const all=loginsOf(lead);return all.filter(l=>httpOk(l.url)).map(l=>`<button class="stk-link" type="button" data-act="login" data-i="${all.indexOf(l)}">Sign in as ${safe(l.label||l.persona||"user")}</button>`).join("")};
const stkChecks=c=>typeof checkRows==="function"?`<dl class="skd-checks">${checkRows(c)}</dl>`:"";
const stkEv=c=>{try{return typeof evidenceHtml==="function"?evidenceHtml(c)||"":""}catch(e){return""}};
function stkKidHtml(lead,k,rv){
  const st=kidState(k),{steps,acts,foot}=kidParts(lead,k,st,rv);
  return{head:`<div class="skd-lead">${STK_ICON}Stack <span class="num">${esc(cardNum(lead)||"")}</span>${safe(lead.title||"Untitled stack")}</div>
    <h2 class="skd-t"><span class="stk-dot s-${st.k}"></span><span class="num">${esc(cardNum(k)||"")}</span>${safe(k.title||"Untitled card")}</h2>
    <div class="skd-meta"><span class="stk-state s-${st.k}">${esc(st.label)}</span>${stkLogins(lead)}</div>${STK_X}`,
    bar:rv?stkBarHtml(lead,k):"",checks:stkChecks(k),ev:stkEv(k),steps,foot:acts+foot};
}
// The stack page: the last stop in a stack's review. Its Pass ships the whole stack.
function stkPageHtml(lead){
  const kids=stackKids(lead),done=kids.filter(x=>x.passed===true||STK_SHIPPED.includes(colOf(x))).length,f=stkForm[lead.id];
  // A child in Human test is a stop (its row opens it); the others are listed for their state only.
  const rows=kids.map(x=>{const s=kidState(x),stop=STK_HT===colOf(x),g=stop?stkStageOf(lead,x):null,why=g&&Object.hasOwn(STK_STAGE_WHY,g.stage)?STK_STAGE_WHY[g.stage]:"";
    const inner=`<span class="stk-name"><span class="stk-dot s-${s.k}"></span><span class="num">${esc(cardNum(x)||"")}</span>${safe(x.title||"Untitled card")}</span><span class="stk-state s-${s.k}">${esc(why||s.label)}</span>`;
    return`<li>${stop?`<button class="stk-row" type="button" data-act="kid" data-kid="${esc(x.id)}">${inner}</button>`:`<div class="stk-row">${inner}</div>`}</li>`}).join("");
  return{head:`<div class="skd-lead">${STK_ICON}Stack <span class="num">${esc(cardNum(lead)||"")}</span>${safe(lead.title||"Untitled stack")}</div>
    <h2 class="skd-t">Ship the stack as one</h2>
    <div class="skd-meta"><span class="stk-state">${done} of ${kids.length} passed</span>${stkLogins(lead)}</div>${STK_X}`,
    bar:stkBarHtml(lead,lead),checks:`<ul class="skd-kids">${rows}</ul>${stkChecks(lead)}`,ev:stkEv(lead),
    steps:lead.nextStep?`<div class="stk-md">${stkMd(lead.nextStep)}</div>`:`<p class="stk-q">No how-to-test steps on the stack's lead.</p>`,
    foot:`${f&&f.kind==="problem"?stkFailForm(lead,lead,f):""}<div class="stk-foot"><button class="stk-link" type="button" data-act="session">Open the lead's session</button><span class="stk-col">${esc(colOf(lead))}</span></div>`};
}

// The overlay: one child at a time, over the dimmed board, wide enough to read its steps. A second
// open while it is open is a step: the same dialog, so no close event, no flicker, and the queue
// stays. Hugh's close (×, the backdrop, Esc) ends #102's queue; stkDlgClose (a step out, a Send, a
// detach) doesn't.
function stkKidOpen(leadId,kidId){
  if(stkDlg&&stkDlg.el.open){
    if(stkDlg.lead!==leadId||stkDlg.kid!==kidId)stkArm=null;
    stkDlg.lead=leadId;stkDlg.kid=kidId;stkDlg.focusBar=true;stkDlgDraw();if(typeof navNote==="function")navNote();return;
  }
  stkDlgClose();
  const d=document.createElement("dialog");d.className="stk-kdlg";d.innerHTML=STK_SKEL;
  const s={lead:leadId,kid:kidId,el:d,parts:{},focusBar:false,rv:false,inQueue:false,quiet:false,drawn:null};stkDlg=s;
  d.addEventListener("click",e=>{
    if(e.target===d){d.close();return}   // the backdrop
    const t=e.target.closest("[data-act]");if(t&&t.dataset.act==="close"){e.preventDefault();d.close();return}
    const shot=e.target.closest("img.shot[src]");if(shot){e.preventDefault();openShot(shot);return}   // a screenshot in the steps, full size
    const lead=cards.find(x=>x.id===s.lead);if(lead)stkClick(e,lead)});
  // A click anywhere but the confirm's own buttons cancels "Pass #n?". The bar is redrawn after the
  // click, so the button under the pointer still gets it (Fail, Next).
  d.addEventListener("click",e=>{if(stkArm&&!e.target.closest('[data-act="rv-yes"],[data-act="rv-no"]')){stkArm=null;setTimeout(stkDlgDraw,0)}},true);
  // Keep what's typed (an answer box has no entry until the first key), so a redraw keeps it.
  d.addEventListener("input",e=>{const f=e.target.closest("[data-f]");if(!f)return;
    const id=f.dataset.f;if(!stkForm[id])stkForm[id]={kind:f.dataset.kind,text:""};stkForm[id].text=f.value});
  // In a box: Enter sends (Shift+Enter is a new line), as in the panel's message box; Esc closes the
  // box (its text is dropped), not the overlay. Esc anywhere else closes the overlay.
  d.addEventListener("keydown",e=>{const f=e.target.closest("[data-f]");if(!f||e.isComposing)return;
    if(e.key==="Escape"){e.preventDefault();e.stopPropagation();delete stkForm[f.dataset.f];stkDlgDraw();stkFocusBar()}
    else if(e.key==="Enter"&&!e.shiftKey&&!e.altKey){e.preventDefault();const b=f.parentNode.querySelector('[data-act="send"]');if(b&&!b.disabled)b.click()}});
  // With "Pass #n?" showing, Esc only cancels it. #102's key handler takes that Esc first; this is the backstop.
  d.addEventListener("cancel",e=>{if(stkArm){e.preventDefault();stkConfirmNo()}});
  d.addEventListener("close",()=>{
    stkBannerOut(d);d.remove();
    if(stkDlg===s){stkDlg=null;stkArm=null}
    if(!s.quiet&&s.inQueue)reviewClosed(false);   // Hugh's close ends the queue, as the panel's does
    if(typeof navNote==="function")navNote(); // nav.js (card #187)
  });
  document.body.appendChild(d);
  stkDlgDraw();
  if(stkDlg!==s)return;   // the child is no longer in the stack
  try{d.showModal()}catch(e){d.setAttribute("open","")}
  stkBannerIn();stkFocusBar();
  if(typeof navNote==="function")navNote(); // nav.js: Back and Forward (card #187)
}
function stkDlgClose(){if(stkDlg){const s=stkDlg;s.quiet=true;stkDlg=null;stkArm=null;stkBannerOut(s.el);if(s.el.open)s.el.close();s.el.remove();if(typeof navNote==="function")navNote();}}
function stkDlgDraw(){
  if(!stkDlg)return;
  const s=stkDlg,{lead,page,k}=stkCur();
  if(!lead||!k){stkDlgClose();return}   // detached, removed, archived, or the stack is gone
  stkQ=typeof reviewQueue==="function"?reviewQueue():[];
  try{
    const inReview=stkRvOn(lead);
    if(page&&!inReview){stkDlgClose();return}   // the stack page is only for review: the stack shipped or left
    s.inQueue=inReview;s.rv=inReview&&(page||colOf(k)===STK_HT);
    if(!s.rv)stkArm=null;
    const parts=page?stkPageHtml(lead):stkKidHtml(lead,k,s.rv),d=s.el;
    const bar=d.querySelector('[data-p="bar"]'),body=d.querySelector(".skd-body");
    const hadBar=bar.contains(document.activeElement),fresh=s.drawn!==s.kid,top=body.scrollTop;
    for(const p of STK_PARTS){
      const html=parts[p]||"";if(s.parts[p]===html)continue;
      const el=d.querySelector(`[data-p="${p}"]`);el.innerHTML=html;s.parts[p]=html;if(p!=="head"&&p!=="foot")el.hidden=!html;
      // Screenshots in the steps or the evidence load through image_read, as in the panel.
      if((p==="steps"||p==="ev")&&html&&typeof showShots==="function")showShots(el);
      if(p==="ev"&&html&&typeof showEvidence==="function")try{showEvidence(el)}catch(e){}
    }
    body.hidden=!(parts.checks||parts.ev||parts.steps);
    body.scrollTop=fresh?0:top;s.drawn=s.kid;
    if(d.open&&(s.focusBar||(hadBar&&!bar.contains(document.activeElement)))){s.focusBar=false;stkFocusBar()}
    stkBannerIn();
  }finally{stkQ=null}
}
// The focus goes on the bar (not a button: a stray Enter clicks nothing), or the × out of review.
function stkFocusBar(){
  if(!stkDlg||!stkDlg.el.open)return;
  const b=stkDlg.el.querySelector('[data-p="bar"]'),t=b&&!b.hidden?b:stkDlg.el.querySelector(".skd-x");
  if(t)t.focus({preventScroll:true});
}
// #102's Undo banner is in the page; under a modal dialog it can't be seen or clicked. It goes in
// the open overlay, and back to the page before the overlay goes, so a waiting ship keeps its Undo.
function stkBannerIn(){const b=document.getElementById("ht-pending");if(b&&stkDlg&&stkDlg.el.open&&b.parentNode!==stkDlg.el)stkDlg.el.append(b)}
function stkBannerOut(d){const b=document.getElementById("ht-pending");if(b&&d.contains(b))document.body.append(b)}

// Stepping: within the stack in place; past either end, #102's queue opens the stop before or after.
function stkStep(d){
  if(!stkDlg||!stkDlg.rv||stkBusy)return;
  const{lead}=stkCur();if(!lead)return;
  const stops=stkStops(lead),i=stops.indexOf(stkDlg.kid),j=i+(d<0?-1:1);
  if(i>=0&&j>=0&&j<stops.length){stkKidOpen(lead.id,stops[j]);return}
  stkLeaving=lead.id;reviewStepFrom(lead.id,d<0?-1:1);stkLeaving=null;
}
// After a Pass or a Fail: the next child not yet passed (after this one, then from the start), else
// the stack page. `order` is the stops as they were before the action.
function stkNextStop(lead,order,fromId){
  const now=stkStops(lead),open=id=>id!==lead.id&&now.includes(id)&&(cards.find(x=>x.id===id)||{}).passed!==true;
  return order.slice(order.indexOf(fromId)+1).find(open)||now.find(open)||lead.id;
}
// Pass is P then Enter (the router, for every card): P asks in the bar, Enter confirms.
function stkArmPass(){
  if(!stkDlg||!stkDlg.rv||stkBusy)return;
  const{lead,page,k}=stkCur();if(!k)return;
  if(!stkPassState(lead,k).ok||(stkForm[k.id]&&stkForm[k.id].kind==="problem"))return;
  stkArm=k.id;stkDlgDraw();
}
function stkConfirmNo(){if(stkArm){stkArm=null;stkDlgDraw()}}
async function stkConfirmYes(){
  const id=stkArm;stkArm=null;
  const{lead,page,k}=stkCur();
  if(!k||k.id!==id||!stkDlg.rv||stkBusy||!stkPassState(lead,k).ok){stkDlgDraw();return}
  if(page){   // #102: the 5-second Undo, then Ship it; the queue moves on
    if(!reviewShip(lead.id))status(`Stack #${cardNum(lead)} can't ship now: ${stkPassState(lead,lead).why||"it isn't ready"}`,true);
    stkBannerIn();stkDlgDraw();return;
  }
  const order=stkStops(lead),n=cardNum(k);
  stkBusy=true;stkDlgDraw();
  try{await invoke("set_passed",{id:k.id,on:true});status(`#${n} passed`)}
  catch(e){stkBusy=false;stkDlgDraw();status(`Couldn't mark #${n} passed: ${errText(e)}`,true);return}
  stkBusy=false;
  await load();   // so the next stop sees the new Passed
  if(!stkDlg||stkDlg.kid!==k.id)return;   // Hugh moved on meanwhile
  const L=cards.find(x=>x.id===lead.id);if(L)stkKidOpen(L.id,stkNextStop(L,order,k.id));
}
// Fail: F opens the problem box (#102's handler stops the "f" being typed).
function stkFailOpen(){
  if(!stkDlg||!stkDlg.rv||stkBusy)return;
  const{lead,page,k}=stkCur();if(!k||(page&&stkPending(lead.id)))return;
  stkArm=null;if(!stkForm[k.id]||stkForm[k.id].kind!=="problem")stkForm[k.id]={kind:"problem",text:""};
  stkDlgDraw();stkFocus(k.id);
}
// The Fail's Send: a child's Found a problem (only its own Passed is cleared), or the stack's Fail.
// If the stack is still in Human test, the next child shows; if it left with the card, the overlay
// closes and the queue opens the next stop still in it (the router, for card #116).
async function stkFailSend(lead,k,text,btn){
  const page=k.id===lead.id,n=cardNum(k),order=stkStops(lead);
  const q=reviewQueue(),qi=q.indexOf(lead.id);
  const after=qi<0?[]:q.slice(qi+1),before=qi<0?[]:q.slice(0,qi).reverse();
  btn.disabled=true;stkBusy=true;
  let r=null;
  try{r=page?await invoke("human_test_fail",{cardId:lead.id,note:text}):await invoke("stack_problem",{id:k.id,note:text})}
  catch(e){stkBusy=false;btn.disabled=false;status(`Not sent: ${errText(e)}`,true);return}
  stkBusy=false;delete stkForm[k.id];
  // human_test_fail's sendError: the stack went back and the note is on its log, but its session didn't get it.
  const lost=r&&r.sendError,what=page?`stack #${n}`:`#${n}`;
  const said=lost?`Failed ${what}: it's back in In progress and your note is on its log, but the session didn't get it: ${r.sendError}`
    :`Failed ${what}: it's back in In progress, and ${page?"its":"the stack's"} session has your note`;
  status(said,!!lost);
  await load();
  const L=cards.find(x=>x.id===lead.id);
  if(!page&&L&&stkRvOn(L)){if(stkDlg&&stkDlg.lead===L.id)stkKidOpen(L.id,stkNextStop(L,order,k.id));return}
  stkDlgClose();
  const live=reviewQueue(),t=after.find(id=>live.includes(id))||before.find(id=>live.includes(id));
  if(t&&reviewStartAt(t))return;
  if(!lost)status(`${said}. Human test is clear.`);
}
// #102's key handler gives the keys to this target while the overlay is open in review.
const STK_KEYS={
  active:()=>!!(stkDlg&&stkDlg.el.open&&stkDlg.rv),
  root:()=>stkDlg?stkDlg.el:null,
  prev:()=>stkStep(-1),next:()=>stkStep(1),
  pass:()=>stkArmPass(),fail:()=>stkFailOpen(),
  undo:()=>{htUndo();stkDlgDraw()},   // #102's U: the newest pending Pass
  confirming:()=>!!(stkArm&&stkDlg&&stkDlg.rv),
  confirm:()=>{stkConfirmYes()},cancel:()=>stkConfirmNo(),
};
// #102's queue gives a stack's stop to this function (reviewStackStop): the overlay on the first
// child in Human test not yet passed, or the stack page once they all are. Stepping back into the
// stack from the stop after it lands on its last stop, the stack page. False if it can't show it,
// and #102 opens the lead's panel instead.
function stkStackStop(lead){
  if(!lead||!isStackLead(lead))return false;
  const from=stkLeaving||(typeof panel!=="undefined"&&panel?panel.id:null),q=reviewQueue();
  const back=!!from&&q.indexOf(from)===q.indexOf(lead.id)+1;
  const k=back?null:stackKids(lead).find(x=>!isArch(x)&&colOf(x)===STK_HT&&x.passed!==true);
  stkKidOpen(lead.id,k?k.id:lead.id);
  return!!(stkDlg&&stkDlg.lead===lead.id&&stkDlg.rv);
}
// review.js loads after this file: hand it the stack stop and the key target once the page is in.
{const hook=()=>{if(typeof reviewStackStop!=="undefined")reviewStackStop=stkStackStop;if(typeof htKeys!=="undefined")htKeys.add(STK_KEYS)};
  if(document.readyState==="loading")document.addEventListener("DOMContentLoaded",hook);else setTimeout(hook,0)}
// The way in (the router): a click on a stack card in Human test, or Enter or Space on it, opens
// the overlay through #102's queue (its first unpassed child, or the stack page). Links, buttons
// and the child rows keep their own clicks; Open session still opens the lead's panel.
function stkReviewEntry(b,c){
  if(colOf(c)!==STK_HT||typeof reviewStartAt!=="function")return;
  const go=e=>{if(!stkRvOn(c))return;e.preventDefault();e.stopImmediatePropagation();
    if(panel&&panel.id===c.id){closePanel(true);render()}   // the lead's own panel open: its stack's review replaces it
    if(!reviewStartAt(c.id))openPanel(c.id)};
  b.addEventListener("click",e=>{if(e.target.closest("a,button,textarea,input,select,[data-noopen]"))return;go(e)},true);
  b.addEventListener("keydown",e=>{if((e.key==="Enter"||e.key===" ")&&e.target===b)go(e)},true);
}

const STK_VERB={stack_detach:"detach it",stack_join:"add it to the stack",set_passed:"mark it passed",stack_problem:"send the problem",stack_answer:"send the answer",stack_remove:"take it out of the stack"};
async function stkDo(cmd,args,done){
  try{await invoke(cmd,args);if(done)status(done);return true}
  catch(e){status(`Couldn't ${STK_VERB[cmd]||cmd}: ${errText(e)}`,true);return false}
}
function stkClick(e,lead){
  const t=e.target.closest("[data-act]");
  if(!t||!e.currentTarget.contains(t))return;
  if(t.tagName==="A")return;   // the PR: the board's link handler opens it
  e.stopPropagation();e.preventDefault();
  const n=cardNum(lead),kid=t.dataset.kid?cards.find(x=>x.id===t.dataset.kid):null,kn=kid?cardNum(kid):"";
  switch(t.dataset.act){
    case"toggle":stkOpen[n]=!stkOpen[n];if(!stkOpen[n])delete stkOpen[n];stkSave();render();break;
    case"session":stkDlgClose();openPanel(lead.id);break;
    case"login":{const l=loginsOf(lead)[+t.dataset.i];if(!l)return;
      invoke("open_login",{url:l.url,browser:l.browser||""}).then(()=>status(`Opened the sign-in for ${l.label||"the login"}`),err=>status("Couldn't open it: "+errText(err),true));break}
    case"kid":stkKidOpen(lead.id,t.dataset.kid);break;
    case"card":if(kid){stkDlgClose();openPanel(kid.id)}break;
    case"passed":if(kid){const on=!kid.passed;   // Passed closes the overlay; Undo passed leaves it open
      stkDo("set_passed",{id:kid.id,on},on?`#${kn} passed`:`#${kn} is no longer marked passed`).then(ok=>{if(ok&&on)stkDlgClose()})}break;
    case"problem":stkForm[t.dataset.kid]={kind:"problem",text:""};render();stkFocus(t.dataset.kid);break;
    case"cancel":delete stkForm[t.dataset.kid];render();stkFocusBar();break;
    case"answer":{const ch=kid&&Array.isArray(kid.choices)?kid.choices.filter(x=>typeof x==="string"&&x.trim())[+t.dataset.i]:null;if(!ch)return;
      t.disabled=true;stkDo("stack_answer",{id:kid.id,answer:ch},`Answered #${kn}: ${ch}`).then(ok=>{if(ok)stkDlgClose();else t.disabled=false});break}
    case"send":{if(!kid)return;const box=t.closest(".stk-form").querySelector("textarea"),text=box.value.trim();
      if(!text){status(t.dataset.kind==="problem"?"Say what went wrong first.":"Type an answer first.",true);box.focus();return}
      if(SECRET.test(text)){status("Not sent: it looks like it holds a token, key or password.",true);return}
      if(t.dataset.kind==="problem"&&stkDlg&&stkDlg.rv){stkFailSend(lead,kid,text,t);break}   // review's Fail (card #116)
      t.disabled=true;
      const p=t.dataset.kind==="problem"?stkDo("stack_problem",{id:kid.id,note:text},`Sent the problem with #${kn} to the stack's session; it's back in progress`)
        :stkDo("stack_answer",{id:kid.id,answer:text},`Sent your answer on #${kn}`);
      p.then(ok=>{if(ok){delete stkForm[kid.id];stkDlgClose();render()}else t.disabled=false});break}
    case"detach":if(!kid)return;
      stkConfirm({title:`Detach #${kn} from stack #${n}?`,from:kid,to:lead,arrow:"out of",
        lines:[["stop",`Stack #${n}'s session stops building it, and logs on #${kn} what it has already done.`],
               ["go",`#${kn} becomes its own card in Planning, and its own session starts there.`]],ok:"Detach"})
        .then(yes=>{if(yes){stkDlgClose();stkDo("stack_detach",{lead:n,child:kn},`#${kn} is its own card now, in Planning`)}else stkFocusBar()});break;
    case"remove":if(!kid)return;
      if(!confirm(`Take #${kn} "${kid.title||""}" out of stack #${n}? It goes back on the board as its own card, in ${colOf(kid)}.`))return;
      stkDo("stack_remove",{lead:n,child:kn},`#${kn} is out of the stack, back in ${colOf(kid)}`);break;
    // The review bar (card #116).
    case"rv-prev":stkStep(-1);break;
    case"rv-next":stkStep(1);break;
    case"rv-pass":stkArmPass();break;
    case"rv-yes":stkConfirmYes();break;
    case"rv-no":stkConfirmNo();break;
    case"rv-fail":stkFailOpen();break;
    case"rv-undo":if(kid)htUndo(kid.id);stkDlgDraw();break;
    case"rv-unpass":if(kid)stkDo("set_passed",{id:kid.id,on:false},`#${kn} is no longer marked passed`);break;
  }
}
function stkFocus(id){const f=document.querySelector(`${STK_TA}[data-f="${CSS.escape(id)}"]`);if(f)f.focus()}

// The card panel: a lead lists its children; a child names its stack.
function stackDetail(c){
  const kids=stackKids(c);
  if(kids.length)return`<div class="shipbox stk-det"><h3>Stack</h3><ul>${kids.map(k=>{const s=kidState(k);
    return`<li><span class="stk-dot s-${s.k}"></span><button class="stk-link" type="button" data-open-card="${esc(k.id)}"><span class="num">${esc(cardNum(k)||"")}</span>${safe(k.title||"Untitled card")}</button><span class="meta">${esc(colOf(k))}${k.passed&&colOf(k)==="Human test"?" · passed":s.k==="need"&&colOf(k)!=="Blocked"?" · needs you":""}</span></li>`}).join("")}</ul></div>`;
  const lead=stkLead(c);
  if(lead&&lead.id!==c.id)return`<div class="shipbox stk-det"><div>${STK_ICON}In stack <button class="stk-link" type="button" data-open-card="${esc(lead.id)}"><span class="num">${esc(cardNum(lead))}</span>${safe(lead.title||"Untitled stack")}</button></div></div>`;
  return"";
}
document.addEventListener("click",e=>{const b=e.target.closest("[data-open-card]");if(!b)return;e.preventDefault();openPanel(b.dataset.openCard)});

// render() rebuilds the board on every change: redraw the overlay from the latest cards, and put
// the cursor back in a box Hugh was typing in.
{
  const draw=render;
  render=function(){
    const ae=document.activeElement,f=ae&&ae.matches&&ae.matches(STK_TA)?{id:ae.dataset.f,s:ae.selectionStart,e:ae.selectionEnd}:null;
    if(f&&stkForm[f.id])stkForm[f.id].text=ae.value;
    draw.apply(this,arguments);
    stkDlgDraw();
    if(f){const n=document.querySelector(`${STK_TA}[data-f="${CSS.escape(f.id)}"]`);if(n&&n!==document.activeElement){n.focus();try{n.setSelectionRange(f.s,f.e)}catch(e){}}}
  };
}
render();

// Dropping a card on a card or a stack (its middle half; its top and bottom edges still reorder)
// asks to put it in that stack, making the card a stack if it isn't one (stack_join). The stack's
// session is told to plan and build it, or to merge its branch if it is built already.
// Card #89: a card dropped on a card in Ship it ships with it as one merge, if that ship hasn't
// started; if it has, the card ships straight after it instead. A card in Ship it can leave it
// for a stack, and a built stack can be dropped into another. The app decides what a drop does
// (stack_join_preview runs stack_join's own checks), so the dialog always says what will happen.
const STK_BUILT=c=>STK_READY.includes(colOf(c))||!!c.pr;
const stkOwn=c=>Array.isArray(c.stack)?c.stack.map(Number).filter(n=>n>0):[];   // a lead's cards, as the store has them
const stkCards=n=>`card${n===1?"":"s"}`;
function stackDropTarget(b,c){
  if(!canWrite||isArch(c))return;
  const mid=e=>{const r=b.getBoundingClientRect();return e.clientY>r.top+r.height*0.25&&e.clientY<r.bottom-r.height*0.25};
  // Every card dropped in the middle is the stack's to answer: one the app refuses (a stack still
  // being built, say) gets its reason, instead of falling through to a move into this column.
  const ok=()=>{const d=dragId&&cards.find(x=>x.id===dragId);return d&&d.id!==c.id&&!isArch(d)?d:null};
  const off=()=>b.classList.remove("stk-drop");
  b.addEventListener("dragover",e=>{if(!ok()||!mid(e)){off();return}
    e.preventDefault();e.stopPropagation();b.classList.add("stk-drop");dropLine.remove();
    document.querySelectorAll(".col.over").forEach(x=>x.classList.remove("over"));e.dataTransfer.dropEffect="move"});
  b.addEventListener("dragleave",e=>{if(!e.relatedTarget||!b.contains(e.relatedTarget))off()});
  b.addEventListener("drop",e=>{const d=ok();if(!d||!mid(e)){off();return}
    e.preventDefault();e.stopPropagation();off();stkDrop(d,c)});
}
// Asks the app what the drop does, then shows that dialog, or why it can't.
async function stkDrop(d,c){
  const ln=cardNum(c),dn=cardNum(d);
  let plan;
  try{plan=await invoke("stack_join_preview",{lead:ln,child:dn})}
  catch(e){status(`Couldn't check that drop: ${errText(e)}`,true);return}
  if(plan.kind==="refuse"){status(plan.why,true);return}
  if(plan.kind==="already"){status(`#${dn} is already waiting in Ship it: it ships after #${ln}, whose ship has started (${plan.why}).`);return}
  const ask=plan.kind==="after"?stkAfterAsk(d,c,plan.why):stkJoinAsk(d,c);
  if(await stkConfirm(ask))stkJoinDo(d,c);
}
// The dialog for a drop on a card whose ship has started: d ships straight after it.
function stkAfterAsk(d,c,why){
  const ln=cardNum(c),dn=cardNum(d);
  return{title:`Ship #${dn} straight after #${ln}?`,from:d,to:c,arrow:"after",ok:"Ship it next",
    lines:[["stop",`#${ln}'s ship has started (${why}), so #${dn} can't join that push.`],
           ["ship",`#${dn} goes into Ship it on its own and waits its turn in the ship queue, behind #${ln}.`]]};
}
// The dialog for a join: what the stack's session does, and what happens to each ship.
function stkJoinAsk(d,c){
  const ln=cardNum(c),dn=cardNum(d),lead=isStackLead(c),kids=stkOwn(d);
  const intoShip=colOf(c)==="Ship it",fromShip=colOf(d)==="Ship it",built=STK_BUILT(d);
  const same=d.branch&&c.branch&&String(d.branch).trim()===String(c.branch).trim();
  const what=kids.length?`#${dn}'s stack`:`#${dn}`;
  const lines=[];
  if(!built)lines.push(["plan",`#${dn} isn't built yet, so the stack's session plans it, then builds it on the stack's branch.`]);
  else if(same)lines.push(["merge",`${what} is on the same branch as #${ln} (${d.branch}), so there's nothing to merge.`]);
  else lines.push(["merge",`${kids.length?`#${dn}'s branch holds its stack's work, so `:`#${dn} is already built, so `}the stack's session merges it into the stack's branch and closes #${dn}'s PR in favour of the stack's.`]);
  if(kids.length)lines.push(["go",`${kids.length===1?"Its card":"Its cards"} ${kids.map(k=>"#"+k).join(", ")} ${kids.length===1?"comes":"come"} with it, in order. #${dn} no longer leads a stack.`]);
  if(fromShip)lines.push(["stop",`${what}'s own ship request is cancelled: it ships with ${intoShip?`#${ln}`:"the stack"} instead.`]);
  if(intoShip){
    const waits=shipState(c)==="queued";
    lines.push(["ship",`They ship as one merge ${waits?`when #${ln}'s turn comes`:`when #${ln} ships`}: one push to main, one deploy.`]);
    lines.push(["test",`${kids.length?`#${dn} and its cards are`:`#${dn} is`} marked passed and ${kids.length?"wait":"waits"} in the stack until it ships: dropping it here is your approval.`]);
  }else if(fromShip)lines.push(["test",`${kids.length?`#${dn} and its cards are`:`#${dn} is`} marked passed and ${kids.length?"wait":"waits"} in the stack in Human test. ${kids.length?"They ship":"It ships"} when you move #${ln} to Ship it.`]);
  else if(built)lines.push(["test",`${kids.length?"They stay":"It stays"} in Human test with ${kids.length?"their":"its"} own steps, and you test ${kids.length?"them":"it"} with the rest.`]);
  else lines.push(["test",`It shows up in the stack's Human test with its own steps once it works.`]);
  if(d.sessionId||d.claudeSessionId)lines.push(["stop",`#${dn}'s own session is told the stack's session owns it now.`]);
  const title=intoShip?`Ship ${kids.length?`stack #${dn}`:`#${dn}`} with #${ln} as one?`
    :kids.length?`Put stack #${dn} (${kids.length+1} ${stkCards(kids.length+1)}) into ${lead?`stack #${ln}`:`#${ln}`}?`
    :lead?`Add #${dn} to stack #${ln}?`:`Make #${ln} a stack with #${dn} in it?`;
  const ok=intoShip?"Ship as one":kids.length?"Put them together":lead?"Add to stack":"Make a stack";
  return{title,from:d,to:c,arrow:"into",lines,ok};
}
// stack_join, and a status line that says what the app actually did.
async function stkJoinDo(d,c){
  const ln=cardNum(c),dn=cardNum(d);
  let r;
  try{r=await invoke("stack_join",{lead:ln,child:dn})}
  catch(e){status(`Couldn't add it to the stack: ${errText(e)}`,true);return}
  if(r&&r.shipAfter){
    status(r.already?`#${dn} is already in Ship it: it ships after #${ln}.`
      :r.shipWaitFor?`#${dn} is in Ship it: it waits for #${r.shipWaitFor} to finish shipping, then ships.`
      :`#${dn} is in Ship it: its session is told to ship it.`);
    return;
  }
  const l=r&&r.lead,inShip=l&&l.column==="Ship it";
  status(inShip?`#${dn} is in stack #${ln}: they ship as one. The stack's session has been told.`:`#${dn} is in stack #${ln}; its session has been told`);
}

// The board's own confirm for stack actions (not the browser's): the two cards, what will happen,
// Cancel and the action. Resolves true on the action, false on Cancel, Esc or a click outside.
const STK_ICONS={plan:"✎",test:"✓",merge:"⑂",stop:"■",go:"→",ship:"↑"};
function stkConfirm({title,from,to,arrow,lines,ok}){
  return new Promise(done=>{
    const d=document.createElement("dialog");d.className="stk-dlg";d.setAttribute("aria-label",title);
    const chip=(c,lab)=>`<div class="stk-chip${isStackLead(c)?" lead":""}"><span class="ch-lab">${lab}</span><span class="ch-t">${isStackLead(c)?STK_ICON:""}${numBadge(c)}${safe(c.title||"Untitled card")}</span><span class="ch-col">${esc(colOf(c))}${isStackLead(c)?` · ${stackKids(c).length} ${stkCards(stackKids(c).length)}`:""}</span></div>`;
    d.innerHTML=`<form method="dialog"><h2>${esc(title)}</h2>
      <div class="stk-pair">${chip(from,"Card")}<span class="stk-arrow">${esc(arrow)}</span>${chip(to,isStackLead(to)?"Stack":"Card")}</div>
      <ul class="stk-what">${lines.map(([k,t])=>`<li><span class="i i-${k}">${STK_ICONS[k]||"•"}</span><span>${esc(t)}</span></li>`).join("")}</ul>
      <div class="stk-btns"><button class="btn ghost" value="no" type="submit">Cancel</button><button class="btn" value="yes" type="submit" autofocus>${esc(ok)}</button></div></form>`;
    document.body.appendChild(d);
    let answer=false;
    d.querySelector("form").addEventListener("submit",e=>{answer=(e.submitter&&e.submitter.value)==="yes"});
    d.addEventListener("click",e=>{if(e.target===d)d.close()});
    d.addEventListener("close",()=>{d.remove();done(answer)});
    try{d.showModal()}catch(e){d.setAttribute("open","")}
    const y=d.querySelector('[value="yes"]');if(y)y.focus();
  });
}
