// Column widths (card #130): drag a column's right edge to change its width; the board keeps each
// width as a share (a weight), so every column scales with the window. The weights live in
// localStorage, `wb.colWeights`: {"<column>": weight}, the smallest weight 1. board.css turns a
// weight into `flex: w 0 w*COL_MIN`: a column is w x COL_MIN when the board overflows, and w x board /
// sum of weights when there is room. Nothing here touches cards or the data.
const COL_MIN=160;      // px; keep in step with .col in board.css
const COL_KEY="wb.colWeights";
const COL_DEAD=3;       // px the pointer must move before a press counts as a drag
const COL_DOUBLE=400;   // ms between two presses on an edge that reset the widths

// Weights for these columns: a missing, non-finite or non-positive value is 1; the smallest becomes 1.
function colNormalise(map,keys){
  const v=keys.map(k=>{const w=Number(map&&map[k]);return Number.isFinite(w)&&w>0?w:1});
  const m=Math.min(...v);
  return Object.fromEntries(keys.map((k,i)=>[k,v[i]/m]));
}
function colStored(){try{const o=JSON.parse(localStorage.getItem(COL_KEY)||"{}");return o&&typeof o==="object"?o:{}}catch(e){return {}}}
function colStore(map){try{if(map)localStorage.setItem(COL_KEY,JSON.stringify(map));else localStorage.removeItem(COL_KEY)}catch(e){}}

// Pure: the weights after dragging column i's right edge by dx px, from the columns' measured widths.
// Column i takes from its right neighbour while that has room; a neighbour at COL_MIN stays there and
// column i still widens (the board scrolls further); narrowing gives the width to the neighbour.
function colWeightsAfterDrag(widths,i,dx){
  const j=i+1;if(j>=widths.length)return colNormaliseList(widths);
  const out=widths.slice();
  out[i]=Math.max(COL_MIN,widths[i]+dx);
  out[j]=Math.max(COL_MIN,widths[j]+(widths[i]-out[i]));
  return colNormaliseList(out);
}
function colNormaliseList(w){const m=Math.min(...w);return w.map(x=>x/m)}

let colDrag=null;   // the weights of a drag in progress, by column name; render() reads it meanwhile
let colLast=null;   // the last press on an edge, for the double-press reset
const colBoard=()=>document.getElementById("board");
const colEls=()=>[...colBoard().querySelectorAll(":scope > .col")];

// Sets --w on every column (looked up afresh: render() replaces them) and gives each but the last an edge.
function colApply(){
  const b=colBoard();if(!b)return;
  const els=colEls(),keys=els.map(c=>c.dataset.col);
  const w=colDrag||colNormalise(colStored(),keys);
  els.forEach((c,i)=>{
    c.style.setProperty("--w",String(w[c.dataset.col]??1));
    if(i<els.length-1&&!c.querySelector(":scope > .colgrip")){
      const g=document.createElement("div");g.className="colgrip";g.dataset.i=String(i);
      g.title="Drag to change the column width. Press twice to reset.";c.appendChild(g);
    }
  });
}
function colReset(){colStore(null);colDrag=null;colApply()}

function colStart(e,i){
  const els=colEls(),keys=els.map(c=>c.dataset.col),widths=els.map(c=>c.getBoundingClientRect().width),x0=e.clientX;
  let moved=false;
  document.body.classList.add("resizing");document.body.style.cursor="col-resize";
  const move=ev=>{
    const dx=ev.clientX-x0;
    if(!moved&&Math.abs(dx)<COL_DEAD)return;
    moved=true;
    const w=colWeightsAfterDrag(widths,i,dx);
    colDrag=Object.fromEntries(keys.map((k,j)=>[k,w[j]]));
    colApply();
  };
  const up=()=>{
    removeEventListener("pointermove",move);removeEventListener("pointerup",up);removeEventListener("pointercancel",up);
    document.body.classList.remove("resizing");document.body.style.cursor="";
    if(moved&&colDrag)colStore(colDrag);
    colDrag=null;
  };
  addEventListener("pointermove",move);addEventListener("pointerup",up);addEventListener("pointercancel",up);
}

{
  const b=colBoard();
  if(b)b.addEventListener("pointerdown",e=>{
    const g=e.target&&e.target.closest?e.target.closest(".colgrip"):null;
    if(!g||(e.button||0)!==0)return;
    e.preventDefault();
    const i=+g.dataset.i,t=e.timeStamp;
    if(colLast&&colLast.i===i&&t-colLast.t<COL_DOUBLE){colLast=null;colReset();return}
    colLast={i,t};
    colStart(e,i);
  });
  boardHooks.rendered.push(colApply);
}
