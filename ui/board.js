const COLUMNS=["Backlog","Blocked","Planning","In progress","Human test","Ship it","Deploy complete","Done"];
// The columns drawn on the board. Done isn't one (card #83): a card that reaches it is archived
// at once, and shows in the Archive column instead.
const BOARD_COLS=COLUMNS.filter(c=>c!=="Done");
const ARCH_RECENT=20; // the Archive shows its newest cards; the rest behind a button
const SECRET=/ct=|customtoken|idtoken|eyJ[A-Za-z0-9_-]{6,}|sk_(live|test)|password/i;
const WEEK=7*24*3600*1000;
let loaded=false,cards=[],canWrite=false,selected=null,showOld=false,q="";
const isArch=c=>c.archived===true;
try{showOld=localStorage.getItem("wb.showOldArchive")==="1"}catch(e){}
try{selected=location.hash.slice(1)||null}catch(e){}
const $=s=>document.querySelector(s);
// A message stays STATUS_KEEP ms: the reload its own change sets off doesn't wipe it with the card count.
const STATUS_KEEP=6000;let statusAt=0;
const status=(t,err,count)=>{if(count&&Date.now()-statusAt<STATUS_KEEP)return;const el=$("#status");el.textContent=t;el.classList.toggle("err",!!err);if(!count)statusAt=Date.now()};
const esc=s=>String(s??"").replace(/[&<>"']/g,c=>({"&":"&amp;","<":"&lt;",">":"&gt;",'"':"&quot;","'":"&#39;"}[c]));
const safe=v=>{const s=String(v??"");return SECRET.test(s)?'<span class="hidden-secret">hidden: looks like a secret</span>':esc(s)};
const safeUrl=u=>{const s=String(u??"");if(!s)return"";if(SECRET.test(s))return'<span class="hidden-secret">hidden: looks like a secret</span>';
  if(!/^https?:\/\//i.test(s))return esc(s);return`<a href="${esc(s)}" target="_blank" rel="noopener">${esc(s.replace(/^https?:\/\//,""))}</a>`};
const ts=c=>Date.parse(c.updatedAt||"")||0;
const ago=t=>{if(!t)return"";const m=Math.round((Date.now()-t)/60000);if(m<1)return"just now";if(m<60)return m+"m ago";const h=Math.round(m/60);if(h<48)return h+"h ago";return Math.round(h/24)+"d ago"};
const colOf=c=>COLUMNS.includes(c.column)?c.column:"Backlog";
// Where Restore puts an archived card (the store's rule): a Done card goes back to where it came from.
const restoreCol=c=>colOf(c)!=="Done"?colOf(c):(BOARD_COLS.includes(c.archivedFrom)&&c.archivedFrom!=="Ship it"?c.archivedFrom:"Deploy complete");
const archAt=c=>Date.parse(c.archivedAt||"")||ts(c);
// Hugh's manual order: a card's `order` counts only in the column it was set in (`orderCol`), so a session's move drops it.
const hasOrd=c=>typeof c.order==="number"&&isFinite(c.order)&&c.orderCol===colOf(c);
const byOrder=(a,b)=>{const ao=hasOrd(a),bo=hasOrd(b);if(ao&&bo)return a.order-b.order||ts(b)-ts(a);if(ao)return -1;if(bo)return 1;return ts(b)-ts(a)};
// The router's own card (card #120 for #119): `system: "router"` or number 0. It is never drawn as a
// card and never counted (columns, pills, the review queue). Global, so other files use it too.
const isSystemCard=c=>!!c&&(c.fields&&c.fields.system==="router"||c.system==="router"||c.number===0);
window.isSystemCard=isSystemCard;
const colCards=col=>cards.filter(c=>!isSystemCard(c)&&!isArch(c)&&colOf(c)===col&&!(typeof stackHidden==="function"&&stackHidden(c))).sort(byOrder); // a stack's children aren't in the column (stacks.js)
const cardNum=c=>Number.isInteger(c.number)&&c.number>0?c.number:null;
const numBadge=c=>cardNum(c)?`<span class="num" title="Card number: /kanban ${cardNum(c)}">${cardNum(c)}</span>`:"";
const issueNum=c=>c.issue&&typeof c.issue==="object"?c.issue.number:c.issue;
const issueUrl=c=>c.issue&&typeof c.issue==="object"?c.issue.url:"";
const httpOk=u=>/^https?:\/\//i.test(String(u||""))&&!SECRET.test(String(u));
const SHIP_LABEL={queued:"Waiting to ship",requested:"Ship requested",shipping:"Shipping",shipped:"Shipped",failed:"Ship failed","needs-hugh":"Needs Hugh"};
const shipState=c=>c.ship&&typeof c.ship==="object"?c.ship.state:"";
const BACKUP_NOTE="No Claude session is watching the board; the backup check will pick it up";
const shipChip=c=>{const st=shipState(c);if(!SHIP_LABEL[st])return"";const det=c.ship&&c.ship.detail?String(c.ship.detail):"";
  const backup=st==="requested"&&det===BACKUP_NOTE;
  return`<span class="tag chip ${esc(st)}"${det&&!SECRET.test(det)?` title="${esc(det)}"`:""}>${SHIP_LABEL[st]}${backup?" · backup check":""}</span>`};
const PLAN_LABEL={requested:"Plan requested",claimed:"Starting session",started:"Session started"};
const planState=c=>c.plan&&typeof c.plan==="object"?c.plan.state:"";
const PLAN_NOTE="No board session is watching; start one or open the card's session";
const planChip=c=>{const st=planState(c);if(!PLAN_LABEL[st])return"";if(st==="started"&&colOf(c)!=="Planning")return"";const det=c.plan&&c.plan.detail?String(c.plan.detail):"";
  const none=st==="requested"&&det===PLAN_NOTE;
  return`<span class="tag chip plan-${esc(st)}"${det&&!SECRET.test(det)?` title="${esc(det)}"`:""}>${PLAN_LABEL[st]}${none?" · nobody watching":""}</span>`};
// Take it to (card #76): how far a card goes on by itself (`autoTo`), and its card-tester subagent's verdict (`test`).
const AUTO_TO={"Human test":"→ Human test","Deploy complete":"→ Deploy"};
const autoOf=c=>c&&typeof c.autoTo==="string"&&Object.hasOwn(AUTO_TO,c.autoTo)?c.autoTo:"";
const autoChip=c=>{const a=autoOf(c);return a?`<span class="tag chip auto${a==="Deploy complete"?" auto-deploy":""}" title="Goes on by itself to ${esc(a)} (Take it to)">${esc(AUTO_TO[a])}</span>`:""};
// The app's subagent verdicts on a card (cards #76, #96, #100), one entry each: the card field, the
// panel row, the chip for each state, and the columns where the chip shows.
const CHECKS=[
  {key:"test",row:"Subagent test",chip:{passed:"Tested",failed:"Test failed",unclear:"Test unclear"},cols:["In progress","Human test"]},
  {key:"review",row:"Subagent review",chip:{passed:"Reviewed",failed:"Review failed",unclear:"Review unclear"},cols:["In progress","Human test"]},
  // The plan critique (card #97): on Planning and Blocked cards, not once the build has started.
  {key:"critique",row:"Plan critique",chip:{passed:"Plan critiqued",failed:"Critique failed",unclear:"Critique unclear"},cols:["Planning","Blocked"],hide:t=>t.started===true},
  // The card-reproducer (card #112): its own states, and its chip on a Blocked card too (NOT REPRODUCED sends it there).
  {key:"repro",row:"Reproducer",chip:{needed:"Bug to reproduce",reproduced:"Bug reproduced",fixed:"Fix proved","not-reproduced":"Not reproduced",unclear:"Repro unclear"},cols:["In progress","Human test","Blocked"],
   labels:{needed:"A bug to reproduce",reproduced:"Bug reproduced",fixed:"Fix proved","not-reproduced":"Not reproduced",unclear:"Unclear"}},
  {key:"merge",row:"Merger",chip:{passed:"Merged",failed:"Merge failed",unclear:"Merge unclear"},cols:["Ship it","Deploy complete"]},
];
const CHECK_LABEL={needed:"Needed",passed:"Passed",failed:"Failed",unclear:"Unclear"};
const checkOf=(c,k)=>c[k]&&typeof c[k]==="object"&&typeof c[k].state==="string"?c[k]:null;
const checkChip=(c,ch)=>{const t=checkOf(c,ch.key);if(!t||!Object.hasOwn(ch.chip,t.state)||!ch.cols.includes(colOf(c))||(ch.hide&&ch.hide(t)))return"";const det=t.detail?String(t.detail):"";
  return`<span class="tag chip test-${esc(t.state)}" data-check="${esc(ch.key)}"${det&&!SECRET.test(det)?` title="${esc(det)}"`:""}>${esc(ch.chip[t.state])}</span>`};
const testChip=c=>CHECKS.map(ch=>checkChip(c,ch)).join("");
// The panel's rows: the state, when, how many FAILs since the reset, and what failed.
// Card #151: a subagent Hugh has off on the card says so in its row, and why.
const subOff=(c,k)=>typeof subagentOffText==="function"&&window.SUBAGENT_OF_CHECK?subagentOffText(c,window.SUBAGENT_OF_CHECK[k]):"";
const checkRows=c=>CHECKS.map(ch=>{const t=checkOf(c,ch.key),at=t?Date.parse(t.at||""):0,r=t&&Number.isInteger(t.round)?t.round:0,off=subOff(c,ch.key);
  const v=t?`${esc(Object.hasOwn(ch.labels||CHECK_LABEL,t.state)?(ch.labels||CHECK_LABEL)[t.state]:t.state)}${at?` · <span title="${esc(t.at)}">${esc(ago(at))}</span>`:""}${r?` · ${r} FAIL${r===1?"":"s"}`:""}${t.detail?`<br>${safe(t.detail)}`:""}`:"—";
  return`<dt>${esc(ch.row)}</dt><dd>${v}${off?` <span class="meta sub-off">· ${esc(off)}</span>`:""}</dd>`}).join("");
// The card-router subagent's route (card #98): a size chip on Planning and In progress cards,
// and the panel's Route row, with Route again while the card is in Planning.
const ROUTE_CHIP={small:"Small",normal:"Normal",split:"Split"};
const routeOf=c=>c&&c.route&&typeof c.route==="object"&&Object.hasOwn(ROUTE_CHIP,c.route.size)?c.route:null;
const routeChip=c=>{const r=routeOf(c);if(!r||!["Planning","In progress"].includes(colOf(c)))return"";const why=r.reason&&!SECRET.test(String(r.reason))?`: ${r.reason}`:"";
  return`<span class="tag chip route route-${esc(r.size)}" data-route="${esc(r.size)}" title="${esc(`Routed ${r.size} by the card-router subagent${why}`)}">${esc(ROUTE_CHIP[r.size])}</span>`};
const routeRow=c=>{const r=routeOf(c),at=r?Date.parse(r.at||""):0;
  const again=colOf(c)==="Planning"&&!c.stackOf?`<button class="ibtn" id="route-again" type="button" title="Clear the route and ask the card's session to run the card-router again">Route again</button>`:"";
  const kids=r&&Array.isArray(r.children)?r.children.filter(x=>typeof x==="string"):[];
  const v=r?`${esc(ROUTE_CHIP[r.size])}${r.model||r.effort?` · ${esc([r.model,r.effort].filter(Boolean).join(" "))}`:""}${r.check?` · check ~${esc(r.check)}`:""}${at?` · <span title="${esc(r.at)}">${esc(ago(at))}</span>`:""}${r.reason?`<br>${safe(r.reason)}`:""}${kids.map(x=>`<br>- ${safe(x)}`).join("")}${r.wentOnAt?`<br><span class="meta">Went on to In progress by itself</span>`:""}`:"—";
  const ro=typeof subagentOffText==="function"?subagentOffText(c,"router"):"",off=ro?` <span class="meta sub-off">· card-router ${esc(ro)}</span>`:"";
  return`<dt>Route</dt><dd>${v}${off}${again?` ${again}`:""}</dd>`};
// The stuck-session monitor (card #99): its `stuck` field puts "May be stuck" on the card, with
// the reason as its title, and a panel row saying since when and what the app did.
const STUCK_KIND={repeating:"Repeating",quiet:"Quiet",question:"At a question",permission:"At a permission prompt",failed:"Failed","dead wait":"Dead wait",idle:"Idle, nothing running"};
const stuckOf=c=>c.stuck&&typeof c.stuck==="object"&&typeof c.stuck.reason==="string"?c.stuck:null;
const stuckChip=c=>{const s=stuckOf(c);if(!s||isArch(c)||colOf(c)==="Done")return"";
  return`<span class="tag chip stuck" data-stuck="${esc(s.kind||"")}"${SECRET.test(s.reason)?"":` title="${esc(s.reason)}"`}>May be stuck</span>`};
const stuckRow=c=>{const s=stuckOf(c);if(!s||isArch(c)||colOf(c)==="Done")return"";const at=Date.parse(s.since||"");
  const did=[s.nudged?(s.typed===false?"couldn't get its message in":s.kind==="dead wait"||s.kind==="idle"?"sent it a message":"nudged it"):"",s.notified?"told you":""].filter(Boolean).join(", ");
  return`<dt>May be stuck</dt><dd class="stuck-row">${esc(STUCK_KIND[s.kind]||s.kind||"")}${at?` · since <span title="${esc(s.since)}">${esc(ago(at))}</span>`:""}${did?` · the app ${esc(did)}`:""}<br>${safe(s.reason)}</dd>`};
// The card-reproducer's proof (card #112): the bug reports, the steps and shot before the fix and
// after it, and the test that pins it. Shots load through image_read (panel.js showShots).
const REPRO_SRC={"found-a-problem":"Found a problem","human-test-fail":"Failed in Human test","report-a-bug":"Report a bug","card-tester":"card-tester FAIL","card-reviewer":"card-reviewer FAIL"};
function reproRun(name,r){
  if(!r||typeof r!=="object")return`<div class="rp-run"><b>${name}</b><span class="meta">not run yet</span></div>`;
  const at=Date.parse(r.at||"");
  const shot=typeof r.shot==="string"&&r.shot&&typeof shotImg==="function"&&!SECRET.test(r.shot)?shotImg(r.shot,r.at):"";
  return`<div class="rp-run"><b>${name}</b>${at?`<span class="meta" title="${esc(r.at)}">${esc(ago(at))}</span>`:""}${r.steps?`<pre class="rp-steps">${safe(r.steps)}</pre>`:`<span class="meta">no steps given</span>`}${shot||`<span class="meta">no screenshot</span>`}</div>`;
}
function reproBox(c){
  const r=checkOf(c,"repro");if(!r)return"";
  const ch=CHECKS.find(x=>x.key==="repro"),at=Date.parse(r.at||"");
  const reps=Array.isArray(r.reports)?r.reports.filter(x=>x&&typeof x==="object"):[];
  const earlier=Array.isArray(r.earlier)?r.earlier.filter(x=>x&&typeof x==="object"):[];
  return`<div class="shipbox repro"><h3>Reproducer</h3>
   <div><span class="tag chip test-${esc(r.state)}">${esc(ch.chip[r.state]||r.state)}</span>${at?` <span class="meta" title="${esc(r.at)}">${esc(ago(at))}</span>`:""}</div>
   ${r.detail?`<div>${safe(r.detail)}</div>`:""}
   ${reps.length?`<ul class="log">${reps.map(x=>`<li><b>${esc(REPRO_SRC[x.source]||x.source||"Bug report")}</b>: ${safe(x.note||"")}</li>`).join("")}</ul>`:""}
   <div class="rp-runs">${reproRun("Before the fix",r.before)}${reproRun("After the fix",r.after)}</div>
   ${r.test?`<div>Test: <code>${safe(r.test)}</code></div>`:""}
   ${earlier.length?`<details class="rp-earlier"><summary>Earlier rounds (${earlier.length})</summary>${earlier.slice().reverse().map(e=>`<div class="rp-round"><span class="tag chip test-${esc(e.state||"")}">${esc(ch.chip[e.state]||e.state||"")}</span>${Array.isArray(e.reports)&&e.reports.length?` <span class="meta">${e.reports.map(x=>safe(x&&x.note||"")).join(" · ")}</span>`:""}<div class="rp-runs">${reproRun("Before the fix",e.before)}${reproRun("After the fix",e.after)}</div>${e.test?`<div>Test: <code>${safe(e.test)}</code></div>`:""}</div>`).join("")}</details>`:""}
  </div>`;
}
const loginsOf=c=>Array.isArray(c.logins)?c.logins.filter(l=>l&&typeof l==="object"):[];
function moveExtras(c,col,now){
  const into=col==="Ship it"&&colOf(c)!=="Ship it";
  const outOfRequested=colOf(c)==="Ship it"&&col!=="Ship it"&&(shipState(c)==="requested"||shipState(c)==="queued");
  const retries=c.ship&&typeof c.ship==="object"&&c.ship.retries?c.ship.retries:0;
  const patch={},texts=[];
  if(into){patch.shipRequestedAt=now;patch.ship={state:"requested",requestedAt:now,retries};texts.push("Ship requested")}
  if(outOfRequested){patch.shipRequestedAt=null;patch.ship={state:"cancelled",cancelledAt:now,retries};texts.push("Ship request cancelled")}
  const intoPlan=col==="Planning"&&colOf(c)!=="Planning";
  const outOfPlan=colOf(c)==="Planning"&&col!=="Planning"&&planState(c)==="requested";
  if(intoPlan){patch.planRequestedAt=now;patch.plan={state:"requested",requestedAt:now};texts.push("Plan requested")}
  if(outOfPlan){patch.planRequestedAt=null;patch.plan={state:"cancelled",cancelledAt:now};texts.push("Plan request cancelled")}
  return{patch,text:texts.join("; "),ship:into,plan:intoPlan};
}

// Shared hooks (card #107), so a feature adds one line in its own file instead of editing the long
// lines here. Each hook is caught on its own: one that throws never stops the board drawing.
//  filters:  f(card, term) -> true or false when the term is its own ("is:waiting"), null when not
//  decorate: f(cardElement, card), on every card after stackDecorate and stackDropTarget
//  details:  f(card) -> HTML for the panel's details, after the Ship and Planning boxes
//  rendered: f(), once at the end of every render (also when no card was drawn)
//  final:    f(cardElement, card), after every decorate hook, so the card face (face.js) is laid out last
const boardHooks={filters:[],decorate:[],details:[],rendered:[],final:[]};
const hookTry=(f,...a)=>{try{return f(...a)}catch(e){console.error("board hook",e);return null}};
// "is:stuck" (card #120): the cards the stuck-session monitor flagged, as its "May be stuck" pill counts them.
const isStuckCard=c=>!!stuckOf(c)&&!isArch(c)&&colOf(c)!=="Done";
boardHooks.filters.push((c,t)=>t==="is:stuck"?isStuckCard(c):null);
// The filter box splits on spaces and every term must match. A "quoted part" keeps its spaces: label:"abc 9".
const Q_TERM=/[^\s"]*"[^"]*"?[^\s"]*|\S+/g;
// "label: bug" (a space after the colon) is one term, and so is an unquoted label with spaces that
// some card has ("label:abc 9"), the longest such label first. Worked out once per box text and board.
let qMemo={s:null,cards:null,t:[]};
function qTerms(s){s=String(s||"");if(qMemo.s===s&&qMemo.cards===cards)return qMemo.t;
  const ts=(s.match(Q_TERM)||[]).map(t=>t.replace(/"/g,"")).filter(Boolean),out=[];
  const known=new Set(typeof boardLabels==="function"?boardLabels().map(l=>"label:"+String(l).trim().toLowerCase()):[]);
  for(let i=0;i<ts.length;i++){let t=ts[i];
    if(/^label:$/i.test(t)&&i+1<ts.length)t+=ts[++i];
    if(/^label:/i.test(t))for(let j=Math.min(ts.length-1,i+5);j>i;j--){const c=[t,...ts.slice(i+1,j+1)].join(" ");if(known.has(c.toLowerCase())){t=c;i=j;break}}
    out.push(t)}
  qMemo={s,cards,t:out};return out}
// The box's text with `term` taken out if it is there (any case, quotes or not), else added at the end.
function qToggle(s,term){const key=t=>t.replace(/"/g,"").toLowerCase(),k=key(term);const ts=String(s||"").match(Q_TERM)||[];const kept=ts.filter(t=>key(t)!==k);return(kept.length<ts.length?kept:[...ts,term]).join(" ")}
function matches(c){if(!q)return true;return qTerms(q).every(t=>termMatches(c,t))}
function termMatches(c,t){for(const f of boardHooks.filters){const m=hookTry(f,c,t);if(m===true||m===false)return m}
  if(typeof labelMatch==="function"){const m=labelMatch(c,t);if(m!==null)return m}if(/^\d+$/.test(t))return String(cardNum(c))===t||String(issueNum(c))===t;const h=[c.title,c.repo,issueNum(c),c.branch,c.summary,c.sessionTitle,c.id,...(Array.isArray(c.labels)?c.labels:[])].join(" ").toLowerCase();return h.includes(t)}

function render(){
  const board=$("#board");board.innerHTML="";
  // The empty-board note only after a load that worked, so a failed load never looks like an empty board.
  $("#emptyNote").hidden=!(loaded&&cards.length===0);
  const now=Date.now();
  // A card in Done is archived by the store; one an older build left there unarchived shows in the Archive too.
  const arch=cards.filter(c=>!isSystemCard(c)&&(isArch(c)||colOf(c)==="Done"));
  for(const col of BOARD_COLS){
    // A stack's children leave the columns (stacks.js); a stack shows when it or a child matches.
    // A card the Router asked to archive waits in the Done pile instead (archive-ask.js, card #131).
    let list=cards.filter(c=>!isSystemCard(c)&&!isArch(c)&&colOf(c)===col&&!(typeof inDonePile==="function"&&inDonePile(c))&&!(typeof stackHidden==="function"&&stackHidden(c))&&(typeof stackMatches==="function"?stackMatches(c):matches(c))).sort(byOrder);
    if(typeof pinFirst==="function")list=pinFirst(list); // cards that need Hugh go first (needs.js)
    const el=document.createElement("section");
    el.className="col c-"+col.replace(/ /g,"-");el.dataset.col=col;
    el.innerHTML=`<div class="colhead"><span class="dot"></span>${esc(col)}<span class="n">${list.length}</span></div><div class="cards"></div>`;
    const wrap=el.querySelector(".cards");
    for(const c of list)wrap.appendChild(cardEl(c));
    if(canWrite){
      el.addEventListener("dragover",e=>{e.preventDefault();el.classList.add("over");
        const k=dropBefore(wrap,e.clientY);
        if(dropLine.parentNode!==wrap||dropLine.nextSibling!==k)wrap.insertBefore(dropLine,k)});
      el.addEventListener("dragleave",e=>{if(e.relatedTarget&&el.contains(e.relatedTarget))return;el.classList.remove("over");dropLine.remove()});
      el.addEventListener("drop",e=>{e.preventDefault();el.classList.remove("over");const k=dropBefore(wrap,e.clientY);dropLine.remove();
        const id=e.dataTransfer.getData("text/plain")||dragId;if(id)placeCard(id,col,k?k.dataset.id:null)});
    }
    board.appendChild(el);
  }
  board.appendChild(archiveCol(arch));
  if(typeof timesHeads==="function")timesHeads(); // each column header's median, in its tooltip (times.js, card #111)
  if(focusAfter){const k=document.querySelector(`.card[data-id="${CSS.escape(focusAfter)}"]`);if(k&&!(typeof panel!=="undefined"&&panel)){k.focus();focusAfter=null}}
  renderDetail();
  for(const f of boardHooks.rendered)hookTry(f); // other files' work after a render (boardHooks)
}

// The Archive column (card #83): a box to drop a card in at the top, then the archived cards,
// newest first. Dropping a card here finishes it: it moves to Done, which archives it and stops
// its session. Archived cards open as usual (Restore is in the card); they can't be dragged.
function archiveCol(arch){
  const el=document.createElement("section");el.className="col c-Archived";el.dataset.col="Archived";
  const list=arch.filter(matches).sort((a,b)=>archAt(b)-archAt(a));
  const shown=showOld?list:list.slice(0,ARCH_RECENT),more=list.length-shown.length;
  el.innerHTML=`<div class="colhead"><span class="dot"></span>Archive</div>
   ${canWrite?`<div class="archdrop" title="Drop a card here to archive it. It moves to Done, its session stops, and Restore in the card brings it back."><span class="ico" aria-hidden="true">&#x2913;</span>Drop here to archive</div>`:""}
   <div class="archhead"><span>Archived</span><span class="n">${list.length}</span></div><div class="cards"></div>`;
  const wrap=el.querySelector(".cards");for(const c of shown)wrap.appendChild(cardEl(c));
  if(!list.length){const n=document.createElement("div");n.className="archnone";n.textContent=q?"No archived card matches.":"Nothing archived yet.";wrap.appendChild(n)}
  if(list.length>ARCH_RECENT){const b=document.createElement("button");b.className="older";b.type="button";
    b.textContent=showOld?`Show only the newest ${ARCH_RECENT}`:`Show ${more} older`;
    b.onclick=()=>{showOld=!showOld;try{localStorage.setItem("wb.showOldArchive",showOld?"1":"0")}catch(e){}render()};wrap.appendChild(b)}
  const box=el.querySelector(".archdrop");
  if(box){
    // Only a card being dragged lights the box; anything else dropped here is ignored.
    box.addEventListener("dragover",e=>{if(!dragId)return;e.preventDefault();box.classList.add("over")});
    box.addEventListener("dragleave",()=>box.classList.remove("over"));
    box.addEventListener("drop",e=>{e.preventDefault();box.classList.remove("over");const id=dragId||e.dataTransfer.getData("text/plain");if(id)archiveDrop(id)});
  }
  return el;
}

let dragId=null,focusAfter=null;
const dropLine=document.createElement("div");dropLine.className="dropline";dropLine.setAttribute("aria-hidden","true");
// The card the dragged one will land above: the first whose midpoint is below the pointer (null = the end of the list).
function dropBefore(wrap,y){for(const k of wrap.querySelectorAll(".card")){if(k.dataset.id===dragId)continue;const r=k.getBoundingClientRect();if(y<r.top+r.height/2)return k}return null}

// Parts other files add to every card (card #105): addCardPart({at, line, panel, wire}), drawn in `at` order.
// line(c): HTML on the card face; panel(c): HTML in its panel; wire(c, d): handlers once the panel has drawn.
// Each runs in try/catch, so a broken part logs to the console and never blanks the board.
const CARD_PARTS=[];
function addCardPart(p){if(!p||typeof p!=="object")return;CARD_PARTS.push(p);CARD_PARTS.sort((a,b)=>(+a.at||0)-(+b.at||0))}
function cardPartsRun(k,args){return CARD_PARTS.map(p=>{try{const v=typeof p[k]==="function"?p[k](...args):"";return typeof v==="string"?v:""}
  catch(e){console.error(`Card part (at ${p.at}) ${k} failed:`,e);return""}})}
function cardLines(c){return cardPartsRun("line",[c]).join("")}
function panelParts(c){return cardPartsRun("panel",[c]).join("")}
function panelPartsWire(c,d){cardPartsRun("wire",[c,d])}

function cardEl(c){
  const b=document.createElement("div");b.className="card"+(c.id===selected?" sel":"")+(typeof needClass==="function"&&!isArch(c)?needClass(c.id):"");b.setAttribute("role","button");b.tabIndex=0;
  b.draggable=canWrite&&!isArch(c);b.dataset.id=c.id;
  if(b.draggable)b.title="Drag to move or reorder. With the card focused, Alt+Up or Alt+Down moves it one place.";
  const n=issueNum(c);
  b.innerHTML=`<span class="t">${typeof needIcon==="function"&&!isArch(c)?needIcon(c.id):""}${typeof stateDot==="function"?stateDot(c.id):""}${numBadge(c)}${safe(c.title||"Untitled card")}</span>${c.summary?`<span class="s">${safe(c.summary)}</span>`:""}
  ${cardLines(c)}
  <span class="tags">${typeof labelTags==="function"?labelTags(c):""}${typeof queuedBadge==="function"?queuedBadge(c.id):""}${shipChip(c)}${autoChip(c)}${typeof smallChip==="function"?smallChip(c):""}${stuckChip(c)}${testChip(c)}${routeChip(c)}${(typeof buildingChip==="function"&&buildingChip(c.id))||planChip(c)}${n?(httpOk(issueUrl(c))?`<a class="tag" data-meta="issue" href="${esc(issueUrl(c))}" target="_blank" rel="noopener">issue ${esc(n)}</a>`:`<span class="tag" data-meta="issue">issue ${esc(n)}</span>`):""}${c.repo?`<span class="tag" data-meta="repo">${esc(c.repo)}</span>`:""}${c.pr?(httpOk(c.pr)?`<a class="tag" data-meta="pr" href="${esc(c.pr)}" target="_blank" rel="noopener">PR</a>`:`<span class="tag" data-meta="pr">PR</span>`):""}${loginsOf(c).length?`<span class="tag" data-meta="logins">${loginsOf(c).length} login${loginsOf(c).length===1?"":"s"}</span>`:""}${c.devStack?`<span class="tag" data-meta="stack">stack</span>`:""}${c.account==="account2"?`<span class="tag a2" data-meta="acct">acct 2</span>`:""}${colOf(c)==="Blocked"&&c.blockedReason?`<span class="tag blocked">blocked</span>`:""}<span data-meta="ago"${isArch(c)?' title="When it was archived"':""}>${esc(ago(isArch(c)?archAt(c):ts(c)))}</span></span>`;
  if(typeof stackDecorate==="function")stackDecorate(b,c); // a stack's lead draws as the stack card (stacks.js)
  if(typeof stackDropTarget==="function")stackDropTarget(b,c); // drop a card on it to stack them (stacks.js)
  for(const f of boardHooks.decorate)hookTry(f,b,c); // other files' additions to the card (boardHooks)
  for(const f of boardHooks.final)hookTry(f,b,c); // then the face's layout (face.js), always last
  b.onclick=e=>{if(e.target.closest("a,[data-noopen]"))return;openPanel(c.id)};
  b.addEventListener("keydown",e=>{if((e.key==="Enter"||e.key===" ")&&e.target===b){e.preventDefault();openPanel(c.id);return}
    if(e.altKey&&(e.key==="ArrowUp"||e.key==="ArrowDown")&&e.target===b&&canWrite&&!isArch(c)){e.preventDefault();
      const sibs=[...b.parentNode.querySelectorAll(".card")].map(k=>k.dataset.id),i=sibs.indexOf(c.id);
      if(e.key==="ArrowUp"){if(i<=0)return;focusAfter=c.id;placeCard(c.id,colOf(c),sibs[i-1])}
      else{if(i<0||i>=sibs.length-1)return;focusAfter=c.id;placeCard(c.id,colOf(c),sibs[i+2]||null)}}});
  b.addEventListener("dragstart",e=>{dragId=c.id;e.dataTransfer.setData("text/plain",c.id)});
  b.addEventListener("dragend",()=>{dragId=null;dropLine.remove();document.querySelectorAll(".col.over,.archdrop.over").forEach(x=>x.classList.remove("over"))});
  return b;
}

// The card's details: the left column of its panel (panel.js), rebuilt on every render.
let draft=null; // unsaved summary/next step/column, kept across live updates while the panel is open
let autoPend=null; // {id, v}: a "Goes on to" choice being saved, so a redraw meanwhile doesn't put the old one back
const AUTO_OPTS=[["","Nowhere: waits for you at each step"],["Human test","Human test"],["Deploy complete","Deploy complete"]];

function renderDetail(){
  if(typeof panel==="undefined"||!panel)return;
  const d=panel.el.querySelector("#p-det");if(!d)return;
  const c=cards.find(x=>x.id===panel.id);
  if(!c){d.innerHTML=`<div class="quiet">This card isn't on the board any more.</div>`;return}
  // Don't rebuild under Hugh's cursor: a live update while he is typing waits for the next render.
  const ae=document.activeElement;
  if(draft&&draft.id===c.id&&ae&&d.contains(ae)&&/^(TEXTAREA|SELECT)$/.test(ae.tagName))return;
  // Keep what Hugh is typing when a live update re-renders the details.
  const prevSm=d.querySelector("#f-summary"),prevNx=d.querySelector("#f-next"),prevCol=d.querySelector("#f-col");
  if(prevSm&&draft&&draft.id===c.id){draft.summary=prevSm.value;draft.nextStep=prevNx.value;draft.col=prevCol.value}
  const links=Array.isArray(c.links)?c.links:[];
  const log=Array.isArray(c.log)?c.log.slice(0,20):[];
  const n=issueNum(c);
  const logins=loginsOf(c);
  const st=shipState(c);const showShip=st&&st!=="cancelled";
  const sLink=String(c.sessionLink||"").replace(/^claude2:\/\//,"claude://");const sessionBtn=/^claude:\/\/claude\.ai\/[A-Za-z0-9_\/.-]+$/.test(sLink)?`<a class="btn ghost" href="${esc(sLink)}">Open session</a>`:"";
  const shipBox=showShip?`<div class="shipbox"><h3>Ship</h3><div>${shipChip(c)||esc(st)}</div>${c.ship.detail?`<div>${safe(c.ship.detail)}</div>`:""}${c.ship.retries?`<span class="meta">Retries: ${esc(c.ship.retries)} of 3</span>`:""}${c.dryRun?`<span class="meta">Dry run: nothing really ships</span>`:""}${c.ship.threadId?`<span class="meta">Comment thread on this card</span>`:""}</div>`:"";
  const pst=planState(c);const planBox=pst&&pst!=="cancelled"&&(colOf(c)==="Planning"||pst!=="started")?`<div class="shipbox"><h3>Planning</h3><div>${planChip(c)||esc(pst)}</div>${c.plan.detail?`<div>${safe(c.plan.detail)}</div>`:""}${c.dryRun?`<span class="meta">Dry run</span>`:""}${c.plan.threadId?`<span class="meta">Comment thread on this card</span>`:""}</div>`:"";
  const loginBox=logins.length?`<div class="shipbox"><h3>Logins</h3><div class="logins">${logins.map((l,i)=>`<div class="login"${httpOk(l.url)?` data-url="${esc(l.url)}"`:""}><span class="br">${safe(l.browser||"Any browser")}</span><span class="who">${safe(l.label||"")}${l.persona?` · ${safe(l.persona)}`:""}</span><span class="url">${safe(l.url||"")}</span><div class="row">${httpOk(l.url)?`<button class="btn" type="button" data-open="${i}">Open in this browser</button><button class="btn ghost" type="button" data-copy="${i}">Copy link</button><span class="msg err stackst" hidden>Stack down: the dev stack isn&#39;t answering</span>`:""}</div></div>`).join("")}</div><p class="note">Open starts the browser named above, in its own Chrome profile. If you're not signed in there, sign in once in that profile.</p></div>`:"";
  const top=d.scrollTop;
  const lblBox=typeof labelEditor==="function"?labelEditor(c):""; // read before the redraw: it keeps what Hugh is typing
  const autoNow=autoPend&&autoPend.id===c.id?autoPend.v:autoOf(c);
  d.innerHTML=`<span class="tags">${typeof queuedBadge==="function"?queuedBadge(c.id):""}${shipChip(c)}${planChip(c)}${routeChip(c)}<span class="tag">${esc(colOf(c))}</span>${isArch(c)?`<span class="tag">archived</span>`:""}${n?(httpOk(issueUrl(c))?`<a class="tag" href="${esc(issueUrl(c))}" target="_blank" rel="noopener">issue ${esc(n)}</a>`:`<span class="tag">issue ${esc(n)}</span>`):""}${c.pr&&httpOk(c.pr)?`<a class="tag" href="${esc(c.pr)}" target="_blank" rel="noopener">PR</a>`:""}</span>
  ${lblBox}
  ${typeof evidenceHtml==="function"?evidenceHtml(c):""}
  ${shipBox}${planBox}${typeof smallBox==="function"?smallBox(c):""}${panelParts(c)}
  ${boardHooks.details.map(f=>hookTry(f,c)||"").join("")}
  <label class="f">Summary<textarea id="f-summary" rows="4" ${canWrite?"":"disabled"}></textarea></label>
  <label class="f">Next step<textarea id="f-next" rows="3" ${canWrite?"":"disabled"}></textarea></label>
  <label class="f">Column<select class="colsel" id="f-col" ${canWrite?"":"disabled"}>${(BOARD_COLS.includes(colOf(c))?BOARD_COLS:[...BOARD_COLS,colOf(c)]).map(x=>`<option${x===colOf(c)?" selected":""}>${x}</option>`).join("")}</select></label>
  <label class="f">Goes on to<select class="colsel" id="f-auto" title="Saved as soon as you choose" ${canWrite?"":"disabled"}>${AUTO_OPTS.map(([v,l])=>`<option value="${esc(v)}"${v===autoNow?" selected":""}>${esc(l)}</option>`).join("")}</select></label>
  <div class="row"><button class="btn" id="save" type="button" ${canWrite?"":"disabled"}>Save</button>${canWrite?`<button class="btn ghost" id="arch" type="button">${isArch(c)?"Restore":"Archive"}</button>`:""}<span class="msg" id="msg"></span></div>
  <div id="staleBox" hidden></div>
  ${isArch(c)?`<p class="note">Archived${c.archivedAt?` ${esc(c.archivedAt)}`:""}${c.archivedBy?` by ${safe(c.archivedBy)}`:""}. It stays in the board's data; Restore puts it back in ${esc(restoreCol(c))}.</p>`:""}
  ${colOf(c)==="Blocked"||c.blockedReason?`<dl><dt>Blocked on</dt><dd>${safe(c.blockedReason||"—")}</dd></dl>`:""}
  ${typeof stackDetail==="function"?stackDetail(c):""}
  ${loginBox}
  ${reproBox(c)}
  ${typeof subagentsBox==="function"?subagentsBox(c):""}
  <h3>Links</h3>
  <dl>
   <dt>Issue</dt><dd>${n?(httpOk(issueUrl(c))?`<a href="${esc(issueUrl(c))}" target="_blank" rel="noopener">#${esc(n)}</a>`:"#"+esc(n)):"—"}</dd>
   <dt>PR</dt><dd>${c.pr?safeUrl(c.pr):"—"}</dd>
   <dt>Session</dt><dd>${safe(c.sessionTitle||"—")}${c.sessionId?`<br><span class="meta">${esc(c.sessionId)}</span>`:""}${sessionBtn?`<br>${sessionBtn}`:""}</dd>
   <dt>Dev stack</dt><dd>${c.devStack?safeUrl(c.devStack):"—"}${c.persona?`<br><span class="meta">as ${safe(c.persona)}</span>`:""}</dd>
  </dl>
  ${links.length?`<ul class="log">${links.map(l=>`<li>${esc(l&&l.label||"Link")}: ${safeUrl(l&&l.url)}</li>`).join("")}</ul>`:""}
  <h3>Details</h3>
  <dl>
   <dt>Branch</dt><dd class="mono">${safe(c.branch||"—")}</dd>
   <dt>Account</dt><dd>${c.account==="account2"?"Account 2":c.account==="main"?"Main":"—"}</dd>
   <dt>Updated</dt><dd>${esc(c.updatedAt||"—")}${c.updatedBy?`<br><span class="meta">${safe(c.updatedBy)}</span>`:""}</dd>
   <dt>Repo</dt><dd>${safe(c.repo||"—")}</dd>
   ${stuckRow(c)}${checkRows(c)}
   ${routeRow(c)}
   <dt>Card</dt><dd class="mono">${cardNum(c)?`${cardNum(c)} · `:""}${esc(c.id)}</dd>
  </dl>
  ${typeof timesSection==="function"?timesSection(c):""}
  ${log.length?`<h3>Log</h3><ul class="log">${log.map(e=>`<li><time>${esc(e&&e.at||"")}</time>${safe(e&&e.text)}</li>`).join("")}</ul>`:""}`;
  // The evidence shows once (card #101): other boxes may call evidenceHtml(c) too; the first .ev wins.
  d.querySelectorAll(".ev").forEach((x,i)=>{if(i)x.remove()});
  if(typeof showEvidence==="function")showEvidence(d);
  d.scrollTop=top;
  // The reproducer's shots (card #112): drawn through image_read, and full size on a click.
  if(typeof showShots==="function"){showShots(d);d.querySelectorAll(".repro img.shot").forEach(img=>img.onclick=()=>{if(img.src&&typeof openShot==="function")openShot(img)});d.querySelectorAll(".rp-earlier").forEach(x=>x.addEventListener("toggle",()=>{if(x.open)showShots(x)}))}
  const ttl=panel.el.querySelector(".ph .ttl");if(ttl)ttl.innerHTML=`${numBadge(c)}${n?`<span class="iss">#${esc(n)}</span>`:""}${safe(c.title||"Untitled card")}`;
  const sm=$("#f-summary"),nx=$("#f-next"),cs=$("#f-col");
  const clean=v=>SECRET.test(v||"")?"":(v||"");
  if(draft&&draft.id===c.id){sm.value=draft.summary;nx.value=draft.nextStep;cs.value=draft.col}
  else{sm.value=clean(c.summary);nx.value=clean(c.nextStep);draft={id:c.id,summary:sm.value,nextStep:nx.value,col:colOf(c)}}
  d.querySelectorAll("[data-copy]").forEach(btn=>btn.onclick=()=>{const l=logins[+btn.dataset.copy];if(!l)return;
    const done=()=>{btn.textContent="Copied"};
    try{navigator.clipboard.writeText(l.url).then(done,()=>{selectUrl(btn)})}catch(e){selectUrl(btn)}});
  d.querySelectorAll("[data-open]").forEach(btn=>btn.onclick=()=>{const l=logins[+btn.dataset.open];if(!l)return;
    invoke("open_login",{url:l.url,browser:l.browser||""}).then(()=>{btn.textContent="Opened"},err=>say("Couldn't open it: "+errText(err),"err"))});
  checkStacks(c);
  if(typeof labelWire==="function")labelWire(c,d);
  if(typeof smallWire==="function")smallWire(c,d);
  panelPartsWire(c,d);
  {const ab=$("#arch");if(ab)ab.onclick=()=>archiveCard(c.id,!isArch(c))}
  {const rb=$("#route-again");if(rb)rb.onclick=async()=>{rb.disabled=true;
    try{await invoke("route_again",{cardId:c.id});say("Route cleared: the session runs the card-router again","ok")}
    catch(err){rb.disabled=false;say("Couldn't route it again: "+errText(err),"err")}}}
  // Goes on to: saved at once, with no popup (card #87). Deploy complete says what it means in the saved line.
  {const as=$("#f-auto");if(as)as.onchange=async()=>{const v=as.value,cur=()=>autoOf(cards.find(x=>x.id===c.id)||c);
    autoPend={id:c.id,v};say("");const ok=await write(c.id,{autoTo:v||null});autoPend=null;
    if(ok){say(v==="Deploy complete"?"Saved: it ships with no human test once it is built":"Saved","ok");return}
    const s=$("#f-auto");if(s&&typeof panel!=="undefined"&&panel&&panel.id===c.id)s.value=cur();
    const mm=$("#msg");if(mm&&!mm.textContent)say("Not saved. Try again.","err")}}
  $("#save").onclick=async()=>{const ok=await saveText(c.id,cs.value,sm.value,nx.value);if(ok!==false)draft=null};
  // A refused save stays on show across live updates until Hugh re-applies or discards it.
  if(stale&&stale.id===c.id&&stale.cur)showStale(stale.cur);
  if(said&&said.id===c.id&&Date.now()-said.at<5000){const m=$("#msg");if(m&&!m.textContent){m.textContent=said.t;m.className="msg "+(said.kind||"")}}
}

function selectUrl(btn){const u=btn.closest(".login").querySelector(".url");const r=document.createRange();r.selectNodeContents(u);const sel=getSelection();sel.removeAllRanges();sel.addRange(r);btn.textContent="Selected: press Ctrl+C"}
// The panel's message line. A save's own live update redraws the panel straight after, so the last
// line is kept for a few seconds and put back by renderDetail (card #87).
let said=null;
function say(t,kind){const m=$("#msg");if(m){m.textContent=t;m.className="msg "+(kind||"")}
  said=t?{id:typeof panel!=="undefined"&&panel?panel.id:null,t,kind,at:Date.now()}:null}

// Every write goes to the Rust store, pinned to the version this page is showing.
const invoke=(cmd,args)=>window.__TAURI__.core.invoke(cmd,args);
const verOf=id=>{const c=cards.find(x=>x.id===id);return c?c.version:0};
const errText=e=>e&&typeof e==="object"?(e.message||JSON.stringify(e)):String(e||"unknown error");
async function call(cmd,args,quiet){
  try{return await invoke(cmd,args)}
  catch(e){
    if(e&&e.kind==="conflict")throw e;
    const msg=e&&e.kind==="secret"?`Not saved: ${e.message}`:"Couldn't save: "+errText(e)+". Try again.";
    if(!quiet){say(msg,"err");status(msg,true)}return null}
}
async function write(id,patch,quiet){
  const bad=Object.entries(patch).find(([k,v])=>typeof v==="string"&&SECRET.test(v));
  if(bad){say(`Not saved: "${bad[0]}" looks like it holds a token, key or password. Sign-ins and secrets never go on this board.`,"err");return false}
  try{return (await call("update_card",{id,version:verOf(id),patch,by:"Hugh (board)"},quiet))||false} // the saved card (truthy), or false
  catch(e){status(`Card ${id} changed meanwhile; the board has been refreshed.`,true);await load();return false}
}
let busy=false;
// Cards are archived, never deleted: the record and its log stay in the board's data.
async function archiveCard(id,on){const c=cards.find(x=>x.id===id);if(!c||busy||isArch(c)===on)return;busy=true;
  let ok=null;try{ok=await call("archive_card",{id,version:c.version,on})}catch(e){await load()}busy=false;if(!ok)return;
  status(on?"Archived: it's in the Archive column":"Restored to "+(ok.column||restoreCol(c)));
  if(on&&typeof panel!=="undefined"&&panel&&panel.id===id)closePanel()}
// A card dropped in the Archive box moves to Done, which archives it and stops its session.
// A card mid-ship stays put: its session is pushing and staging.
async function archiveDrop(id){const c=cards.find(x=>x.id===id);if(!c||isArch(c)||colOf(c)==="Done")return;
  if(colOf(c)==="Ship it"&&(shipState(c)==="shipping"||shipState(c)==="requested")){status(`#${cardNum(c)||id} is shipping: archive it once the ship has finished`,true);return}
  await moveCard(id,"Done")}
// Drop a card at a position: above beforeId, or after the last card shown when beforeId is null.
async function placeCard(id,col,beforeId){
  const c=cards.find(x=>x.id===id);if(!c||isArch(c)||busy)return;
  // The column as shown: cards that need Hugh or hold a place first (needs.js, card #132).
  const pins=typeof pinFirst==="function",full=pins?pinFirst(colCards(col)):colCards(col),rest=full.filter(x=>x.id!==id);
  const pinN=pins?rest.filter(isPinned).length:0; // the pinned cards lead `rest`
  let idx;
  if(beforeId){idx=rest.findIndex(x=>x.id===beforeId);if(idx<0)idx=rest.length}
  else{const vis=[...document.querySelectorAll(`.col[data-col="${CSS.escape(col)}"] .card`)].map(k=>k.dataset.id).filter(x=>x!==id);
    const last=vis.length?rest.findIndex(x=>x.id===vis[vis.length-1]):-1;idx=last+1}
  const same=colOf(c)===col;
  if(same&&full.findIndex(x=>x.id===id)===idx)return;
  // Dropped among the pinned cards (needs.js, card #132): they show by their needs' times, not by
  // order, so the dropped card and the held ones get holds that match the order shown; the cards that
  // need Hugh keep theirs. One that needs him itself stays where its need puts it. Nothing is written.
  if(idx<pinN){
    if(!same){await moveCard(id,col);await load();const m=cards.find(x=>x.id===id);if(!m||colOf(m)!==col)return}
    const arr=[...rest];arr.splice(idx,0,c);
    holdPlaces(arr.slice(0,pinN+1).filter(x=>x.id!==id||!needs[id]),col);
    if(needs[id])noHold.set(id,col);
    render();if(same)status(`Reordered ${col}`);return;
  }
  // A pinned card's order says nothing about where it shows, so a drop just below the pinned cards
  // has no card above it.
  const prev=idx>pinN?rest[idx-1]:null,next=rest[idx];
  let order;
  if((prev&&!hasOrd(prev))||(next&&!hasOrd(next))||(prev&&next&&!(prev.order<next.order))){
    // A neighbour has no order yet, or the orders run backwards: number the whole column as it is
    // shown, with the moved card in its new place.
    const arr=[...rest];arr.splice(idx,0,c);busy=true;
    const res=[];for(const [i,x] of arr.entries())res.push(x.id===id||(hasOrd(x)&&x.order===i+1)?true:await write(x.id,{order:i+1,orderCol:col},true));
    busy=false;if(res.includes(false))return;
    order=idx+1;
  }else order=prev&&next?(prev.order+next.order)/2:next?next.order-1:prev?prev.order+1:1;
  // The dragged card's own place is his drop now: no hold, and none when its need clears.
  if(pins){endHolds([id]);if(needs[id])noHold.set(id,col)}
  if(same){busy=true;const ok=await write(id,{order,orderCol:col},true);busy=false;if(ok)status(`Reordered ${col}`)}
  else moveCard(id,col,{order,orderCol:col});
}
async function moveCard(id,col,extra){const c=cards.find(x=>x.id===id);if(!c||isArch(c)||colOf(c)===col||busy)return;busy=true;
  let r=null;try{r=await call("move_card",{id,version:c.version,column:col,extra:extra||{}})}
  catch(e){status(`Card ${id} changed meanwhile; the board has been refreshed. Try the move again.`,true);await load()}
  busy=false;if(!r)return;
  if(r.ship&&r.shipWaitFor)status(`Moved to Ship it: it waits for #${r.shipWaitFor} to finish shipping, then its session is told`);
  else if(r.ship)status("Moved to Ship it: the card's session is told to ship it");
  else if(r.plan)status("Moved to Planning: a session starts on it, or its session is told");
  else status(col==="Done"?"Archived: it's in the Archive column, and its session has stopped":`Moved to ${col}`)}
// A save refused because the card changed keeps Hugh's text: he sees the card as it is now and re-applies or discards.
let stale=null;
async function saveText(id,col,summary,nextStep){if(busy)return false;busy=true;const c=cards.find(x=>x.id===id);
  const version=stale&&stale.id===id?stale.version:c.version;
  const patch={summary,nextStep};
  try{
    let ok=await call("update_card",{id,version,patch,by:"Hugh (board)"});
    if(ok&&c&&col!==colOf(c)){ok=await call("move_card",{id,version:ok.version,column:col,extra:{}})}
    busy=false;if(!ok)return false;stale=null;say("Saved","ok");return true}
  catch(e){busy=false;
    if(e&&e.kind==="conflict"){stale={id,version:e.current.version,summary,nextStep,col,cur:e.current};await load();showStale(e.current);return false}
    say("Couldn't save: "+errText(e),"err");return false}
}
function showStale(cur){
  const box=$("#staleBox");if(!box)return;
  box.hidden=false;
  box.innerHTML=`<p class="note">This card changed while you were editing (it is now at version ${esc(cur.version)}). Your text is still in the boxes above. This is the card as it is now:</p>
  <dl><dt>Summary</dt><dd>${safe(cur.summary||"—")}</dd><dt>Next step</dt><dd>${safe(cur.nextStep||"—")}</dd><dt>Column</dt><dd>${esc(cur.column)}</dd></dl>
  <div class="row"><button class="btn" id="reapply" type="button">Re-apply my text</button><button class="btn ghost" id="discard" type="button">Discard my text</button></div>`;
  $("#reapply").onclick=()=>{const d=stale;if(!d)return;saveText(d.id,d.col,$("#f-summary").value,$("#f-next").value).then(ok=>{if(ok){draft=null;box.hidden=true}})};
  $("#discard").onclick=()=>{stale=null;draft=null;box.hidden=true;renderDetail()};
}
// Links open in the system browser; logins open in the Chrome profile they name.
document.addEventListener("click",e=>{const a=e.target.closest("a[href]");if(!a)return;const href=a.getAttribute("href");
  if(/^(https?|claude):\/\//i.test(href)){e.preventDefault();invoke("open_external",{url:href}).catch(err=>status("Couldn't open it: "+errText(err),true))}});
async function checkStacks(c){
  if(window.KBStack)return window.KBStack.check(c);   // stack.js: branch and a restart command too
  for(const row of document.querySelectorAll(".login[data-url]")){
    const url=row.dataset.url;let up=true;
    try{up=await invoke("stack_up",{url})}catch(e){up=true}
    const st=row.querySelector(".stackst");if(st){st.hidden=up}
    const open=row.querySelector("[data-open]");if(open)open.hidden=!up}
}

// Add card: Hugh's own task. It lands in Backlog with no session attached, or goes on by itself as far as
// "Take it to" says (card #76; create_card's `to`). It always starts at Backlog: the last choice isn't kept.
const TAKE_TO={
  "Backlog":{hint:"It waits in Backlog until you move it.",btn:"Add to Backlog",done:" to Backlog"},
  "Planning":{hint:"A session plans it now, then waits for you.",btn:"Add and plan",done:": a session is planning it"},
  "Human test":{hint:"A session plans and builds it now, and hands it to you to test.",btn:"Add and build",done:": a session is planning and building it"},
  "Deploy complete":{hint:"A session plans, builds and ships it with no human test. For most repos that merges and deploys its PR (as your Ship it prompt says); for this app it stages the live update.",btn:"Add and ship",done:": a session is planning, building and shipping it",warn:true}};
// The repos a card can be for, from the private folder's repos.json (Settings), as repo -> short label.
const APP_REPO="hughht5/kanban-app";
let REPOS={[APP_REPO]:"kanban-app"};
const loadRepos=async()=>{try{const l=await invoke("list_repos");REPOS={[APP_REPO]:"kanban-app"};for(const x of l)REPOS[x.repo]=x.label}catch(e){}};
const repoOptions=()=>Object.keys(REPOS).filter(r=>r!==APP_REPO).map(r=>`<option>${esc(r)}</option>`).join("")+`<option value="${APP_REPO}">${APP_REPO} (this app)</option><option value="">Other</option>`;
const slug=t=>String(t).toLowerCase().replace(/[^a-z0-9]+/g,"-").replace(/^-+|-+$/g,"").slice(0,40)||"card";
async function openAdd(){
  await loadRepos();
  const d=$("#addDlg");
  d.innerHTML=`<form class="dlg" id="addForm" novalidate>
  <div class="dlg-head"><div class="ttl"><h2>Add card</h2><span class="meta">Take it to sets how far it goes by itself before it waits for you.</span></div>
   <button class="close" id="addClose" type="button" aria-label="Close">✕</button></div>
  <div class="dlg-body"><div class="colL" style="display:flex;flex-direction:column;gap:12px">
   <label class="f">Template<select class="colsel" id="a-tpl"><option value="">No template</option></select></label>
   <label class="f">Title<input class="tx" id="a-title" maxlength="200" placeholder="Search: remove the status filter"></label>
   <label class="f">Description<textarea id="a-desc" rows="4" placeholder="What needs doing, and anything a session should know"></textarea></label>
   <label class="f">Repo<select class="colsel" id="a-repo">${repoOptions()}</select></label>
   <label class="f">Take it to<select class="colsel" id="a-to" aria-describedby="a-hint">${Object.keys(TAKE_TO).map(k=>`<option>${esc(k)}</option>`).join("")}</select></label>
   <p class="hint" id="a-hint"></p>
   <div id="a-subs"></div>
   <label class="f">GitHub issue (optional)<input class="tx" id="a-issue" placeholder="https://github.com/owner/repo/issues/1234"></label>
   <label class="f">Links (optional, one per line: a label, then the address)<textarea id="a-links" rows="2" placeholder="Vault doc https://..."></textarea></label>
   <div class="row"><button class="btn" id="a-save" type="submit">Add to Backlog</button><span class="msg" id="a-msg"></span></div>
  </div></div></form>`;
  const m=t=>{const el=$("#a-msg");el.textContent=t;el.className="msg err"};
  $("#addClose").onclick=()=>d.close();
  $("#addForm").addEventListener("submit",async e=>{e.preventDefault();
    const title=$("#a-title").value.trim(),summary=$("#a-desc").value.trim(),repo=$("#a-repo").value,issueTxt=$("#a-issue").value.trim();
    if(!title){m("Give the card a title.");return}
    const links=[];for(const line of $("#a-links").value.split(/\n/)){const t=line.trim();if(!t)continue;const mm=t.match(/^(.*?)\s*(https?:\/\/\S+)$/);
      if(!mm){m(`"${t}" has no web address at the end.`);return}links.push({label:mm[1]||"Link",url:mm[2]})}
    let issue=null;
    if(issueTxt){const im=issueTxt.match(/^https?:\/\/github\.com\/([^\/\s]+\/[^\/\s]+)\/issues\/(\d+)/i);
      if(!im){m("The issue must be a github.com/owner/repo/issues/number address.");return}
      issue={number:+im[2],url:`https://github.com/${im[1]}/issues/${im[2]}`}}
    const all=[title,summary,issueTxt,...links.map(l=>l.label+" "+l.url)].join(" ");
    if(SECRET.test(all)){m("Not saved: something here looks like a token, key or password. Sign-ins and secrets never go on this board.");return}
    const short=REPOS[repo]||(issue?issue.url.split("/")[4]:"");
    const id=issue&&short?`${short}-${issue.number}`:`${slug(title)}-${Math.random().toString(36).slice(2,6)}`;
    const btn=$("#a-save");btn.disabled=true;
    if(cards.some(x=>x.id===id)){m(`There's already a card for this issue (${id}). Open that one instead.`);btn.disabled=false;return}
    try{
      // The store gives the number, from its counter, in the same transaction.
      const fields={title,summary};
      if(repo)fields.repo=repo;if(issue)fields.issue=issue;if(links.length)fields.links=links;
      if($("#a-tpl").value)fields.templateId=$("#a-tpl").value; // its starting prompt opens the card's first session
      if(typeof subagentChoice==="function"&&$("#a-subs").querySelector("input"))fields.subagents=subagentChoice($("#a-subs")); // card #151
      const to=Object.hasOwn(TAKE_TO,$("#a-to").value)?$("#a-to").value:"Backlog";
      const args={id,fields};if(to!=="Backlog")args.to=to; // the store moves it on and sets autoTo
      const c=await invoke("create_card",args);
      d.close();status(`Added ${c&&c.number?c.number+" ":""}"${title}"${TAKE_TO[to].done}`);
    }catch(err){m("Couldn't add it: "+errText(err)+". Try again.");btn.disabled=false}
  });
  {const to=$("#a-to"),h=$("#a-hint"),b=$("#a-save");
   const sync=()=>{const t=TAKE_TO[to.value]||TAKE_TO.Backlog;h.textContent=t.hint;h.classList.toggle("warn",!!t.warn);b.textContent=t.btn};
   to.addEventListener("change",sync);sync()}
  try{d.showModal()}catch(e){d.setAttribute("open","")}
  if(typeof templatePicker==="function")templatePicker($("#a-tpl"),$("#a-title"),$("#a-desc"));else $("#a-tpl").closest("label").hidden=true;
  if(typeof subagentTicks==="function")subagentTicks($("#a-subs")); // card #151: set from Settings' defaults
  // A change to this app: its repo and the "Kanban app" template go together (the template's
  // starting prompt sends the card's session to this repo's AGENTS.md and prompts/kanban-app.md).
  {const rp=$("#a-repo"),tp=$("#a-tpl"),APP=APP_REPO,TPL="kanban-app";
   rp.addEventListener("change",()=>{if(rp.value===APP&&!tp.value&&[...tp.options].some(o=>o.value===TPL)){tp.value=TPL;tp.dispatchEvent(new Event("change",{bubbles:true}))}});
   tp.addEventListener("change",()=>{if(tp.value===TPL)rp.value=APP})}
  $("#a-title").focus();
}
{const a=$("#addDlg");a.addEventListener("click",e=>{if(e.target===a)a.close()});a.addEventListener("close",()=>{a.innerHTML="";const b=$("#addBtn");if(b)b.focus()})}
$("#addBtn").onclick=openAdd;

$("#q").addEventListener("input",e=>{q=e.target.value.trim().toLowerCase();render()});
render();

// Cards come from the Rust store; it sends "cards-changed" after every write, from any source.

async function load(){
  try{cards=await invoke("list_cards");loaded=true;
    const live=cards.filter(c=>!isArch(c)&&!isSystemCard(c)).length,gone=cards.filter(c=>isArch(c)&&!isSystemCard(c)).length;
    let where="";try{where=await invoke("board_location")}catch(e){}
    status(`${live} card${live===1?"":"s"} · ${gone} archived`+(where?` · ${where}`:""),false,true);render()}
  catch(e){loaded=false;status("Couldn't read the board: "+errText(e)+". Your cards are safe; quit the app from the tray and open it again.",true);render()}
}
// A test build (KANBAN_CHANNEL=test, test2, ...) shows a warning bar and its name from the app,
// "Claude Kanban DSH (test)" or "Claude Kanban DSH (test 2)", in its titles.
async function showChannel(){
  let ch="live";try{ch=await invoke("app_channel")}catch(e){}
  if(ch==="live")return;
  let name="Claude Kanban DSH (test)";try{name=await invoke("app_name")}catch(e){}
  $("#testBar").hidden=false;document.title=name;$("header h1").textContent=name;
  const label=(/\((test[^)]*)\)/.exec(name)||[])[1]||"test";
  if(label!=="test")$("#testBar b").textContent=label.toUpperCase()+" VERSION:";
}
(async()=>{
  canWrite=true;$("#addBtn").hidden=false;
  showChannel();
  await load();
  if(selected&&cards.some(x=>x.id===selected))openPanel(selected);
  try{await window.__TAURI__.event.listen("cards-changed",()=>load())}catch(e){}
})();

// Keep the board where Hugh was when it redraws (a session finishing, a pinned card moving to
// the top): each column's scroll, and the board's own, are put back after the redraw.
{
  const draw=render;
  render=function(){
    const b=document.getElementById("board"),cols={};
    if(b)for(const w of b.querySelectorAll(".col"))for(const s of [w,...w.querySelectorAll(".cards")])if(s.scrollTop)cols[w.dataset.col+(s===w?"":"/cards")]=s.scrollTop;
    const left=b?b.scrollLeft:0,top=b?b.scrollTop:0;
    draw.apply(this,arguments);
    if(!b)return;
    for(const w of b.querySelectorAll(".col")){
      if(cols[w.dataset.col])w.scrollTop=cols[w.dataset.col];
      const c=w.querySelector(".cards");if(c&&cols[w.dataset.col+"/cards"])c.scrollTop=cols[w.dataset.col+"/cards"];
    }
    b.scrollLeft=left;b.scrollTop=top;
  };
}
