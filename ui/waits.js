// Cards that wait on others (card #109). A card's `blockedBy: [12, 40]` names the cards it waits on.
// The store checks every write (crates/store/src/waits.rs) and refuses a bad one with a plain
// message, shown under the panel's row. When every wait is met the app starts or tells the card and
// clears them; a card it started or told keeps them in `waitedOn` (the app's field, shown read-only).
// The card gets a "Waits on #12, #40" chip and the panel a "Waits on" row: through boardHooks
// (card #107) when board.js has them, else by wrapping cardEl and renderDetail. The row's controls
// listen on document, so they work whichever path drew it. Uses board.js's helpers and openPanel.
(()=>{
const MAX=20,MET_SAME=["Human test","Ship it","Deploy complete","Done"],MET_OTHER=["Deploy complete","Done"];
const nums=v=>Array.isArray(v)?[...new Set(v.map(Number).filter(n=>Number.isInteger(n)&&n>0))]:[];
const waitsOf=c=>nums(c&&c.blockedBy),waitedOf=c=>nums(c&&c.waitedOn);
const repoKey=c=>typeof c.repo==="string"?c.repo.trim().toLowerCase():"";
// Is waiter w's wait on blocker b met? The store's rule (is_met): in the same repo from Human test
// on, in another repo only once shipped, and an archived card only in Deploy complete or Done.
function waitMet(w,b){if(!b)return false;const col=colOf(b);
  if(isArch(b))return MET_OTHER.includes(col);
  return(repoKey(w)===repoKey(b)?MET_SAME:MET_OTHER).includes(col)}

// Cards by number, and the waits of every card that isn't archived, once per list (load() replaces it).
let memo=null;
function idx(){if(memo&&memo.of===cards)return memo;const by=new Map(),next=new Map();
  for(const c of cards){const n=cardNum(c);if(!n)continue;if(!by.has(n))by.set(n,c);if(!isArch(c)&&waitsOf(c).length)next.set(n,waitsOf(c))}
  return memo={of:cards,by,next,reach:new Map()}}
// A wait w -> b is in a circle when b's waits lead back to w (a depth-first walk, kept per card).
function reach(from){const m=idx();let s=m.reach.get(from);if(s)return s;s=new Set();const st=[from];
  while(st.length)for(const k of m.next.get(st.pop())||[])if(!s.has(k)){s.add(k);st.push(k)}
  m.reach.set(from,s);return s}
const inLoop=(w,b)=>reach(b).has(w);
// One wait: its card (null when there's no such card) and its state, met before circle.
function info(w,n){const b=idx().by.get(n)||null,wn=cardNum(w);
  return{n,b,st:!b?"gone":waitMet(w,b)?"met":isArch(b)?"arch":wn&&!isArch(w)&&inLoop(wn,n)?"loop":"wait"}}

// "12", "#12", "12, 40" or "#12 #40": the numbers, and anything that isn't one.
function parseNums(s){const out=[],bad=[];
  for(const p of String(s||"").split(/[\s,;]+/)){if(!p||p==="#")continue;const m=/^#?(\d{1,9})$/.exec(p),n=m?Number(m[1]):0;
    if(n>0){if(!out.includes(n))out.push(n)}else bad.push(p)}
  return{nums:out,bad}}

const cut=(s,m)=>s.length>m?s.slice(0,m-1).trimEnd()+"…":s;
const plainTitle=(b,m)=>{const t=String(b.title||"Untitled card");return SECRET.test(t)?"(title hidden: looks like a secret)":cut(t,m)};
const shortTitle=b=>{const t=String(b.title||"Untitled card");return SECRET.test(t)?safe(t):esc(cut(t,28))};
function tipLine(w,i){if(i.st==="gone")return`#${i.n}: there's no card with this number`;
  const b=i.b,col=colOf(b),where=isArch(b)?`archived in ${col}`:col;
  const why=i.st==="met"?"ready":i.st==="arch"?"it won't count: restore it or take it off"
    :i.st==="loop"?"in a circle of waits: none of them would start"
    :repoKey(w)!==repoKey(b)?"another repo: ready at Deploy complete":"ready from Human test";
  return`#${i.n} ${plainTitle(b,60)}: ${where} (${why})`}

// The card's chip: "Waits on #12, #40", or "Waits on #12 +3"; a circle first, met cards last.
const RANK={loop:0,wait:1,gone:2,arch:2,met:3};
function chip(c){const ns=waitsOf(c);if(!ns.length||isArch(c))return"";
  const all=ns.map(n=>info(c,n)),shown=[...all].sort((a,b)=>RANK[a.st]-RANK[b.st]);
  const num=i=>`<span class="wt-n is-${i.st}">#${i.n}</span>`;
  const txt=shown.length<=2?shown.map(num).join(", "):`${num(shown[0])} +${shown.length-1}`;
  const cls=all.some(i=>i.st==="loop")?" wt-loop":all.every(i=>i.st==="met")?" wt-ready":"";
  return`<span class="tag wt-chip${cls}" title="${esc(["Waits on:",...all.map(i=>tipLine(c,i))].join("\n"))}">Waits on ${txt}</span>`}
function decorate(b,c){const h=chip(c);if(!h)return;
  const tags=b.querySelector(":scope > .tags");
  if(tags){const l=tags.querySelectorAll(":scope > .lbl");if(l.length)l[l.length-1].insertAdjacentHTML("afterend",h);else tags.insertAdjacentHTML("afterbegin",h);return}
  const sub=b.querySelector(":scope > .stk-sub");if(sub)sub.insertAdjacentHTML("beforeend"," "+h)} // a stack card

// The panel's row: the waits, each removable, a box to add one, the store's refusal, a hint, the
// cards that wait on this one and what it waited on.
let typed=null; // {id, value, focused}: the add box, kept across live redraws
let note=null;  // {id, t, at}: the last refusal, kept under the row across redraws
function hint(c){const col=colOf(c);
  if(Number(c.stackOf)>0)return"In a stack: the stack's rules decide when it starts.";
  if(col==="Backlog")return"Drag it to Blocked to have it start by itself when these are ready.";
  if(col==="Blocked")return String(c.blockedReason||"").trim()?"Won't start by itself: it has a blocked reason. You'll get a notification when these are ready."
    :"Starts by itself in Planning when these are ready, if you moved it to Blocked yourself.";
  return col==="Done"?"":"Its session is told when these are ready."}
function row(c){
  const box=document.getElementById("wt-in"); // read before the redraw: it keeps what Hugh is typing
  typed=box&&box.dataset.id===c.id?{id:c.id,value:box.value,focused:document.activeElement===box}:null;
  const ns=waitsOf(c),me=cardNum(c),live=!isArch(c),edit=canWrite&&live,was=waitedOf(c);
  const by=me?cards.filter(x=>!isArch(x)&&waitsOf(x).includes(me)).sort((a,b)=>cardNum(a)-cardNum(b)):[];
  if(!ns.length&&!by.length&&!was.length&&!edit)return"";
  const open=(x,t)=>`<button class="wt-go" type="button" data-wt-open="${esc(x.id)}" title="${esc(t)}">#${esc(cardNum(x))}</button>`;
  const chips=ns.map(n=>{const i=info(c,n),b=i.b,col=b?colOf(b):"";
    const where=!b?"no such card":i.st==="met"?col+" ✓":i.st==="arch"?"archived in "+col:i.st==="loop"?col+" · circle":col;
    return`<span class="wt-c is-${i.st}" title="${esc(tipLine(c,i))}">${b?`${open(b,"Open #"+n)}<span class="wt-t">${shortTitle(b)}</span>`:`<span class="wt-num">#${n}</span>`}<span class="wt-col">${esc(where)}</span>${edit?`<button class="wt-x" type="button" data-wt-del="${n}" aria-label="Stop waiting on #${n}" title="Stop waiting on #${n}">✕</button>`:""}</span>`}).join("");
  const add=edit&&ns.length<MAX?`<input class="wt-in" id="wt-in" data-id="${esc(c.id)}" value="${esc(typed?typed.value:"")}" placeholder="Add a card number" aria-label="Add a card number this card waits on" title="A card number, then Enter. Several at once: 12, 40" autocomplete="off" spellcheck="false" maxlength="200">`:"";
  const msg=note&&note.id===c.id&&Date.now()-note.at<60000?note.t:"";
  const h=live&&ns.length?hint(c):"";
  if(typed&&typed.focused)queueMicrotask(()=>{const b=document.getElementById("wt-in");if(!b||b.dataset.id!==c.id||document.activeElement===b)return;
    b.focus({preventScroll:true});try{b.setSelectionRange(b.value.length,b.value.length)}catch(e){}});
  return`<div class="wt-box" data-wt-card="${esc(c.id)}"><div class="wt-row"><span class="wt-h">Waits on</span>${chips}${add}${!ns.length&&!add?`<span class="meta">nothing</span>`:""}</div>
   <div class="msg err wt-msg" id="wt-msg" role="status"${msg?"":" hidden"}>${esc(msg)}</div>
   ${h?`<p class="wt-hint">${esc(h)}</p>`:""}
   ${by.length?`<div class="wt-by">Waited on by ${by.map(x=>open(x,`#${cardNum(x)} ${plainTitle(x,60)}: ${colOf(x)}`)).join(", ")}</div>`:""}
   ${was.length?`<div class="meta wt-was" title="The app started or told this card when these were ready. It hears again if one goes back.">Waited on ${was.map(n=>"#"+n).join(", ")}</div>`:""}</div>`}

// Saving: the card's current waits through fn, pinned to its version. A card that changed meanwhile
// (its session writes often) is read again and tried once more. Returns "" or the store's message.
async function setWaits(id,fn){
  for(let i=0;;i++){const c=cards.find(x=>x.id===id);if(!c)return"this card isn't on the board any more";
    const cur=waitsOf(c),next=fn(cur);if(next.length===cur.length&&next.every((n,j)=>n===cur[j]))return"";
    try{await invoke("update_card",{id,version:c.version,patch:{blockedBy:next.length?next:null},by:"Hugh (board)"});return""}
    catch(e){if(e&&e.kind==="conflict"&&i<1){await load();continue}
      return e&&e.kind==="conflict"?"the card changed meanwhile. Try again":errText(e)}}}
function show(id,t){note=t?{id,t,at:Date.now()}:null;
  const m=document.getElementById("wt-msg"),box=m&&m.closest(".wt-box");
  if(box&&box.dataset.wtCard===id){m.textContent=t||"";m.hidden=!t}}
const refusal=t=>{t=String(t||"").trim();return"Not saved: "+t+(/[.!?]$/.test(t)?"":".")};
let pending=false;
async function commit(box){if(pending)return;const id=box.dataset.id,{nums:add,bad}=parseNums(box.value);
  if(bad.length){show(id,`"${cut(bad[0],20)}" isn't a card number. Type one like 12 or #12.`);return}
  const c=cards.find(x=>x.id===id);if(!add.length||!c)return;
  if(add.every(n=>waitsOf(c).includes(n))){show(id,`It waits on ${add.map(n=>"#"+n).join(", ")} already.`);return}
  const err=await once(id,cur=>[...cur,...add.filter(n=>!cur.includes(n))]);
  if(err){show(id,refusal(err));return}
  if(typed&&typed.id===id)typed.value="";const b=document.getElementById("wt-in");if(b&&b.dataset.id===id)b.value=""}
async function drop(id,n){if(pending)return;const err=await once(id,cur=>cur.filter(x=>x!==n));if(err)show(id,refusal(err))}
async function once(id,fn){pending=true;show(id,"");try{return await setWaits(id,fn)}catch(e){return errText(e)}finally{pending=false}}
const at=(e,s)=>e.target&&e.target.closest?e.target.closest(s):null;
document.addEventListener("keydown",e=>{const box=at(e,"input.wt-in");if(box&&e.key==="Enter"&&!e.isComposing){e.preventDefault();commit(box)}});
document.addEventListener("input",e=>{const box=at(e,"input.wt-in");if(box&&note&&note.id===box.dataset.id)show(box.dataset.id,"")});
document.addEventListener("click",e=>{const x=at(e,"[data-wt-del]");
  if(x){e.preventDefault();const box=x.closest("[data-wt-card]");if(box)drop(box.dataset.wtCard,Number(x.dataset.wtDel));return}
  const o=at(e,"[data-wt-open]");if(o&&typeof openPanel==="function"){e.preventDefault();openPanel(o.dataset.wtOpen)}});

// Hooking in: boardHooks (card #107) when board.js has them, else wrap cardEl and renderDetail.
if(typeof boardHooks!=="undefined"&&boardHooks&&Array.isArray(boardHooks.decorate)&&Array.isArray(boardHooks.details)){
  boardHooks.decorate.push(decorate);boardHooks.details.push(row)}
else{
  const baseCard=cardEl;cardEl=function(c){const b=baseCard.apply(this,arguments);try{decorate(b,c)}catch(e){console.error("waits",e)}return b};
  // The row goes where boardHooks.details would put it: above Summary. A redraw renderDetail skips
  // (Hugh typing in a box) keeps the row it has.
  const baseDet=renderDetail;renderDetail=function(){
    const c=typeof panel!=="undefined"&&panel?cards.find(x=>x.id===panel.id):null,d=c?panel.el.querySelector("#p-det"):null;
    let html="";try{html=d?row(c):""}catch(e){console.error("waits",e)}
    const top=d?d.scrollTop:0,r=baseDet.apply(this,arguments);
    if(d&&html&&d.isConnected&&!d.querySelector(".wt-box")){const f=d.querySelector(":scope > label.f");
      if(f)f.insertAdjacentHTML("beforebegin",html);else d.insertAdjacentHTML("beforeend",html);d.scrollTop=top}
    return r}}
window.KBWaits={met:waitMet,parse:parseNums,inLoop,info,chip,row,hint};
})();
