// Time to check (card #103): how long a card in Human test takes Hugh to check. The app writes
// `checkSize` on the card ({class: "quick"|"real", why, reasons, diff, login, seed, at}); this
// shows it as a chip on the card, a row in the card's Details, two search terms (is:quick,
// is:needs-testing) and a pill for the 30-second looks (filterbar.js). Uses board.js's helpers (esc,
// safe, SECRET, colOf, isArch, ago, invoke, render, renderDetail, matches, cardEl, cards, panel,
// say, $). Every top-level name starts with checkSize; the rest is private, in the block below.
const checkSizeCol="Human test";

// The card's checkSize, or null when it is missing or malformed.
function checkSizeOf(c){
  const s=c&&c.checkSize;
  if(!s||typeof s!=="object"||Array.isArray(s))return null;
  if(s.class!=="quick"&&s.class!=="real")return null;
  return s;
}
// A card the size counts for: in Human test, not archived, and not a stack's child (its lead's
// check covers the stack's branch, and the app never works one out for a child).
const checkSizeIn=c=>!!c&&colOf(c)===checkSizeCol&&!isArch(c)&&(c.stackOf==null||c.stackOf==="");
// The chip's words for a card, or "" (card #107 reads this for its "To test" line).
function checkSizeText(c){const s=checkSizeIn(c)?checkSizeOf(c):null;return s?(s.class==="quick"?"30-second look":"Needs real testing"):""}

// The search box: "is:quick" and "is:needs-testing". Any other query is not ours (null).
function checkSizeMatch(c,query){
  const k=String(query??"").trim().toLowerCase();
  const want=k==="is:quick"?"quick":k==="is:needs-testing"?"real":null;
  if(!want)return null;
  const s=checkSizeIn(c)?checkSizeOf(c):null;
  return !!s&&s.class===want;
}

{
  const LABEL={quick:"30-second look",real:"Needs real testing"};
  const MAX_PATHS=8;
  const num=v=>typeof v==="number"&&Number.isFinite(v)&&v>=0?Math.round(v):0;
  const str=v=>typeof v==="string"?v:typeof v==="number"&&Number.isFinite(v)?String(v):"";
  // Plain text for a title attribute: a line that looks like a secret is replaced.
  const plain=v=>{const s=str(v);return SECRET.test(s)?"hidden: looks like a secret":s};
  const reasonsOf=s=>Array.isArray(s.reasons)?s.reasons.map(str).filter(x=>x.trim()):[];
  // The reasons after the headline: the first reason is often the "why" itself, so it isn't shown twice.
  const moreReasons=s=>{const w=str(s.why).trim();return reasonsOf(s).filter(r=>r.trim()!==w)};
  const diffOf=s=>s.diff&&typeof s.diff==="object"&&!Array.isArray(s.diff)?s.diff:null;
  const pathsOf=d=>Array.isArray(d.paths)?d.paths.filter(p=>p&&typeof p==="object"&&str(p.path)):[];
  const totals=d=>{const f=num(d.files);return`${f} file${f===1?"":"s"}, +${num(d.added)} −${num(d.removed)}`};
  // "N files, +A −R, from <from>": plain text, for the chip's title.
  const diffLine=s=>{const d=diffOf(s);if(!d)return"";const from=str(d.from);return totals(d)+(from?`, from ${plain(from)}`:"")};

  window.checkSizeTitle=function checkSizeTitle(s){
    const lines=[plain(s.why),...moreReasons(s).map(plain),diffLine(s)].map(x=>x.trim()).filter(Boolean);
    return[...new Set(lines)].join("\n");
  };
  // The chip on a card: only in Human test, not archived, with a valid checkSize.
  window.checkSizeChip=function checkSizeChip(c){
    const s=checkSizeIn(c)?checkSizeOf(c):null;if(!s)return"";
    const t=checkSizeTitle(s);
    return`<span class="tag chip check-size-${s.class}" data-check-size${t?` title="${esc(t)}"`:""}>${esc(LABEL[s.class])}</span>`;
  };
  // Puts the chip in a card element's tags, before the time-ago span (the last child span).
  window.checkSizeDecorate=function checkSizeDecorate(el,c){
    if(!el||!el.querySelector)return;
    const tags=el.querySelector(":scope > .tags");
    // A stack's lead draws its own card (stacks.js): the chip goes on its branch-and-state line,
    // since the lead's check covers the whole stack's branch.
    const sub=tags?null:el.querySelector(":scope > .stk-sub");
    if(!tags&&!sub)return;
    (tags||sub).querySelectorAll(":scope > [data-check-size]").forEach(x=>x.remove());
    const html=checkSizeChip(c);if(!html)return;
    const t=document.createElement("template");t.innerHTML=html;const chip=t.content.firstElementChild;if(!chip)return;
    if(sub){sub.append(" ",chip);return}
    const last=tags.lastElementChild;
    if(last&&last.tagName==="SPAN")tags.insertBefore(chip,last);else tags.appendChild(chip);
  };

  // The Details row's content.
  const again=()=>typeof canWrite!=="undefined"&&!canWrite?"":`<div class="cs-act"><button class="btn ghost" type="button" data-check-size-again>Work it out again</button></div>`;
  const rowHtml=c=>{
    const s=checkSizeOf(c);
    if(!s)return`<span class="meta">Working it out…</span>${c.checkSize!=null?again():""}`;
    const reasons=moreReasons(s),d=diffOf(s),at=Date.parse(str(s.at))||0;
    let diff="";
    if(d){
      const from=str(d.from),paths=pathsOf(d),shown=paths.slice(0,MAX_PATHS),more=paths.length-shown.length+num(d.more);
      diff=`<div class="cs-diff">${from?`From ${safe(from)} · `:""}${esc(totals(d))}</div>`
        +(shown.length?`<ul class="cs-paths">${shown.map(p=>{const k=str(p.kind);
          return`<li${k?` class="k-${esc(k.replace(/[^a-z]/gi,""))}" title="${esc(k)}"`:""}><span class="p">${safe(p.path)}</span> <span class="a">+${num(p.added)}</span> <span class="r">−${num(p.removed)}</span></li>`}).join("")}`
          +`${more?`<li class="more">and ${more} more</li>`:""}</ul>`:more?`<div class="cs-more">and ${more} more</div>`:"");
    }
    return`<div class="cs-head">${checkSizeChip(c)}${at?`<span class="meta" title="${esc(str(s.at))}">${esc(ago(at))}</span>`:""}</div>`
      +(str(s.why)?`<div class="cs-why">${safe(s.why)}</div>`:"")
      +(reasons.length?`<ul class="cs-reasons">${reasons.map(r=>`<li>${safe(r)}</li>`).join("")}</ul>`:"")
      +diff+again();
  };

  // The same content as a box, for card #107's details hook (HTML, after the Ship and Planning boxes).
  window.checkSizeBox=function checkSizeBox(c){
    if(!c||!checkSizeIn(c))return"";
    return`<div class="shipbox cs-box" data-check-size-box data-card="${esc(c.id)}"><h3>Time to check</h3>${rowHtml(c)}</div>`;
  };
  // "Work it out again" in the box: one listener for every redraw.
  document.addEventListener("click",async e=>{const b=e.target.closest("[data-check-size-box] [data-check-size-again]");if(!b)return;
    const id=b.closest("[data-check-size-box]").dataset.card;b.disabled=true;
    try{await invoke("check_size_again",{id});say("Working it out again…","ok")}
    catch(err){b.disabled=false;say("Couldn't work it out again: "+(typeof errText==="function"?errText(err):String(err)),"err")}});

  // The "Time to check" row in the open card's Details (the <dl> with "Subagent test"). renderDetail
  // runs often: a row already there is replaced, or kept when it would come out the same.
  let lastHtml=null;
  window.checkSizeRow=function checkSizeRow(d,c){
    if(typeof panel==="undefined"||!panel)return;
    if(!(d instanceof Element))d=panel.el&&panel.el.querySelector("#p-det");
    if(!d)return;
    if(!c||typeof c!=="object"||c.id!==panel.id)c=cards.find(x=>x.id===panel.id);
    const old=[...d.querySelectorAll("[data-check-size-row]")];
    if(!c||!checkSizeIn(c)){old.forEach(x=>x.remove());lastHtml=null;return}
    const dls=[...d.querySelectorAll("dl")];
    const dl=dls.find(x=>[...x.querySelectorAll(":scope > dt")].some(t=>t.textContent.trim()==="Subagent test"))
      ||dls.find(x=>{const h=x.previousElementSibling;return h&&h.tagName==="H3"&&h.textContent.trim()==="Details"});
    if(!dl){old.forEach(x=>x.remove());return}
    const html=rowHtml(c);
    const dd=old.find(x=>x.tagName==="DD");
    if(old.length===2&&dd&&dd.parentNode===dl&&dd.dataset.card===c.id&&lastHtml===html&&dl.lastElementChild===dd)return;
    old.forEach(x=>x.remove());
    const dt=document.createElement("dt");dt.dataset.checkSizeRow="";dt.textContent="Time to check";
    const nd=document.createElement("dd");nd.dataset.checkSizeRow="";nd.dataset.card=c.id;nd.className="cs-dd";nd.innerHTML=html;
    dl.append(dt,nd);lastHtml=html;
    const b=nd.querySelector("[data-check-size-again]");
    if(b){const id=c.id;b.onclick=async()=>{b.disabled=true;
      try{await invoke("check_size_again",{id});say("Working it out again…","ok")}
      catch(e){b.disabled=false;say("Couldn't work it out again: "+(typeof errText==="function"?errText(e):String(e)),"err")}}}
  };

  // The "30-second looks · N" pill (card #120 moved it from the header into filterbar.js's row; same
  // id, title and aria-pressed): it turns the is:quick filter on and off. Queued, since filterbar.js
  // loads after this file. checkSizeButton stays for older callers: it redraws the row.
  const QUICK="is:quick";
  (window.kbPills=window.kbPills||[]).push({key:"quick",id:"checkSizeBtn",label:"30-second looks",tone:"",term:QUICK,order:40,
    title:"Show only the cards in Human test that need just a 30-second look",
    count:list=>list.filter(c=>checkSizeIn(c)&&!(typeof isSystemCard==="function"&&isSystemCard(c))&&!(typeof stackHidden==="function"&&stackHidden(c))&&checkSizeOf(c)?.class==="quick").length});
  window.checkSizeButton=function checkSizeButton(){if(typeof fbDraw==="function")fbDraw()};

  // Our stylesheet.
  if(!document.querySelector('link[href="check-size.css"]')){
    const l=document.createElement("link");l.rel="stylesheet";l.href="check-size.css";document.head.appendChild(l);
  }

  // The shared hook list (card #107) when board.js has one; otherwise wrap board.js's functions,
  // as it wraps render itself. Decided once, here. Each wrap reassigns the global name, so every
  // caller that looks it up at run time (render, archiveCol, stacks.js stackMatches) gets ours.
  const H=typeof boardHooks!=="undefined"?boardHooks:window.boardHooks;
  if(H&&Array.isArray(H.filters)&&Array.isArray(H.decorate)){
    H.filters.push((card,query)=>checkSizeMatch(card,query));
    H.decorate.push((el,card)=>checkSizeDecorate(el,card));
  }else{
    const oldMatches=matches;
    matches=function(c){if(q){const m=checkSizeMatch(c,q);if(m!==null)return m}return oldMatches.apply(this,arguments)};
    const oldCardEl=cardEl;
    cardEl=function(c){const el=oldCardEl.apply(this,arguments);try{checkSizeDecorate(el,c)}catch(e){}return el};
  }
  if(H&&Array.isArray(H.details)){
    H.details.push(c=>checkSizeBox(c));
  }else{
    const oldDetail=renderDetail;
    renderDetail=function(){const r=oldDetail.apply(this,arguments);try{checkSizeRow()}catch(e){}return r};
  }
}
