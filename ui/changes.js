// What changed on each card (card #108): a tag on the card ("3 files +48 −6", "1 handed over") and a
// "What changed" box in its panel (files, commits, what isn't committed yet, the files the session
// handed over). The figures come from the Rust side: card_changes_brief for the board, card_changes
// for one card's panel, changes_open to open a handed-over file or show it in its folder.
// It loads last and hooks in by itself (boardHooks, addCardPart, or by wrapping cardEl and
// renderDetail), so board.js and panel.js need no change. Uses board.js's helpers (esc, safe, ago,
// colOf, isArch, cardNum, cards, panel, invoke, say, render). Any error leaves the old figures.
// The pure helpers come first, so a test can load them with stub globals.

const CHG_MINUS="−";               // a real minus sign
const CHG_SKIP=["Backlog","Planning"];  // no figures yet in these columns
const CHG_LIST=15;                      // files shown before "Show all"
const CHG_COMMITS=3;                    // commits shown open; more fold into a <details>
const CHG_BRIEF_MS=60000;               // a card's brief at most once a minute
const CHG_FULL_MS=30000;                // the panel's full figures are fresh for 30 s
const CHG_SESSION_MS=3000;              // a session's events settle for 3 s first

const chgN=v=>{const n=Number(v);return Number.isFinite(n)?n:0};
const chgFmt=v=>chgN(v).toLocaleString("en-GB");
const chgFiles=n=>`${chgFmt(n)} file${chgN(n)===1?"":"s"}`;
const chgFigText=(a,r)=>`+${chgFmt(a)} ${CHG_MINUS}${chgFmt(r)}`;
const chgFigHtml=(a,r)=>`<span class="chg-add">+${chgFmt(a)}</span> <span class="chg-del">${CHG_MINUS}${chgFmt(r)}</span>`;
// The card face: "3 files +48 −6"; its tooltip: "3 files changed, +48 −6 (saved when it entered Human test)".
const chgFaceText=b=>`${chgFiles(b.files)} ${chgFigText(b.added,b.removed)}`;
const chgFaceTitle=b=>`${chgFiles(b.files)} changed, ${chgFigText(b.added,b.removed)}${b.saved?" (saved when it entered Human test)":""}`;
const chgHanded=n=>`${chgFmt(n)} handed over`;
// A card can have figures when it has a worktree, is in a stack, or had them saved on entering Human test;
// a card with handed-over files shows them even with no checkout.
const chgHasSource=c=>!!(c&&(c.worktreePath||c.stackOf||c.changesSaved||(Array.isArray(c.changesOutputs)&&c.changesOutputs.length)));
// A Done card's tag comes from its own fields (the figures saved when it entered Human test, and
// its handed-over files), so the board never runs git for it.
const chgDone=c=>!!c&&colOf(c)==="Done";
function chgLocalBrief(c){
  const t=c&&c.changesSaved&&typeof c.changesSaved==="object"&&c.changesSaved.totals;
  const outputs=Array.isArray(c&&c.changesOutputs)?c.changesOutputs.length:0;
  const ok=t&&typeof t==="object";
  if(!ok&&!outputs)return null;
  return{files:ok?chgN(t.files):0,added:ok?chgN(t.added):0,removed:ok?chgN(t.removed):0,outputs,saved:!!ok};
}
// The board asks for briefs only for these: live cards past Planning, but not Done.
const chgEligible=c=>!!c&&!isArch(c)&&!CHG_SKIP.includes(colOf(c))&&!chgDone(c)&&chgHasSource(c);
// The panel shows the box for archived cards too (their saved figures stay).
const chgPanelEligible=c=>!!c&&!CHG_SKIP.includes(colOf(c))&&chgHasSource(c);
const chgAgo=s=>{const t=Date.parse(s||"");return t?ago(t):""};
function chgSize(n){
  if(n===null||n===undefined||!Number.isFinite(Number(n)))return"";
  n=Number(n);
  if(n<1024*1024)return`${Math.max(n>0?1:0,Math.round(n/1024)).toLocaleString("en-GB")} KB`;
  return`${(n/1024/1024).toFixed(1)} MB`;
}

// The tags on the card face, or "" (archived, Backlog, Planning, or nothing to show).
function chgTagsHtml(c,b){
  if(!c||!b||typeof b!=="object"||isArch(c)||CHG_SKIP.includes(colOf(c)))return"";
  let h="";
  // Up to 3 paths after the figures (card #120: the brief's `paths`, most changed first), when the app sends them.
  const ps=Array.isArray(b.paths)?b.paths.filter(p=>typeof p==="string"&&p).slice(0,3):[];
  const pth=ps.length?`<span class="chg-paths">${ps.map(p=>` · <span class="chg-path">${safe(p)}</span>`).join("")}</span>`:"";
  if(chgN(b.files)>0)h+=`<span class="tag chg-tag" title="${esc(chgFaceTitle(b)+(ps.length?"\n"+ps.filter(p=>!SECRET.test(p)).join("\n"):""))}">${esc(chgFiles(b.files))} ${chgFigHtml(b.added,b.removed)}${pth}</span>`;
  if(chgN(b.outputs)>0)h+=`<span class="tag chg-tag chg-out" title="${esc(`The session handed over ${chgFiles(b.outputs)}`)}">${esc(chgHanded(b.outputs))}</span>`;
  return h;
}

// One file row: the path, a status word (added, deleted, renamed from ...), and +N −N or "binary".
function chgFileRow(f){
  if(!f||typeof f!=="object")return"";
  const st=f.status==="A"?"added":f.status==="D"?"deleted":f.status==="R"?(f.old?`renamed from ${safe(f.old)}`:"renamed"):"";
  const figs=f.added===null||f.added===undefined||f.removed===null||f.removed===undefined?`<span class="chg-bin">binary</span>`:chgFigHtml(f.added,f.removed);
  return`<li class="chg-f"><span class="chg-p">${safe(f.path)}${st?` <span class="chg-st chg-st-${esc(f.status)}">${st}</span>`:""}</span><span class="chg-n">${figs}</span></li>`;
}
// A list of files: the first CHG_LIST, then "Show all <n>" (unless ui.showAll), then "and <more> more".
function chgFileList(c,files,more,showAll){
  const fs=Array.isArray(files)?files:[];
  const shown=showAll?fs:fs.slice(0,CHG_LIST);
  let h=shown.length?`<ul class="chg-files">${shown.map(chgFileRow).join("")}</ul>`:"";
  if(!showAll&&fs.length>CHG_LIST)h+=`<button class="chg-all" type="button" data-chg-act="all" data-chg-card="${esc(c.id)}">Show all ${chgFmt(fs.length)}</button>`;
  if(chgN(more)>0)h+=`<div class="chg-more">and ${chgFmt(more)} more</div>`;
  return h;
}
// "3 files changed, +48 −6".
const chgTotals=t=>t&&typeof t==="object"&&chgN(t.files)>0?`${esc(chgFiles(t.files))} changed, ${chgFigHtml(t.added,t.removed)}`:"";
// Where the figures come from.
function chgSource(c,d){
  const s=d.source;
  if(s==="branch"){const sha=String(d.base||"").slice(0,7);const name=d.baseName||sha||"?";
    return`Branch <code>${safe(d.branch||"?")}</code> against <code${sha&&d.baseName?` title="${esc(`at ${sha}`)}"`:""}>${safe(name)}</code>`}
  if(s==="commits"){const n=chgN(d.commitCount||(Array.isArray(d.commits)?d.commits.length:0));const num=typeof cardNum==="function"?cardNum(c):c.number;
    return`${esc(chgFmt(n))} commit${n===1?"":"s"} that name card #${esc(num||"?")}${d.branch?` on <code>${safe(d.branch)}</code>`:""}`}
  if(s==="saved"){const a=chgAgo(d.savedAt);return`Saved when it entered Human test${a?`, <span title="${esc(d.savedAt)}">${esc(a)}</span>`:""}`}
  return safe(d.why||"No changes yet");
}
function chgCommits(c,d,open){
  const cs=Array.isArray(d.commits)?d.commits.filter(x=>x&&typeof x==="object"):[];
  if(!cs.length)return"";
  const total=Math.max(chgN(d.commitCount),cs.length);
  const list=`<ul class="chg-commits-l">${cs.map(x=>`<li><code class="chg-sha">${esc(String(x.sha||"").slice(0,7))}</code><span class="chg-subj">${safe(x.subject||"")}</span></li>`).join("")}</ul>${total>cs.length?`<div class="chg-more">and ${chgFmt(total-cs.length)} more</div>`:""}`;
  const head=`${chgFmt(total)} commit${total===1?"":"s"}`;
  if(cs.length>CHG_COMMITS)return`<details class="chg-commits" data-chg-card="${esc(c.id)}"${open?" open":""}><summary>${head}</summary>${list}</details>`;
  return`<div class="chg-sec"><div class="chg-h">${head}</div>${list}</div>`;
}
function chgUncommitted(c,u,showAll){
  if(!u||typeof u!=="object")return"";
  const files=Array.isArray(u.files)?u.files:[],un=Array.isArray(u.untracked)?u.untracked.filter(x=>typeof x==="string"):[];
  if(!files.length&&!un.length)return"";
  const tot=chgTotals(u.totals);
  return`<div class="chg-sec chg-unc"><div class="chg-h">Not committed yet</div>${tot?`<div class="chg-tot">${tot}</div>`:""}${chgFileList(c,files,u.more,showAll)}${un.length?`<ul class="chg-files">${un.map(n=>`<li class="chg-f"><span class="chg-p">${safe(n)} <span class="chg-st chg-st-new">new</span></span><span class="chg-n"></span></li>`).join("")}</ul>`:""}</div>`;
}
function chgOutputs(c,d){
  const os=Array.isArray(d.outputs)?d.outputs:[];
  if(!os.length)return"";
  const rows=os.map((o,i)=>{if(!o||typeof o!=="object")return"";
    const meta=[o.label?safe(o.name||""):"",o.exists?esc(chgSize(o.size)):"",o.exists&&o.modified?`<span title="${esc(o.modified)}">${esc(chgAgo(o.modified))}</span>`:""].filter(Boolean).join(" · ");
    const btns=o.exists?`${o.openable?`<button class="btn ghost chg-btn" type="button" data-chg-act="open" data-chg-card="${esc(c.id)}" data-chg-i="${i}" data-chg-path="${esc(o.path)}">Open</button>`:""}<button class="btn ghost chg-btn" type="button" data-chg-act="reveal" data-chg-card="${esc(c.id)}" data-chg-i="${i}" data-chg-path="${esc(o.path)}">Show in folder</button>`:`<span class="chg-miss">Not found</span>`;
    return`<li class="chg-o"><div class="chg-ol">${safe(o.label||o.name||o.path||"File")}</div>${meta?`<div class="chg-om">${meta}</div>`:""}<div class="chg-ob">${btns}</div></li>`}).join("");
  return`<div class="chg-sec chg-outs"><div class="chg-h">Handed over</div><ul class="chg-outl">${rows}</ul></div>`;
}
// The whole "What changed" box for card c from its card_changes result d. ui: {showAll, commitsOpen}.
// Classes only (no ids): another box (card #102) draws it inside its own.
function chgBoxHtml(c,d,ui){
  if(!c||!d||typeof d!=="object")return"";
  ui=ui||{};
  const tot=chgTotals(d.totals);
  const none=d.source==="none"||!d.source;
  const totLine=tot?`<div class="chg-tot">${tot}</div>`:!none?`<div class="chg-tot">No files changed</div>`:"";
  const why=!none&&d.why?`<div class="chg-why">${safe(d.why)}</div>`:"";
  return`<div class="chg" data-chg-card="${esc(c.id)}"><h3>What changed</h3>${totLine}<div class="chg-src">${chgSource(c,d)}</div>${why}${chgFileList(c,d.files,d.more,!!ui.showAll)}${chgCommits(c,d,!!ui.commitsOpen)}${chgUncommitted(c,d.uncommitted,!!ui.showAll)}${chgOutputs(c,d)}</div>`;
}

// ---- State: briefs and full figures, by card id ----
const CHG={briefs:{},briefAt:{},briefBusy:new Set(),briefAgain:new Set(),
  full:{},fullAt:{},fullTry:{},fullBusy:new Set(),fullAgain:new Set(),
  showAll:new Map(),commitsOpen:new Map(),sessT:{}};
const chgPanel=()=>typeof panel!=="undefined"&&panel?panel:null;
// "Show all" and an open commit list last while the same panel stays open.
const chgUi=id=>{const p=chgPanel();return{showAll:CHG.showAll.has(id)&&CHG.showAll.get(id)===p,commitsOpen:CHG.commitsOpen.has(id)&&CHG.commitsOpen.get(id)===p}};
const chgCards=()=>typeof cards!=="undefined"&&Array.isArray(cards)?cards:[];
const chgRender=()=>{try{if(typeof render==="function")render()}catch(e){}};
const chgErr=e=>e&&typeof e==="object"?(e.message||JSON.stringify(e)):String(e||"unknown error");

// The board's briefs: one card_changes_brief call for every eligible card not asked in the last
// minute (or named in force). Renders only when something changed.
async function chgBriefs(force){
  try{
    const now=Date.now(),ids=[];
    for(const c of chgCards()){
      if(!chgEligible(c))continue;
      const forced=force&&force.has(c.id);
      if(CHG.briefBusy.has(c.id)){if(forced)CHG.briefAgain.add(c.id);continue}
      if(forced||now-(CHG.briefAt[c.id]||0)>=CHG_BRIEF_MS)ids.push(c.id);
    }
    if(!ids.length)return;
    for(const id of ids){CHG.briefAt[id]=now;CHG.briefBusy.add(id)}
    let res=null;
    try{res=await invoke("card_changes_brief",{ids})}catch(e){res=null}
    finally{for(const id of ids)CHG.briefBusy.delete(id)}
    if(res&&typeof res==="object"){
      let changed=false;
      for(const id of ids){
        const nb=res[id]&&typeof res[id]==="object"?res[id]:null,ob=CHG.briefs[id]||null;
        if(JSON.stringify(nb)!==JSON.stringify(ob)){changed=true;if(nb)CHG.briefs[id]=nb;else delete CHG.briefs[id]}
      }
      if(changed)chgRender();
    }
    const again=ids.filter(id=>CHG.briefAgain.delete(id));
    if(again.length)setTimeout(()=>chgBriefs(new Set(again)),0);
  }catch(e){}
}
// One card's full figures, in the background; never two requests at once for one card.
function chgWant(id,force){
  try{
    if(CHG.fullBusy.has(id)){if(force)CHG.fullAgain.add(id);return}
    if(!force&&Date.now()-(CHG.fullTry[id]||0)<CHG_FULL_MS)return;
    CHG.fullTry[id]=Date.now();CHG.fullBusy.add(id);
    Promise.resolve().then(()=>invoke("card_changes",{id})).then(d=>{
      if(!d||typeof d!=="object")return;
      const changed=JSON.stringify(d)!==JSON.stringify(CHG.full[id]);
      CHG.full[id]=d;CHG.fullAt[id]=Date.now();
      if(changed)chgRender();
    },()=>{}).finally(()=>{CHG.fullBusy.delete(id);if(CHG.fullAgain.delete(id))chgWant(id,true)});
  }catch(e){CHG.fullBusy.delete(id)}
}

// The panel box for card c, from the cache, or "" when there is nothing to show yet. Drawn for the
// open panel's card, it asks for fresh figures when they are missing or older than 30 s.
function changesHtml(c){
  try{
    if(!c||!chgPanelEligible(c))return"";
    const p=chgPanel();
    if(p&&p.id===c.id&&(!CHG.full[c.id]||Date.now()-(CHG.fullAt[c.id]||0)>=CHG_FULL_MS))chgWant(c.id);
    const d=CHG.full[c.id];
    return d?chgBoxHtml(c,d,chgUi(c.id)):"";
  }catch(e){return""}
}

// The card face: the tags go in its .tags line, before the age (the line's last, plain span).
function chgDecorate(el,c){
  try{
    if(!el||!c||el.dataset.chgTags)return;
    const h=chgTagsHtml(c,chgDone(c)?chgLocalBrief(c):CHG.briefs[c.id]);if(!h)return;
    el.dataset.chgTags="1";
    let t=el.querySelector(".tags");
    if(!t)t=el.querySelector(".stk-sub"); // a stack's card (stacks.js) has no .tags line
    if(!t){t=document.createElement("span");t.className="tags";el.appendChild(t)}
    const last=t.lastElementChild;
    if(last&&last.tagName==="SPAN"&&!last.className)last.insertAdjacentHTML("beforebegin",h);
    else t.insertAdjacentHTML("beforeend",(t.classList.contains("tags")?"":" ")+h);
  }catch(e){}
}
// After a details render: put the box in (only when nothing else draws it), then keep one box.
function chgAfterDetail(insert,d){
  try{
    const p=chgPanel();if(!p)return;
    d=d||p.el.querySelector("#p-det");if(!d)return;
    if(insert&&!d.querySelector(".chg")){
      const c=chgCards().find(x=>x.id===p.id);const h=c?changesHtml(c):"";
      if(h){const s=d.querySelector("#save"),row=s&&s.closest(".row");
        if(row&&d.contains(row))row.insertAdjacentHTML("afterend",h);else d.insertAdjacentHTML("beforeend",h)}
    }
    // One box: a Human test card that also gets card #102's box shows it once.
    const all=d.querySelectorAll(".chg");for(let i=1;i<all.length;i++)all[i].remove();
  }catch(e){}
}
const chgWire=(c,d)=>chgAfterDetail(false,d);

function chgOpen(id,index,path,reveal){
  let p;try{p=invoke("changes_open",{id,index,path,reveal})}catch(e){p=Promise.reject(e)}
  Promise.resolve(p).then(r=>{if(typeof r==="string"&&r&&r!=="ok")throw r},null).catch(e=>{
    const t=(reveal?"Couldn't show it in its folder: ":"Couldn't open it: ")+chgErr(e);
    try{if(typeof say==="function")say(t,"err")}catch(x){}});
}

// ---- Hooking in (changes.js loads last) ----
if(typeof document!=="undefined"){
  if(!document.querySelector("link[data-chg-css]")){const l=document.createElement("link");l.rel="stylesheet";l.href="changes.css";l.dataset.chgCss="1";document.head.appendChild(l)}
  const H=typeof boardHooks==="object"&&boardHooks?boardHooks:null;
  // Card face: boardHooks.decorate (card #107), else wrap cardEl. One path only.
  if(H&&Array.isArray(H.decorate))H.decorate.push(chgDecorate);
  else if(typeof cardEl==="function"){const chgCardEl=cardEl;cardEl=function(c){const b=chgCardEl.apply(this,arguments);chgDecorate(b,c);return b}}
  // Panel: addCardPart's panel slot (card #105), else boardHooks.details, else insert it ourselves.
  let chgInsert=false;
  if(typeof addCardPart==="function"){try{addCardPart({at:40,panel:changesHtml,wire:chgWire})}catch(e){chgInsert=true}}
  else if(H&&Array.isArray(H.details))H.details.push(changesHtml);
  else chgInsert=true;
  // Every path: after each details render, keep only the first box (and insert it in the fallback).
  if(typeof renderDetail==="function"){const chgDet=renderDetail;renderDetail=function(){const r=chgDet.apply(this,arguments);chgAfterDetail(chgInsert);return r}}

  // Buttons in the box, wherever it is drawn.
  document.addEventListener("click",e=>{
    const b=e.target&&e.target.closest&&e.target.closest("[data-chg-act]");if(!b)return;
    e.preventDefault();e.stopPropagation();
    const id=b.dataset.chgCard,act=b.dataset.chgAct;if(!id)return;
    if(act==="all"){
      CHG.showAll.set(id,chgPanel());
      const box=b.closest(".chg"),c=chgCards().find(x=>x.id===id),d=CHG.full[id];
      if(box&&c&&d)box.outerHTML=chgBoxHtml(c,d,chgUi(id));else chgRender();
    }else if(act==="open"||act==="reveal")chgOpen(id,Number(b.dataset.chgI),b.dataset.chgPath||"",act==="reveal");
  });
  // An opened commit list stays open across redraws ("toggle" doesn't bubble: listen in capture).
  document.addEventListener("toggle",e=>{const t=e.target;if(!t||!t.matches||!t.matches("details.chg-commits"))return;
    const id=t.dataset.chgCard;if(!id)return;if(t.open)CHG.commitsOpen.set(id,chgPanel());else CHG.commitsOpen.delete(id)},true);

  // Board: briefs after the cards load (board.js reloads on the same event, so wait a little).
  let chgSoon=null;
  const chgBriefsSoon=()=>{clearTimeout(chgSoon);chgSoon=setTimeout(()=>chgBriefs(),500)};
  const ev=window.__TAURI__&&window.__TAURI__.event;
  if(ev){
    try{Promise.resolve(ev.listen("cards-changed",chgBriefsSoon)).catch(()=>{})}catch(e){}
    // A card's session changed (a turn ended): its brief again, and its full figures if its panel is open.
    try{Promise.resolve(ev.listen("session-changed",e=>{const id=e&&e.payload;if(typeof id!=="string"||!id)return;
      clearTimeout(CHG.sessT[id]);
      CHG.sessT[id]=setTimeout(()=>{delete CHG.sessT[id];chgBriefs(new Set([id]));
        const p=chgPanel();if(p&&p.id===id&&chgPanelEligible(chgCards().find(x=>x.id===id)))chgWant(id,true)},CHG_SESSION_MS)})).catch(()=>{})}catch(e){}
  }
  // Once about a second after start; again shortly if the board hasn't loaded its cards yet.
  {let tries=0;const first=()=>{if(typeof loaded!=="undefined"&&!loaded&&++tries<10){setTimeout(first,1000);return}chgBriefs()};setTimeout(first,1000)}
}
