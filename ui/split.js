// The line between a session pane's live log and its chat (card #185). When the pane stacks the
// log above the chat (narrower than 618px), Hugh drags the line, or focuses it and presses the
// arrow keys, to give one more height and the other less. Each part scrolls inside its own height,
// and neither goes below its minimum. The log's height is kept per panel type (the Router's, every
// other session's) in localStorage, so it survives a restart; a double-click puts back the default.
// Dragging never moves the focus: the pointerdown's default is prevented, so a half-typed message
// keeps its caret. router.js calls PaneSplit.attach for each pane it opens.
(()=>{
const MIN_LOG=60,MIN_CHAT=160,STEP=16,BIG=64;
// The key it is kept under, by what the pane shows.
const keyFor=kind=>"rpane.split."+(kind==="router"?"router":"session");
function read(k){try{const v=parseFloat(localStorage.getItem(k));return v>0?v:null}catch(e){return null}}
function write(k,v){try{if(v==null)localStorage.removeItem(k);else localStorage.setItem(k,String(Math.round(v)))}catch(e){}}
// The log's height kept within its limits. `room`: the log's and the chat's heights together.
function clamp(h,room){
  const max=Math.max(MIN_LOG,(room||0)-MIN_CHAT);
  return Math.round(Math.max(MIN_LOG,Math.min(Number(h)||0,max)));
}
// The log's height after a key on the handle, or null for a key the handle doesn't take.
function keyStep(key,shift,h,room){
  const d=shift?BIG:STEP;
  if(key==="ArrowUp")return clamp(h-d,room);
  if(key==="ArrowDown")return clamp(h+d,room);
  if(key==="Home")return clamp(MIN_LOG,room);
  if(key==="End")return clamp(room,room);
  return null;
}
// Wires the handle. grid: the pane's grid (it carries --log-h and the split-set class); feed: the
// live log; right: the Chat | Terminal part below it. Returns {apply, reset, refit, height}.
function attach({handle,grid,feed,right,kind}){
  const k=keyFor(kind);
  const height=()=>feed.getBoundingClientRect().height;
  const room=()=>height()+right.getBoundingClientRect().height;
  // Only while the handle shows: stacked, and the log not folded.
  const shown=()=>{try{return getComputedStyle(handle).display!=="none"}catch(e){return true}};
  function aria(h){
    const r=room();handle.setAttribute("aria-valuemin",String(MIN_LOG));
    handle.setAttribute("aria-valuemax",String(Math.max(MIN_LOG,Math.round(r-MIN_CHAT))));
    handle.setAttribute("aria-valuenow",String(Math.round(h==null?height():h)));
  }
  function apply(h){
    if(h==null){grid.style.removeProperty("--log-h");grid.classList.remove("split-set")}
    else{grid.style.setProperty("--log-h",h+"px");grid.classList.add("split-set")}
    aria(h);
  }
  // The saved height, clamped to the room the pane has now (a smaller window, a longer digest).
  // The room is measured with the default split: a height set for a taller window can overflow
  // the pane, and then the log's height plus the chat's is not the room there is (the tester's
  // find on card #185: a shorter window left the chat at 0 px). Both reads happen before a paint.
  function refit(){
    const saved=read(k);
    apply(null);
    if(saved==null||!shown())return;
    apply(clamp(saved,room()));
  }
  function reset(){write(k,null);apply(null)}
  handle.addEventListener("pointerdown",e=>{
    if(e.button!==0)return;
    e.preventDefault(); // keeps the focus (and the caret) where Hugh is typing
    const y0=e.clientY,h0=height(),r0=room();
    document.body.classList.add("resizing-v");handle.classList.add("on");
    try{handle.setPointerCapture(e.pointerId)}catch(x){}
    let last=h0;
    const move=ev=>{last=clamp(h0+(ev.clientY-y0),r0);apply(last)};
    const up=()=>{
      document.body.classList.remove("resizing-v");handle.classList.remove("on");
      removeEventListener("pointermove",move);removeEventListener("pointerup",up);removeEventListener("pointercancel",up);
      if(Math.round(last)!==Math.round(h0))write(k,last);
    };
    addEventListener("pointermove",move);addEventListener("pointerup",up);addEventListener("pointercancel",up);
  });
  // The mouse events after the pointerdown would focus the handle (it has a tabindex): not on a click.
  handle.addEventListener("mousedown",e=>e.preventDefault());
  handle.addEventListener("dblclick",e=>{e.preventDefault();reset()});
  handle.addEventListener("keydown",e=>{
    const h=keyStep(e.key,e.shiftKey,height(),room());
    if(h==null)return;
    e.preventDefault();apply(h);write(k,h);
  });
  try{new ResizeObserver(()=>{if(!document.body.classList.contains("resizing-v"))refit()}).observe(grid)}catch(e){}
  refit();
  return{apply,reset,refit,height};
}
window.PaneSplit={attach,clamp,keyStep,keyFor,read,MIN_LOG,MIN_CHAT,STEP,BIG};
})();
