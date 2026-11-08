// The Done pile (card #131). The Router asks to archive a card whose work is live
// (kanban_archive_card puts `archiveAsk` on it); the card then waits here, in a column before the
// Archive, instead of in its own column. Hugh's "Archive all" moves every card shown here into
// Done, as his Archive drop does (router_archive_approve); "Not now" on a card puts it back and
// stops the Router asking again until he or its session changes it (router_archive_dismiss).
// The pile is drawn only while it holds a card. The rules live in the store; this only draws and asks.
// Uses board.js's cards, colOf, isArch, isSystemCard, matches, cardEl, cardNum, esc, canWrite,
// status, load and boardHooks, and panel.js's activity (read only if it is there).

const PILE_LIVE=["starting","idle","working","permission"];
const pileAsk=c=>c&&c.archiveAsk&&typeof c.archiveAsk==="object"?c.archiveAsk:null;
// A card the Router asked to archive and that isn't archived yet: it sits in the pile.
function inDonePile(c){return !!pileAsk(c)&&!isArch(c)&&!(typeof isSystemCard==="function"&&isSystemCard(c))}
// The pile's cards, as the board shows them (the filter applies), oldest ask first.
function pileCards(all){return (all||[]).filter(c=>inDonePile(c)&&(typeof matches!=="function"||matches(c)))
  .sort((a,b)=>String(pileAsk(a).at||"").localeCompare(String(pileAsk(b).at||"")))}
// What a button sends: each card's id and the version the board drew it at.
const pileItems=list=>list.map(c=>({id:c.id,version:c.version}));
function pileLive(id,act){const a=act||(typeof activity!=="undefined"?activity:null);return !!(a&&PILE_LIVE.includes(a[id]))}
// The running sessions archiving `c` would stop: its own, and for a lead each child's (by number).
function pileRunning(c,all,act){
  const own=pileLive(c.id,act);
  const kids=(Array.isArray(c.stack)?c.stack:[]).map(n=>(all||[]).find(k=>k.number===n)).filter(k=>k&&!isArch(k)&&pileLive(k.id,act)).map(k=>k.number);
  return {own,kids};
}
// The note under a card in the pile: where it came from, the Router's reason and evidence, its
// stack, the sessions it would stop, and why the last click left it here, if it did.
function pileNote(c,all,act,why){
  const a=pileAsk(c)||{},r=pileRunning(c,all,act),parts=[];
  parts.push(`<div class="pile-from">From ${esc(a.column||colOf(c))}</div>`);
  if(a.reason)parts.push(`<div class="pile-reason">${esc(a.reason)}</div>`);
  if(a.evidence)parts.push(`<div class="pile-evidence">${esc(a.evidence)}</div>`);
  const n=Array.isArray(c.stack)?c.stack.length:0;
  if(n)parts.push(`<div class="pile-stack">and its ${n} ${n===1?"child":"children"}</div>`);
  if(r.own)parts.push(`<div class="pile-run">Its session is running: archiving stops it</div>`);
  if(r.kids.length){const ns=r.kids.map(k=>"#"+k);const list=ns.length>1?ns.slice(0,-1).join(", ")+" and "+ns[ns.length-1]:ns[0];
    parts.push(`<div class="pile-run">${esc(list)} ${r.kids.length===1?"is":"are"} running: archiving stops ${r.kids.length===1?"it":"them"}</div>`)}
  if(why)parts.push(`<div class="pile-why">Left here: ${esc(why)}</div>`);
  return `<div class="pile-note">${parts.join("")}</div>`;
}

// Why the last click left a card in the pile, by card id, until the next click.
let pileWhy={};
let pileBusy=false;

function pileCol(list){
  const el=document.createElement("section");el.className="col c-DonePile";el.dataset.col="Done pile";
  el.innerHTML=`<div class="colhead"><span class="dot"></span>Done pile<span class="n">${list.length}</span></div>
   <div class="pile-head">The Router put these here: their work is live.</div>
   ${canWrite?`<button class="pile-all" type="button"${pileBusy?" disabled":""}>Archive all ${list.length}</button>`:""}
   <div class="cards"></div>`;
  const wrap=el.querySelector(".cards");
  for(const c of list){
    const row=document.createElement("div");row.className="pile-row";row.dataset.id=c.id;
    row.appendChild(cardEl(c));
    row.insertAdjacentHTML("beforeend",pileNote(c,cards,null,pileWhy[c.id]));
    if(canWrite){const b=document.createElement("button");b.type="button";b.className="pile-not";b.textContent="Not now";
      b.title="Put it back where it was. The Router won't ask again until you or its session change the card.";
      b.addEventListener("click",e=>{e.stopPropagation();pileDismiss([c])});row.appendChild(b)}
    wrap.appendChild(row);
  }
  const all=el.querySelector(".pile-all");
  if(all)all.addEventListener("click",()=>pileArchiveAll(list));
  return el;
}

// One click for the whole pile: every card shown moves into Done. A card the store refuses stays,
// with the reason on it; the others go.
async function pileArchiveAll(list){
  if(pileBusy||!list.length)return;
  pileBusy=true;pileWhy={};
  let out=null;
  try{out=await window.__TAURI__.core.invoke("router_archive_approve",{items:pileItems(list)})}
  catch(e){status("Couldn't archive the Done pile: "+errText(e),true)}
  pileBusy=false;
  if(Array.isArray(out)){
    const done=out.filter(r=>r.done).length,left=out.filter(r=>!r.done);
    for(const r of left)pileWhy[r.id]=r.why||"refused";
    status(left.length?`Archived ${done}; ${left.length} left in the Done pile: ${left.map(r=>"#"+r.number+" "+(r.why||"")).join("; ")}`:`Archived ${done} from the Done pile. Restore in a card brings it back.`,left.length>0);
  }
  if(typeof load==="function")await load();else render();
}

async function pileDismiss(list){
  if(pileBusy||!list.length)return;
  pileBusy=true;
  let out=null;
  try{out=await window.__TAURI__.core.invoke("router_archive_dismiss",{items:pileItems(list)})}
  catch(e){status("Couldn't put it back: "+errText(e),true)}
  pileBusy=false;
  if(Array.isArray(out)){const bad=out.filter(r=>!r.done);
    if(bad.length)status(bad.map(r=>"#"+r.number+": "+(r.why||"refused")).join("; "),true);
    else status(`Put back: ${out.map(r=>"#"+r.number).join(", ")}. The Router won't ask again until the card changes.`)}
  if(typeof load==="function")await load();else render();
}

// Draw the pile before the Archive column after every render, while it holds a card.
boardHooks.rendered.push(()=>{
  const board=document.getElementById("board");if(!board)return;
  const old=board.querySelector(".col.c-DonePile");if(old)old.remove();
  const list=pileCards(cards);if(!list.length)return;
  const el=pileCol(list),arch=board.querySelector(".col.c-Archived");
  board.insertBefore(el,arch||null);
});
