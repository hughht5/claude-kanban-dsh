// The pill row under the header (card #120, the mockup's .filters): one button per quick filter,
// "Waiting on you · 2", "May be stuck · 1", "30-second looks · 4", and the label:x terms in the
// filter box. A click adds or takes out the pill's term in #q with board.js's qToggle, so the pills
// combine with each other and with typed terms (every term must match).
//
// registerPill({key, label, tone, term, count(cards), order, id, title, deps})
//   key    unique name; label: the words before " · N"; tone: "ask", "warn", "ok" or "" (colour when N > 0)
//   term   the filter term it toggles ("is:waiting"); count(cards): how many cards it would show
//   order  its place: Waiting on you 10, May be stuck 20, Router asks 30 (card #119), 30-second looks 40, labels 90
//   id     the button's id (needs.js keeps "waitBtn", check-size.js "checkSizeBtn"); title: its hover text
//   deps() optional: other values the count reads (needs.js's `needs`), so a change to them redraws it
// A pill with count 0 hides unless its term is in the box. The row works its counts out only when
// the cards, the box or a pill's deps change, and touches a button only when its own text changes.
// Uses board.js's $, cards, q, qTerms, qToggle, Q_TERM and boardHooks.
const FB_PILLS=[];
let fbMemo={key:null,sig:""};
function registerPill(p){
  if(!p||typeof p!=="object"||!p.key||FB_PILLS.some(x=>x.key===p.key))return;
  FB_PILLS.push(p);FB_PILLS.sort((a,b)=>(+a.order||0)-(+b.order||0));
  fbMemo.key=null;fbDraw();
}
window.registerPill=registerPill;

// The box's text with `term` taken out, whichever way it was typed: quoted, "label: x", or an
// unquoted label with spaces (qTerms joins those). Unchanged when it isn't there.
function fbRemove(s,term){
  s=String(s||"");const key=t=>String(t).replace(/"/g,"").toLowerCase().trim(),k=key(term);
  const ts=s.match(Q_TERM)||[];
  for(let i=0;i<ts.length;i++)for(let j=i;j<Math.min(ts.length,i+7);j++){
    const part=ts.slice(i,j+1),a=key(part.join(" ")),b=/^label:$/i.test(part[0])?key(part[0]+part.slice(1).join(" ")):null;
    if(a===k||b===k)return[...ts.slice(0,i),...ts.slice(j+1)].join(" ")}
  return s;
}
function fbSet(v){const box=$("#q");if(!box)return;box.value=v;box.dispatchEvent(new Event("input",{bubbles:true}))}
function fbBar(){
  let bar=document.getElementById("filterbar");if(bar)return bar;
  bar=document.createElement("div");bar.id="filterbar";bar.className="filterbar";bar.setAttribute("role","group");bar.setAttribute("aria-label","Quick filters");
  const head=document.querySelector("header");if(head&&head.after)head.after(bar);else document.body.appendChild(bar);
  return bar;
}
// The cards a pill counts: on the board, not archived, not the router's card, not a stack's hidden child.
const fbTop=c=>!!c&&!isArch(c)&&colOf(c)!=="Done"&&!(typeof isSystemCard==="function"&&isSystemCard(c))&&!(typeof stackHidden==="function"&&stackHidden(c));
window.fbTop=fbTop;

// What the row should show now: [{key, id, text, title, tone, term, n, on, hot}]. Pure on its inputs.
function fbModel(list,terms){
  const out=[];
  for(const p of FB_PILLS){
    let n=0;try{n=Number(p.count(list))||0}catch(e){console.error("pill",p.key,e)}
    const on=terms.includes(String(p.term).toLowerCase());
    if(!n&&!on)continue;
    out.push({key:p.key,id:p.id||"",text:`${p.label} · ${n}`,title:p.title||`Show only these cards. Click again to show them all.`,tone:p.tone||"",term:p.term,n,on,hot:n>0});
  }
  // The label terms in the box: each a pressed pill, and a click takes it out.
  for(const t of terms)if(/^label:/i.test(t))out.push({key:"label:"+t,id:"",text:t,title:`Filtered to ${t}. Click to take it out.`,tone:"",term:t,n:0,on:true,hot:false,label:true});
  return out;
}
function fbDraw(){
  const list=typeof cards!=="undefined"&&Array.isArray(cards)?cards:[];
  const qs=typeof q==="string"?q:"";
  const deps=FB_PILLS.map(p=>{try{return typeof p.deps==="function"?p.deps():null}catch(e){return null}});
  const key=[list,qs,...deps];
  if(fbMemo.key&&fbMemo.key.length===key.length&&fbMemo.key.every((v,i)=>v===key[i]))return;
  fbMemo.key=key;
  const model=fbModel(list,typeof qTerms==="function"?qTerms(qs):[]);
  const sig=JSON.stringify(model);if(sig===fbMemo.sig&&document.getElementById("filterbar"))return;
  fbMemo.sig=sig;
  const bar=fbBar(),have=new Map([...bar.children].map(b=>[b.dataset.key,b]));
  let prev=null;
  for(const m of model){
    let b=have.get(m.key);have.delete(m.key);
    if(!b){b=document.createElement("button");b.type="button";b.className="fbtn";b.dataset.key=m.key;
      b.addEventListener("click",()=>{const box=$("#q");if(!box)return;const t=b.dataset.term;
        fbSet(b.dataset.label==="1"?fbRemove(box.value,t):qToggle(box.value,t));
        // The focus goes back to the search box, caret at the end, as the old Waiting-on-you button did.
        try{box.focus({preventScroll:true});const n=box.value.length;if(box.setSelectionRange)box.setSelectionRange(n,n)}catch(e){}})}
    if(m.id&&b.id!==m.id)b.id=m.id;
    if(b.textContent!==m.text)b.textContent=m.text;
    if(b.title!==m.title)b.title=m.title;
    b.dataset.term=m.term;b.dataset.label=m.label?"1":"";
    if(m.tone)b.dataset.tone=m.tone;else delete b.dataset.tone;
    b.setAttribute("aria-pressed",m.on?"true":"false");
    b.classList.toggle("on",m.on);b.classList.toggle("hot",m.hot);b.classList.toggle("lblpill",!!m.label);
    const want=prev?prev.nextSibling:bar.firstChild;if(want!==b)bar.insertBefore(b,want);
    prev=b;
  }
  for(const b of have.values())b.remove();
  bar.hidden=!model.length;
}
window.fbDraw=fbDraw;window.fbModel=fbModel;window.fbRemove=fbRemove;
boardHooks.rendered.push(fbDraw);

// May be stuck (card #99's monitor): the is:stuck term, from board.js.
registerPill({key:"stuck",label:"May be stuck",tone:"warn",term:"is:stuck",order:20,
  title:"Show only the cards whose session may be stuck. Click again to show them all. It combines with other filter terms.",
  count:list=>list.filter(c=>fbTop(c)&&isStuckCard(c)).length});

// The pills other files queued before this file loaded (needs.js, check-size.js), and any later ones.
for(const p of Array.isArray(window.kbPills)?window.kbPills:[])registerPill(p);
window.kbPills={push:p=>registerPill(p)};
