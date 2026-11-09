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
// The to-do list in the open card's chat (card #158), pinned above the message box where the
// terminal shows it: "6 tasks (4 done, 1 in progress, 1 open)", then each task, ✓ done (struck
// through), ■ in progress, □ open. The same list as the tile's (this visit's steps). The header
// folds it, remembered for every card ("panel.todo"). A list longer than TODO_MAX hides its
// earliest done tasks behind "… +N completed", which shows them all until the panel closes.
// Hidden while the panel shows a subagent's conversation: the list is the main session's.
// A redraw keeps the box's scroll and, if a button in it had focus, gives focus to the header.
const TODO_MAX=8,TODO_MARK={completed:"✓",in_progress:"■",pending:"□"};
// The fold: kept in memory, saved to localStorage as a best effort (so it still folds if storage throws).
let todoFold=null;
function todoFolded(v){
  if(v===undefined){if(todoFold===null){try{todoFold=localStorage.getItem("panel.todo")==="folded"}catch(e){todoFold=false}}return todoFold}
  todoFold=!!v;try{localStorage.setItem("panel.todo",v?"folded":"open")}catch(e){}
}
function todoCounts(items){
  const n={completed:0,in_progress:0,pending:0};
  for(const x of items)n[x.status in n?x.status:"pending"]++;
  return n;
}
function todoHead(items){
  const n=todoCounts(items),k=items.length,parts=[];
  if(n.completed===k)return `${k} task${k===1?"":"s"} (all done)`;
  if(n.completed)parts.push(`${n.completed} done`);
  if(n.in_progress)parts.push(`${n.in_progress} in progress`);
  if(n.pending)parts.push(`${n.pending} open`);
  return `${k} task${k===1?"":"s"} (${parts.join(", ")})`;
}
// Which tasks show: all of them, or (past TODO_MAX) the list without its earliest done ones.
function todoShown(items,all){
  if(all||items.length<=TODO_MAX)return{show:items,hid:0};
  let drop=items.length-TODO_MAX;const show=[];
  for(const x of items){if(drop>0&&x.status==="completed"){drop--;continue}show.push(x)}
  return{show,hid:items.length-show.length};
}
function stepsChat(){
  if(typeof panel==="undefined"||!panel||!panel.el)return;
  const el=panel.el.querySelector("#p-todo");if(!el)return;
  const s=panel.view?null:steps[panel.id],items=s&&Array.isArray(s.items)?s.items:[];
  if(!items.length){el.hidden=true;el.textContent="";delete el.dataset.key;return}
  const folded=todoFolded(),all=!!panel.todoAll;
  const key=JSON.stringify([items,folded,all]);
  if(el.dataset.key===key&&!el.hidden)return;
  const focused=typeof document!=="undefined"&&document.activeElement&&el.contains(document.activeElement),top=el.scrollTop||0;
  el.dataset.key=key;el.textContent="";el.hidden=false;
  el.classList.toggle("folded",folded);
  el.classList.toggle("all-done",items.every(x=>x.status==="completed"));
  const head=document.createElement("button");head.type="button";head.className="todo-h";
  head.setAttribute("aria-expanded",String(!folded));
  head.title=folded?"Show the session's to-do list":"Hide the session's to-do list";
  const chev=document.createElement("span");chev.className="todo-c";chev.setAttribute("aria-hidden","true");chev.textContent=folded?"▸":"▾";
  const ht=document.createElement("span");ht.className="todo-t";ht.textContent=todoHead(items);
  head.append(chev,ht);
  if(folded){const cur=items.find(x=>x.status==="in_progress");
    if(cur){const w=document.createElement("span");w.className="todo-now";w.textContent=cur.text;head.appendChild(w)}}
  head.onclick=()=>{todoFolded(!folded);stepsChat()};
  el.appendChild(head);
  const done=()=>{if(focused)head.focus();if(top)el.scrollTop=top};
  if(folded)return done();
  const {show,hid}=todoShown(items,all);
  const ul=document.createElement("ul");ul.className="todo-l";ul.setAttribute("aria-label","The session's to-do list");
  for(const x of show){
    const st=x.status in TODO_MARK?x.status:"pending";
    const li=document.createElement("li");li.className="todo-i "+st;
    const m=document.createElement("span");m.className="todo-m";m.setAttribute("aria-hidden","true");m.textContent=TODO_MARK[st];
    const t=document.createElement("span");t.className="todo-x";t.textContent=x.text;
    const sr=document.createElement("span");sr.className="sr-only";sr.textContent=st==="completed"?"Done: ":st==="in_progress"?"In progress: ":"Open: ";
    li.append(m,sr,t);ul.appendChild(li);
  }
  el.appendChild(ul);
  if(hid){const more=document.createElement("button");more.type="button";more.className="todo-more";
    more.textContent=`… +${hid} completed`;more.title="Show every task";
    more.onclick=()=>{if(panel)panel.todoAll=true;stepsChat()};el.appendChild(more)}
  done();
}
async function loadSteps(){
  let s;try{s=await invoke("card_steps")}catch(e){return}
  if(JSON.stringify(s||{})===JSON.stringify(steps))return;
  steps=s||{};stepsPaint();stepsChat();
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
