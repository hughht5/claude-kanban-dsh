// Stacked cards (design/stacked-cards/SPEC.md, option B): a lead card (`stack: [n, …]`) shows as
// one stack card with a progress strip, the stack's links once, and its children as rows that
// open in an overlay (how to test, Passed, Found a problem, answers). Children (`stackOf: n`) whose lead
// is on the board and not archived leave the columns. Uses board.js's helpers ($, esc, safe,
// httpOk, loginsOf, cards, colOf, isArch, cardNum, matches, render, invoke, errText, status) and
// panel.js's openPanel. board.js calls stackHidden, stackMatches, stackDecorate and stackDetail.
const STK_READY=["Human test","Ship it","Deploy complete","Done"];
const STK_SHIPPED=["Ship it","Deploy complete","Done"];
const STK_ICON=`<svg class="stk-ic" viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 4 4 8l8 4 8-4-8-4"/><path d="m4 12 8 4 8-4"/><path d="m4 16 8 4 8-4"/></svg>`;
const stkOpen={},stkForm={};   // lead number -> expanded; child id -> {kind,text}
let stkDlg=null;              // the child open in the overlay: {lead: lead id, kid: child id, el: the <dialog>}
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
function kidParts(lead,k,st){
  const f=stkForm[k.id],col=colOf(k),ready=STK_READY.includes(col);
  const steps=st.k==="need"?"":k.nextStep?`<div class="stk-md">${stkMd(k.nextStep)}</div>`:`<p class="stk-q">No how-to-test steps on this card yet.</p>`;
  let acts="";
  if(st.k==="need"){
    const choices=Array.isArray(k.choices)?k.choices.filter(x=>typeof x==="string"&&x.trim()):[];
    acts=`<div class="stk-ask"><b>Question:</b> ${safe(k.blockedReason||"The card is blocked; its session says why in the card.")}</div>
    ${choices.length?`<div class="stk-acts">${choices.map((x,i)=>`<button class="btn ghost sm" type="button" data-act="answer" data-kid="${esc(k.id)}" data-i="${i}">${safe(x)}</button>`).join("")}</div>`
      :`<div class="stk-form"><textarea rows="3" data-f="${esc(k.id)}" data-kind="answer" placeholder="Your answer, sent to the stack's session. Paste screenshots here too.">${esc(f&&f.kind==="answer"?f.text:"")}</textarea>
      <div class="stk-acts"><button class="btn sm" type="button" data-act="send" data-kid="${esc(k.id)}" data-kind="answer">Send</button></div></div>`}`;
  }else if(ready){
    const problem=f&&f.kind==="problem";
    acts=problem?`<div class="stk-form"><textarea rows="4" data-f="${esc(k.id)}" data-kind="problem" placeholder="What went wrong? Paste screenshots here too. It goes to the stack's session and the card goes back to In progress.">${esc(f.text||"")}</textarea>
      <div class="stk-acts"><button class="btn sm" type="button" data-act="send" data-kid="${esc(k.id)}" data-kind="problem">Send</button><button class="btn ghost sm" type="button" data-act="cancel" data-kid="${esc(k.id)}">Cancel</button></div></div>`
      :`<div class="stk-acts">${col==="Human test"?`<button class="btn sm${k.passed?" ghost":""}" type="button" data-act="passed" data-kid="${esc(k.id)}">${k.passed?"Undo passed":"Passed"}</button>`:""}<button class="btn ghost sm" type="button" data-act="problem" data-kid="${esc(k.id)}">Found a problem</button></div>`;
  }
  const foot=`<div class="stk-foot"><button class="stk-link" type="button" data-act="card" data-kid="${esc(k.id)}">Open card</button><span class="stk-col">${esc(col)}</span>${canWrite?`<button class="stk-quiet" type="button" data-act="detach" data-kid="${esc(k.id)}" title="Build it as its own card, with its own session">Detach</button>`:""}</div>`;
  return{steps,acts,foot};
}

// The overlay: one child at a time, over the dimmed board, wide enough to read its steps. The
// board redraws every few seconds, so render() redraws it from the latest card (stkDlgDraw).
function stkDlgHtml(lead,k){
  const st=kidState(k),{steps,acts,foot}=kidParts(lead,k,st),all=loginsOf(lead);
  const logins=all.filter(l=>httpOk(l.url)).map(l=>`<button class="stk-link" type="button" data-act="login" data-i="${all.indexOf(l)}">Sign in as ${safe(l.label||l.persona||"user")}</button>`);
  return`<div class="skd-head">
    <div class="skd-lead">${STK_ICON}Stack <span class="num">${esc(cardNum(lead)||"")}</span>${safe(lead.title||"Untitled stack")}</div>
    <h2 class="skd-t"><span class="stk-dot s-${st.k}"></span><span class="num">${esc(cardNum(k)||"")}</span>${safe(k.title||"Untitled card")}</h2>
    <div class="skd-meta"><span class="stk-state s-${st.k}">${esc(st.label)}</span>${logins.join("")}</div>
    <button class="skd-x" type="button" data-act="close" aria-label="Close" title="Close (Esc)">×</button></div>
  ${steps?`<div class="skd-body">${steps}</div>`:""}
  <div class="skd-foot">${acts}${foot}</div>`;
}
function stkKidOpen(leadId,kidId){
  stkDlgClose();
  const d=document.createElement("dialog");d.className="stk-kdlg";
  stkDlg={lead:leadId,kid:kidId,el:d,html:""};
  d.addEventListener("click",e=>{
    if(e.target===d){d.close();return}   // the backdrop
    const t=e.target.closest("[data-act]");if(t&&t.dataset.act==="close"){e.preventDefault();d.close();return}
    const lead=cards.find(x=>x.id===leadId);if(lead)stkClick(e,lead)});
  // Keep what's typed (an answer box has no entry until the first key), so a redraw keeps it.
  d.addEventListener("input",e=>{const f=e.target.closest("[data-f]");if(!f)return;
    const id=f.dataset.f;if(!stkForm[id])stkForm[id]={kind:f.dataset.kind,text:""};stkForm[id].text=f.value});
  d.addEventListener("keydown",e=>{const f=e.target.closest("[data-f]");if(!f)return;
    // Esc in a box closes the box (its text is dropped), not the overlay; Esc anywhere else closes the overlay.
    if(e.key==="Escape"){e.preventDefault();e.stopPropagation();delete stkForm[f.dataset.f];stkDlgDraw()}
    else if(e.key==="Enter"&&(e.ctrlKey||e.metaKey)){e.preventDefault();const s=f.parentNode.querySelector('[data-act="send"]');if(s)s.click()}});
  d.addEventListener("close",()=>{d.remove();if(stkDlg&&stkDlg.el===d)stkDlg=null});
  document.body.appendChild(d);
  stkDlgDraw();
  if(!stkDlg)return;   // the child is no longer in the stack
  try{d.showModal()}catch(e){d.setAttribute("open","")}
  const x=d.querySelector(".skd-x");if(x)x.focus();
}
function stkDlgClose(){if(stkDlg){const d=stkDlg.el;stkDlg=null;if(d.open)d.close();d.remove()}}
function stkDlgDraw(){
  if(!stkDlg)return;
  const lead=cards.find(x=>x.id===stkDlg.lead),k=lead&&isStackLead(lead)?stackKids(lead).find(x=>x.id===stkDlg.kid):null;
  if(!k){stkDlgClose();return}   // detached, removed, archived, or the stack is gone
  const html=stkDlgHtml(lead,k);
  if(html===stkDlg.html)return;
  const d=stkDlg.el,body=d.querySelector(".skd-body"),top=body?body.scrollTop:0;
  d.innerHTML=html;stkDlg.html=html;
  const nb=d.querySelector(".skd-body");if(nb)nb.scrollTop=top;
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
    case"session":openPanel(lead.id);break;
    case"login":{const l=loginsOf(lead)[+t.dataset.i];if(!l)return;
      invoke("open_login",{url:l.url,browser:l.browser||""}).then(()=>status(`Opened the sign-in for ${l.label||"the login"}`),err=>status("Couldn't open it: "+errText(err),true));break}
    case"kid":stkKidOpen(lead.id,t.dataset.kid);break;
    case"card":if(kid){stkDlgClose();openPanel(kid.id)}break;
    case"passed":if(kid){const on=!kid.passed;   // Passed closes the overlay; Undo passed leaves it open
      stkDo("set_passed",{id:kid.id,on},on?`#${kn} passed`:`#${kn} is no longer marked passed`).then(ok=>{if(ok&&on)stkDlgClose()})}break;
    case"problem":stkForm[t.dataset.kid]={kind:"problem",text:""};render();stkFocus(t.dataset.kid);break;
    case"cancel":delete stkForm[t.dataset.kid];render();break;
    case"answer":{const ch=kid&&Array.isArray(kid.choices)?kid.choices.filter(x=>typeof x==="string"&&x.trim())[+t.dataset.i]:null;if(!ch)return;
      t.disabled=true;stkDo("stack_answer",{id:kid.id,answer:ch},`Answered #${kn}: ${ch}`).then(ok=>{if(ok)stkDlgClose();else t.disabled=false});break}
    case"send":{if(!kid)return;const box=t.closest(".stk-form").querySelector("textarea"),text=box.value.trim();
      if(!text){status(t.dataset.kind==="problem"?"Say what went wrong first.":"Type an answer first.",true);box.focus();return}
      if(SECRET.test(text)){status("Not sent: it looks like it holds a token, key or password.",true);return}
      t.disabled=true;
      const p=t.dataset.kind==="problem"?stkDo("stack_problem",{id:kid.id,note:text},`Sent the problem with #${kn} to the stack's session; it's back in progress`)
        :stkDo("stack_answer",{id:kid.id,answer:text},`Sent your answer on #${kn}`);
      p.then(ok=>{if(ok){delete stkForm[kid.id];stkDlgClose();render()}else t.disabled=false});break}
    case"detach":if(!kid)return;
      stkConfirm({title:`Detach #${kn} from stack #${n}?`,from:kid,to:lead,arrow:"out of",
        lines:[["stop",`Stack #${n}'s session stops building it, and logs on #${kn} what it has already done.`],
               ["go",`#${kn} becomes its own card in Planning, and its own session starts there.`]],ok:"Detach"})
        .then(yes=>{if(yes){stkDlgClose();stkDo("stack_detach",{lead:n,child:kn},`#${kn} is its own card now, in Planning`)}});break;
    case"remove":if(!kid)return;
      if(!confirm(`Take #${kn} "${kid.title||""}" out of stack #${n}? It goes back on the board as its own card, in ${colOf(kid)}.`))return;
      stkDo("stack_remove",{lead:n,child:kn},`#${kn} is out of the stack, back in ${colOf(kid)}`);break;
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
