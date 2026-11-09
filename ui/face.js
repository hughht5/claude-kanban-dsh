// The card face in the mockup's order (card #120). It runs last on every card (board.js's
// boardHooks.final, after every decorate hook) and only moves the nodes the other files drew; the
// data they read stays theirs. A card's face, top to bottom:
//   title (need icon, state dot, #n, title) · the wait box or the stuck box · Done when · the step
//   line and bar · Human test shots · ONE chip row (labels, Waits on, Router, Critic, test, review,
//   repro, merge, check size, then every other chip, and the time in the column) · the newest agent
//   line (or the summary) · what changed · a muted row: repo, issue, PR, logins, stack, acct 2, ago.
// A stack card (stacks.js, .stk) keeps its own layout; face.css only restyles it.
// No chip or link is lost: every .tag, .chip and <a> the card had is still on it, only moved.
// Uses board.js's colOf, isArch, stuckOf, routeOf, checkOf, safe, esc, SECRET and boardHooks, and
// panel.js's loadShot and shotCache when they are there. The pure helpers are on window.Face for
// tools/ui-tests/face.test.mjs.
(()=>{
const AGENT={route:"Router",critique:"Critic",test:"Tester",review:"Reviewer",repro:"Reproducer",merge:"Merger"};
// The chip row's order; anything else keeps its own order after these.
const CHIP_ORDER=["doing","label","wait","route","critique","test","review","repro","merge","checksize"];
const META_ORDER=["repo","issue","pr","logins","stack","acct","ago"];

// What a node in the card's .tags line is.
function chipKind(el){
  if(!el||!el.classList)return"other";
  const cl=el.classList,ds=el.dataset||{};
  if(ds.meta)return"meta";
  if(ds.doing!==undefined)return"doing"; // what the card is doing (doing.js, card #150), first
  if(cl.contains("chg-tag"))return"chg";
  if(cl.contains("lbl"))return"label";
  if(cl.contains("wt-chip"))return"wait";
  if(ds.route!==undefined)return"route";
  if(ds.check&&CHIP_ORDER.includes(ds.check))return ds.check;
  if(el.hasAttribute&&el.hasAttribute("data-check-size"))return"checksize";
  return"other";
}
const chipRank=k=>{const i=CHIP_ORDER.indexOf(k);return i<0?CHIP_ORDER.length:i};
// The chips in the row's order; a stable sort, so equal kinds keep the order they were drawn in.
function orderChips(items,kindOf){
  kindOf=kindOf||chipKind;
  return items.map((x,i)=>({x,i,r:chipRank(kindOf(x))})).sort((a,b)=>a.r-b.r||a.i-b.i).map(o=>o.x);
}

// "Router: small · Sonnet low", "Router: split into 3".
function modelWord(m){m=String(m||"");const f=/(opus|sonnet|haiku|fable)/i.exec(m);return f?f[1][0].toUpperCase()+f[1].slice(1).toLowerCase():m}
function routeText(r){
  if(!r||typeof r!=="object")return"";
  const kids=Array.isArray(r.children)?r.children.filter(x=>typeof x==="string").length:0;
  const size=r.size==="split"&&kids?`split into ${kids}`:String(r.size||"");
  const how=[modelWord(r.model),r.effort?String(r.effort):""].filter(Boolean).join(" ");
  return`Router: ${size}${how?` · ${how}`:""}`;
}
// "Critic: 2 findings" when the critique's detail gives a count, else the chip's own words.
function critiqueText(t,fallback){
  const m=/(\d+)\s+(?:findings?|issues?|problems?)\b/i.exec(String(t&&t.detail||""));
  if(!m)return fallback;const n=Number(m[1]);return`Critic: ${n} finding${n===1?"":"s"}`;
}
// The newest agent text among the agents whose chip is on the card: {key, agent, text} or null.
function agentLine(c,shown){
  const src={route:c&&c.route,critique:c&&c.critique,test:c&&c.test,review:c&&c.review,repro:c&&c.repro,merge:c&&c.merge};
  let best=null;
  for(const k of Object.keys(AGENT)){
    if(shown&&!shown.has(k))continue;
    const o=src[k];if(!o||typeof o!=="object")continue;
    const text=String((k==="route"?o.reason:o.detail)||"").trim();if(!text)continue;
    const at=Date.parse(o.at||"")||0;
    if(!best||at>best.at)best={key:k,agent:AGENT[k],text,at};
  }
  return best;
}
// "22m", "1h 12m", "3h 05m", "2d 4h".
function fmtDur(ms){
  ms=Number(ms);if(!Number.isFinite(ms)||ms<0)return"";
  const m=Math.floor(ms/60000);if(m<1)return"<1m";if(m<60)return m+"m";
  const h=Math.floor(m/60);if(h<24)return`${h}h ${String(m%60).padStart(2,"0")}m`;
  return`${Math.floor(h/24)}d ${h%24}h`;
}
// "In progress 1h 12m" from the card's columnSince (the app's, card #120); "" when it isn't known.
function timeIn(c,now){
  if(!c||c.columnSince==null||isArch(c))return"";
  const t=typeof c.columnSince==="number"?c.columnSince:Date.parse(c.columnSince);
  if(!Number.isFinite(t)||t<=0)return"";
  const d=fmtDur((now||Date.now())-t);return d?`${colOf(c)} ${d}`:"";
}
const hhmm=t=>{try{return new Date(t).toLocaleTimeString("en-GB",{hour:"2-digit",minute:"2-digit"})}catch(e){return""}};
const when=v=>typeof v==="number"?v:Date.parse(v||"")||0;
// The stuck box's words after "May be stuck": the reason, how long, and what the app did, with times when it has them.
function stuckLine(s,now){
  if(!s)return"";
  const parts=[];const r=String(s.reason||"").trim().replace(/[.\s]+$/,"");
  const since=when(s.since);
  parts.push((r||"no reason given")+(since?`, ${fmtDur((now||Date.now())-since)}`:""));
  const nAt=when(s.nudgedAt||s.nudged_at),tAt=when(s.notifyAt||s.notifiedAt||s.notify_at);
  const did=[];
  if(nAt)did.push(`Nudged ${hhmm(nAt)}`);else if(s.nudged)did.push(s.typed===false?"Couldn't nudge it":"Nudged");
  if(s.notified)did.push(tAt?`told you at ${hhmm(tAt)}`:"told you");else if(tAt)did.push(`telling you at ${hhmm(tAt)}`);
  return parts.join("")+"."+(did.length?" "+did.join("; ")+".":"");
}
// The shots for a Human test card's face: the latest card-tester run's first two (before, then after).
function shotsOf(c){
  if(!c||isArch(c)||colOf(c)!=="Human test")return[];
  const run=Array.isArray(c.evidence)?c.evidence.find(r=>r&&typeof r==="object"):null;if(!run)return[];
  return(Array.isArray(run.shots)?run.shots:[]).filter(s=>s&&typeof s.path==="string"&&s.path&&!SECRET.test(s.path))
    .sort((a,b)=>(Number(a.n)||0)-(Number(b.n)||0)).slice(0,2).map(s=>({path:s.path,at:run.at||"",kind:s.kind||"shot"}));
}

// One decoded <img> per shot, kept across renders, so a redraw moves it instead of decoding it again.
// Its picture comes from panel.js's shot cache, or one image_read through loadShot.
const THUMBS=new Map(),THUMB_MAX=40;
function thumbNode(sh){
  const k=sh.path+"|"+sh.at;let img=THUMBS.get(k);
  if(img){THUMBS.delete(k);THUMBS.set(k,img);return img}
  img=document.createElement("img");img.className="face-thumb";img.setAttribute("loading","lazy");img.setAttribute("decoding","async");
  img.alt=`${sh.kind==="before"?"Before":sh.kind==="after"?"After":"Shot"}: ${String(sh.path).split(/[\\/]/).pop()}`;img.draggable=false;
  if(sh.kind)img.dataset.kind=sh.kind;
  THUMBS.set(k,img);
  for(const [ok,ov] of THUMBS){if(THUMBS.size<=THUMB_MAX)break;if(!ov.isConnected)THUMBS.delete(ok)}
  const got=typeof shotCache!=="undefined"&&shotCache.get?shotCache.get(sh.path+"|"+(sh.at||"")):null;
  if(typeof got==="string")img.src=got;
  else if(typeof loadShot==="function")loadShot(sh.path,sh.at).then(u=>{img.src=u},()=>{img.classList.add("gone")});
  return img;
}

function el(tag,cls,text){const e=document.createElement(tag);if(cls)e.className=cls;if(text!=null)e.textContent=text;return e}

// Lays out one card's face. Moves nodes only; never drops one.
function arrange(b,c){
  if(!b||!c||b.classList.contains("stk"))return; // a stack card keeps its layout (face.css styles it)
  b.classList.add("face");
  const one=s=>b.querySelector(":scope > "+s);
  const t=one(".t"),box=one(".wbox"),line=one(".wait"),dw=one(".dw-line"),steps=one(".steps"),s=one(".s");
  let tags=one(".tags");
  if(!tags){tags=el("span","tags");b.appendChild(tags)}
  const known=new Set([t,box,line,dw,steps,s,tags].filter(Boolean));
  const kids=[...tags.children];
  const meta=kids.filter(x=>chipKind(x)==="meta").sort((a,b2)=>META_ORDER.indexOf(a.dataset.meta)-META_ORDER.indexOf(b2.dataset.meta));
  const chg=kids.filter(x=>chipKind(x)==="chg");
  let chips=orderChips(kids.filter(x=>{const k=chipKind(x);return k!=="meta"&&k!=="chg"}));
  const now=Date.now();

  // The chips' words on the face.
  for(const x of chips){const k=chipKind(x);
    if(k==="route"){const r=typeof routeOf==="function"?routeOf(c):null;const tx=routeText(r);if(tx&&x.textContent!==tx)x.textContent=tx}
    else if(k==="critique"){const tx=critiqueText(c.critique,x.textContent);if(x.textContent!==tx)x.textContent=tx}
    else if(x.classList.contains("auto")){const tx=c.autoTo==="Deploy complete"?"Goes on by itself → Deploy":"Goes on by itself";if(x.textContent!==tx)x.textContent=tx}}

  // A stuck card: its "May be stuck" chip becomes the label of a box that says why.
  let stuck=null;const sc=chips.find(x=>x.classList.contains("stuck"));
  const st=typeof stuckOf==="function"?stuckOf(c):null;
  if(sc&&st){stuck=el("div","face-stuck");stuck.appendChild(sc);stuck.appendChild(el("span","fs-t"," · "+stuckLine(st,now)));
    chips=chips.filter(x=>x!==sc);b.classList.add("stuck")}

  // What changed: the figures and paths changes.js drew, on their own line.
  let chgRow=null;if(chg.length){chgRow=el("div","face-chg");for(const x of chg)chgRow.appendChild(x)}
  // The muted row at the bottom.
  let metaRow=null;if(meta.length){metaRow=el("div","face-meta");for(const x of meta)metaRow.appendChild(x)}
  // The chip row, then the time in the column at its end.
  for(const x of chips)tags.appendChild(x);
  const ti=timeIn(c,now);
  if(ti){const sp=el("span","timein",ti);sp.title=`In ${colOf(c)} since ${c.columnSince}`;tags.appendChild(sp)}
  tags.classList.add("face-chips");tags.hidden=!tags.children.length;

  // The newest agent line, for the agents whose chip is on the face; else the summary.
  const shown=new Set(chips.map(chipKind).filter(k=>Object.hasOwn(AGENT,k)));
  const ag=agentLine(c,shown);let agent=null;
  if(ag){agent=el("div","face-agent");agent.dataset.agent=ag.key;
    const who=el("b","fa-who",ag.agent+": ");agent.appendChild(who);
    const sp=el("span","fa-t");sp.innerHTML=safe(ag.text);agent.appendChild(sp);
    if(!SECRET.test(ag.text))agent.title=`${ag.agent}: ${ag.text}`;
    if(s)s.hidden=true}
  else if(s)s.hidden=false;

  // Human test: the latest test's shots, small.
  let thumbs=null;const shots=shotsOf(c);
  if(shots.length){thumbs=el("div","face-thumbs");thumbs.setAttribute("aria-hidden","true");for(const sh of shots)thumbs.appendChild(thumbNode(sh))}

  const others=[...b.children].filter(x=>!known.has(x));
  const order=[t,stuck,box,line,dw,steps,...others,thumbs,tags,agent,s,chgRow,metaRow].filter(Boolean);
  for(const x of order)b.appendChild(x);
}

window.Face={chipKind,orderChips,routeText,critiqueText,agentLine,fmtDur,timeIn,stuckLine,shotsOf,arrange,CHIP_ORDER,META_ORDER,AGENT};
if(typeof boardHooks!=="undefined"&&boardHooks&&Array.isArray(boardHooks.final))boardHooks.final.push(arrange);
})();
