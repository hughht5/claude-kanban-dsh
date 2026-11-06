// Step progress on each card (Hugh, card #106): "Step 4 of 7: Writing the tests" and a thin bar,
// from the to-do list the card's session keeps (steps.rs). Its tooltip lists every step.
// Uses board.js's helpers (esc, invoke, isArch, cards).
//
// It never redraws the board: the steps of every card come in one call (card_steps), and only
// each tile's own .steps element changes, so a drag, an open menu or a field being typed in isn't
// disturbed. The element is put on each tile after a render through whichever hook the board
// has: #105's addCardPart (slot 20), else #107's boardHooks.decorate, else a MutationObserver.
let steps={}; // cardId -> {done, total, current?, line, items:[{text, status}]}

const STEP_MARK={completed:"✓",in_progress:"▶",pending:"○"};
function stepsTitle(s){
  return s.items.map((x,i)=>`${STEP_MARK[x.status]||"○"} ${i+1}. ${x.text}`).join("\n");
}
// Fills (or empties and hides) one tile's .steps element. Its parts are made once and then only
// their text and the bar's width change, so the bar slides from one width to the next.
function stepsFill(el,id){
  const s=steps[id];
  if(!s){if(!el.hidden){el.hidden=true;el.textContent="";el.removeAttribute("title");el.removeAttribute("aria-label")}return}
  let t=el.querySelector(".steps-t"),fill=el.querySelector(".steps-bar>span");
  if(!t||!fill){
    el.textContent="";
    t=document.createElement("span");t.className="steps-t";
    const bar=document.createElement("span");bar.className="steps-bar";bar.setAttribute("aria-hidden","true");
    fill=document.createElement("span");bar.appendChild(fill);el.append(t,bar);
  }
  const pct=(s.total?Math.round(100*s.done/s.total):0)+"%";
  // "Step 4 of 7: Writing the tests" as two ends of one row (card #120, the mockup's .aprog).
  if(t.dataset.line!==s.line){t.dataset.line=s.line;const m=/^(Step \d+ of \d+)(?::\s*(.*))?$/.exec(s.line||"");
    t.textContent="";const a=document.createElement("span");a.className="steps-n";a.textContent=m?m[1]:s.line;t.appendChild(a);
    if(m&&m[2]){const w=document.createElement("span");w.className="steps-w";w.textContent=m[2];t.appendChild(w)}}
  if(fill.style.width!==pct)fill.style.width=pct;
  el.hidden=false;
  const title=stepsTitle(s),label=`${s.line}. ${s.done} of ${s.total} steps done.`;
  if(el.title!==title)el.title=title;
  if(el.getAttribute("aria-label")!==label)el.setAttribute("aria-label",label);
  el.classList.toggle("all-done",s.total>0&&s.done===s.total);
}
const stepsCard=id=>(typeof cards!=="undefined"?cards:[]).find(c=>c.id===id);
// The element as HTML, for addCardPart's line.
function stepsLine(c){
  if(!c||isArch(c))return"";
  const el=document.createElement("span");el.className="steps";el.dataset.steps=c.id;stepsFill(el,c.id);
  return el.outerHTML;
}
// Puts the element on a tile that has none, under its summary (or title).
function stepsPlace(tile,c){
  if(!tile||!c||isArch(c))return;
  let el=tile.querySelector(".steps");
  if(!el){
    el=document.createElement("span");el.className="steps";el.dataset.steps=c.id;
    const after=tile.querySelector(".stk-sum")||tile.querySelector(".stk-top")||tile.querySelector(":scope > .s")||tile.querySelector(":scope > .t");
    if(after)after.after(el);else tile.prepend(el);
  }
  stepsFill(el,c.id);
}
// Every tile in place, after new steps arrive.
function stepsPaint(){
  for(const tile of document.querySelectorAll(".card[data-id]")){
    const c=stepsCard(tile.dataset.id);
    if(c&&!isArch(c))stepsPlace(tile,c);
  }
}
async function loadSteps(){
  let s;try{s=await invoke("card_steps")}catch(e){return}
  if(JSON.stringify(s||{})===JSON.stringify(steps))return;
  steps=s||{};stepsPaint();
}
let stepsTimer=null;
const stepsSoon=()=>{clearTimeout(stepsTimer);stepsTimer=setTimeout(loadSteps,150)};

(async()=>{
  if(typeof addCardPart==="function"){
    addCardPart({at:20,line:c=>stepsLine(c)});
  }else if(typeof boardHooks!=="undefined"&&boardHooks&&Array.isArray(boardHooks.decorate)){
    boardHooks.decorate.push((tile,c)=>stepsPlace(tile,c));
  }else{
    // No shared hook in this build: put the element on each tile after the board draws it.
    const board=document.getElementById("board");
    // Its own .steps changes are left out, so painting never sets it off again.
    const own=n=>n.nodeType!==1||n.classList.contains("steps")||!!n.closest(".steps");
    if(board)new MutationObserver(muts=>{if(muts.some(m=>[...m.addedNodes].some(n=>!own(n))))stepsPaint()}).observe(board,{childList:true,subtree:true});
  }
  await loadSteps();
  const ev=window.__TAURI__&&window.__TAURI__.event;
  if(ev)for(const name of ["steps-changed","session-changed"])ev.listen(name,stepsSoon);
  setInterval(loadSteps,60000);
})();
