// Labels on cards (card #71): Hugh's own short tags, like BUG, UX or DOCS. A card's `labels` is a
// list of strings, cleaned and checked by the store (crates/store/src/cards.rs, clean_labels).
// Each label keeps one colour everywhere, picked from its name. They are square-cornered and
// filled, so they read apart from the round, outlined state chips (Shipped, Ship failed...).
// board.js calls labelTags, labelMatch, labelEditor and labelWire.
const LBL_MAX=8,LBL_LEN=24;
const labelsOf=c=>Array.isArray(c&&c.labels)?c.labels.filter(l=>typeof l==="string"&&l.trim()):[];
const lblKey=l=>String(l).trim().toLowerCase();
// A hue from the name, so a label is the same colour on every card. Bug-like labels are red.
function lblHue(l){const k=lblKey(l);if(/\bbugs?\b|\bfix\b/.test(k))return 356;
  let h=0;for(const ch of k)h=(h*31+ch.codePointAt(0))>>>0;
  return [212,268,160,32,190,300,120,48,236,330,180,84][h%12]}
const lblChip=(l,extra)=>`<span class="lbl" style="--h:${lblHue(l)}"${extra||""}>${esc(l)}</span>`;
// On a card in the column: each label is a button that filters the board to that label.
function labelTags(c){return labelsOf(c).map(l=>`<button class="lbl" type="button" style="--h:${lblHue(l)}" data-noopen data-label="${esc(l)}" title="Show only cards labelled ${esc(l)}">${esc(l)}</button>`).join("")}
// The search box: "label:BUG" shows only cards with exactly that label (any case).
function labelMatch(c,q){const m=/^label:\s*(.+)$/.exec(q);if(!m)return null;const k=m[1].trim();return labelsOf(c).some(l=>lblKey(l)===k)}
document.addEventListener("click",e=>{const b=e.target.closest("button.lbl[data-label]");if(!b)return;e.preventDefault();e.stopPropagation();
  // Adds the label's term to the filter, or takes it out again; other terms stay (they all must match).
  const box=$("#q"),l=b.dataset.label,v=/\s/.test(l)?`label:"${l}"`:"label:"+l;box.value=qToggle(box.value,v);box.dispatchEvent(new Event("input",{bubbles:true}))},true);

// Every label on the board, most used first: the add box suggests them.
function boardLabels(){const n=new Map();for(const c of cards)for(const l of labelsOf(c)){const k=lblKey(l);const e=n.get(k);if(e)e.n++;else n.set(k,{l,n:1})}
  return [...n.values()].sort((a,b)=>b.n-a.n||a.l.localeCompare(b.l)).map(e=>e.l)}

// In the card's panel: its labels, each with a remove button, and a box to add one. Saved at once.
let lblTyping=null; // {id, value, focused}: what Hugh is typing in the add box, kept across live redraws
function labelEditor(c){
  const box=document.getElementById("f-label");
  if(box&&box.dataset.id===c.id)lblTyping={id:c.id,value:box.value,focused:document.activeElement===box};
  else if(!lblTyping||lblTyping.id!==c.id)lblTyping=null;
  const ls=labelsOf(c),used=new Set(ls.map(lblKey));
  const sug=boardLabels().filter(l=>!used.has(lblKey(l)));
  const chips=ls.map((l,i)=>canWrite?`<span class="lbl" style="--h:${lblHue(l)}">${esc(l)}<button class="lbl-x" type="button" data-unlabel="${i}" aria-label="Remove the label ${esc(l)}" title="Remove">✕</button></span>`:lblChip(l)).join("");
  const add=canWrite&&ls.length<LBL_MAX?`<input class="lbl-in" id="f-label" data-id="${esc(c.id)}" list="lbl-list" maxlength="${LBL_LEN}" placeholder="${ls.length?"Add":"Add a label"}" aria-label="Add a label" autocomplete="off"><datalist id="lbl-list">${sug.map(l=>`<option value="${esc(l)}">`).join("")}</datalist>`:"";
  return`<div class="lbl-row"><span class="lbl-h">Labels</span>${chips}${add}${!ls.length&&!add?`<span class="meta">none</span>`:""}<span class="msg" id="lbl-msg"></span></div>`;
}
function labelWire(c,d){
  const box=d.querySelector("#f-label");
  const msg=t=>{const m=d.querySelector("#lbl-msg");if(m){m.textContent=t||"";m.className="msg"+(t?" err":"")}};
  const save=async next=>{const ok=await write(c.id,{labels:next.length?next:null});if(ok===false)msg("Not saved. Try again.");return ok};
  if(box){
    if(lblTyping&&lblTyping.id===c.id){box.value=lblTyping.value;if(lblTyping.focused){box.focus();box.setSelectionRange(box.value.length,box.value.length)}}
    const commit=async()=>{const parts=box.value.split(",").map(s=>s.trim().replace(/\s+/g," ")).filter(Boolean);if(!parts.length)return;
      const cur=labelsOf(c),next=[...cur];
      for(const p of parts){if(p.length>LBL_LEN){msg(`"${p}" is longer than ${LBL_LEN} characters.`);return}
        // An existing board label in another case is used as it is spelt there, so BUG stays BUG.
        const known=boardLabels().find(l=>lblKey(l)===lblKey(p));const v=known||p;
        if(!next.some(l=>lblKey(l)===lblKey(v)))next.push(v)}
      if(next.length>LBL_MAX){msg(`A card takes at most ${LBL_MAX} labels.`);return}
      lblTyping={id:c.id,value:"",focused:true};box.value="";
      if(next.length!==cur.length)await save(next)};
    box.addEventListener("keydown",e=>{if(e.key==="Enter"||e.key===","){e.preventDefault();commit()}
      else if(e.key==="Backspace"&&!box.value&&labelsOf(c).length){e.preventDefault();save(labelsOf(c).slice(0,-1))}});
    // Picking a suggestion from the list adds it straight away.
    box.addEventListener("input",e=>{msg("");if(e.inputType==="insertReplacementText"||!e.inputType)commit()});
    box.addEventListener("blur",()=>{if(lblTyping&&lblTyping.id===c.id)lblTyping.focused=false});
  }
  d.querySelectorAll("[data-unlabel]").forEach(b=>b.onclick=()=>{const ls=labelsOf(c);const i=+b.dataset.unlabel;if(i<ls.length)save(ls.filter((_,j)=>j!==i))});
}
