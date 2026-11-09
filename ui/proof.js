// A card's proof in Human test (card #125): the screenshots and reports that show its change
// works, drawn as pictures and readable docs instead of file paths.
//
// The app (src-tauri/src/proof.rs) finds the items (the card's `proof` field, then the pictures and
// docs its hand-over names) and checks each one; proof_list gives them with a state: ok, missing,
// refused (with why) or dup (already a shot in the review strip, dupOf naming it). proof_read
// gives an ok picture as a data URL or an ok report as masked text; proof_open opens one in its
// app or its folder, only on Hugh's click.
//
// review.js calls: proofStripHtml(c) (proof pictures for the review strip), proofSectionHtml(c,
// withImages) (the Proof section), proofSteps(c) (the hand-over steps with file-name chips in
// place of paths) and proofFill(box, c) after it draws the box. What proof_list, proof_read and
// the drawn reports gave is kept while the panel shows the card, so a board redraw reads nothing
// again. Uses board.js's esc, SECRET, invoke and errText, stacks.js's stkMd and panel.js's
// openShot, only when they run. The pure helpers come first, so a test can load this file alone.

// --- Pure helpers ----------------------------------------------------------------------------

// A path's trailing punctuation, a closing ** (bold) or a question mark included.
const PF_TRAIL=/[,.;:)\]"'*?]+$/;
const pfName=p=>String(p||"").split(/[\\/]/).filter(Boolean).pop()||String(p||"");
const pfIsList=t=>/^([-*+] |\d+[.)] )/.test(t);
// The lines of `text` that are prose, by proof.rs's rule (prose_lines): fenced blocks (``` or
// ~~~) and indented code blocks (four spaces or a tab after a blank line, not in a list) are
// code. Each line: {text, code}.
function pfLines(text){
  const out=[];let fence=null,prevBlank=true,inList=false,inIndented=false;
  for(const line of String(text||"").split("\n")){
    const t=line.replace(/^\s+/,"");
    if(fence){if(t.startsWith(fence))fence=null;prevBlank=false;out.push({text:line,code:true});continue}
    if(t.startsWith("```")||t.startsWith("~~~")){fence=t.startsWith("```")?"```":"~~~";prevBlank=false;inIndented=false;out.push({text:line,code:true});continue}
    const blank=!t,indented=/^( {4}|\t)/.test(line);
    if(!blank&&indented&&(inIndented||(prevBlank&&!inList))){inIndented=true;prevBlank=false;out.push({text:line,code:true});continue}
    if(!blank){inIndented=false;if(!indented)inList=pfIsList(t)}
    prevBlank=blank;out.push({text:line,code:false});
  }
  return out;
}
const pfDrive=(s,i)=>/^[A-Za-z]:[\\/]/.test(s.slice(i,i+3))&&(i===0||!/[A-Za-z0-9]/.test(s[i-1]));
const PF_PATH=/^[^\s"'`<>|]*/;
// Whether the name at `line[i..]` carries on past i (a letter, digit, _, -, a slash, or a dot
// before more of the name), so a spelling that ends at i is only part of it.
const pfGoesOn=(line,i)=>/^([A-Za-z0-9_\\/-]|\.[A-Za-z0-9_])/.test(line.slice(i,i+2));
// Each item's spellings, longest first: the text a hand-over wrote it as.
function pfSpellings(items){
  const out=[];
  for(const it of items||[])for(const s of it.spellings||[])if(s)out.push([s,it]);
  return out.sort((a,b)=>b[0].length-a[0].length);
}
// The hand-over text with each item's spelling (outside code, link and image targets) and each
// other full path swapped for a placeholder of plain letters and digits, so stkMd's secret check
// and Markdown leave them alone. Returns {text, marks: [{token, item}|{token, path}]}.
function proofChips(text,items){
  const sp=pfSpellings(items),marks=[];
  const mark=m=>{const token=`PFMARK${marks.length}X${Math.random().toString(36).slice(2,8).replace(/[^a-z0-9]/g,"")}`;marks.push({token,...m});return token};
  const lines=pfLines(text).map(({text:line,code})=>{
    if(code)return line;
    let out="",i=0;
    while(i<line.length){
      // A code span: kept as written, unless its whole text is one item's spelling.
      if(line[i]==="`"){
        const run=/^`+/.exec(line.slice(i))[0],end=line.indexOf(run,i+run.length);
        if(end>=0){
          const inner=line.slice(i+run.length,end),hit=sp.find(([s])=>s===inner);
          out+=hit?mark({item:hit[1]}):line.slice(i,end+run.length);i=end+run.length;continue;
        }
        out+=run;i+=run.length;continue;
      }
      // A Markdown link or image target: kept as written.
      if(line.startsWith("](",i)){const end=line.indexOf(")",i+2);const z=end<0?line.length:end+1;out+=line.slice(i,z);i=z;continue}
      const prev=i?line[i-1]:"";
      if(!/[A-Za-z0-9_\\/.:-]/.test(prev)){
        // A whole spelling only: not the start of a longer name (shot.png in shot.png.bak).
        const hit=sp.find(([s])=>line.startsWith(s,i)&&!pfGoesOn(line,i+s.length));
        if(hit){out+=mark({item:hit[1]});i+=hit[0].length;continue}
      }
      if(pfDrive(line,i)){
        const raw=PF_PATH.exec(line.slice(i))[0],path=raw.replace(PF_TRAIL,"");
        if(path.length>3){out+=mark({path});i+=path.length;continue}
      }
      out+=line[i];i++;
    }
    return out;
  });
  return{text:lines.join("\n"),marks};
}
// The HTML for each placeholder: an item's chip (its file name; a click shows it), or another
// path's last part, the full path in its tooltip.
function pfSwap(html,marks,escF){
  const e=escF||(s=>String(s));
  let out=String(html||"");
  for(const m of marks){
    const rep=m.item
      ?`<button type="button" class="pf-chip" data-pf-chip="${e(m.item.path)}" title="${e(m.item.path)}">${e(m.item.name||pfName(m.item.path))}</button>`
      :`<span class="pf-short" title="${e(m.path)}">${e(pfName(m.path))}</span>`;
    out=out.split(m.token).join(rep);
  }
  return out;
}
// What a click on a link in a drawn report does: "outside" (the system browser) for http and
// https; "none" for anything else, whose href is taken away.
const pfLinkOk=href=>/^https?:\/\//i.test(String(href||"").trim())?"outside":"none";
const PF_TAGS=["p","br","hr","h1","h2","h3","h4","h5","h6","strong","em","del","code","pre","blockquote","ul","ol","li","table","thead","tbody","tr","th","td","a"];
const PF_ATTR=["href","align","start"];
// No data-* or aria-* attributes either (DOMPurify allows them by default): the board's page-wide
// click handlers act on data-* attributes, so a report could otherwise run a board action.
const PF_PURIFY={ALLOWED_TAGS:PF_TAGS,ALLOWED_ATTR:PF_ATTR,ALLOW_DATA_ATTR:false,ALLOW_ARIA_ATTR:false};

// --- Kept state ------------------------------------------------------------------------------

// For the card the panel shows: {id, version, items, pending, imgs: Map(path -> url | {err}),
// docs: Map(path -> html | {err}), open: Set(path), reading: Set(path)}.
let pfNow=null;
function pfState(c){
  if(!c)return null;
  if(!pfNow||pfNow.id!==c.id)pfNow={id:c.id,version:null,items:null,pending:false,imgs:new Map(),docs:new Map(),open:new Set(),reading:new Set()};
  const s=pfNow;
  if(s.version!==c.version&&!s.pending){
    s.pending=true;const v=c.version,id=c.id;
    invoke("proof_list",{id}).then(items=>{
      if(pfNow!==s)return;s.items=Array.isArray(items)?items:[];s.version=v;s.pending=false;
      // A read that failed is tried again once the card changes (the file may be there now).
      for(const m of [s.imgs,s.docs])for(const [k,x] of m)if(x&&typeof x==="object")m.delete(k);
      pfRedraw(id);
    },()=>{if(pfNow!==s)return;s.items=s.items||[];s.version=v;s.pending=false;pfRedraw(id)});
  }
  return s;
}
const pfItems=c=>{const s=pfState(c);return s&&s.items||[]};
// When proof_list answers, the box is drawn again, so its steps get their chips.
function pfRedraw(id){if(typeof panel!=="undefined"&&panel&&panel.id===id&&typeof htBox==="function")htBox()}

// --- HTML ------------------------------------------------------------------------------------

const pfEsc=s=>typeof esc==="function"?esc(s):String(s);
const pfCap=it=>it.label||it.name||pfName(it.path);
function pfImg(it){
  const s=pfNow,got=s&&s.imgs.get(it.path);
  const bad=got&&typeof got==="object";
  if(bad)return`<p class="pf-gone">Can't show ${pfEsc(pfCap(it))}: ${pfEsc(got.err)}</p>`;
  return`<img class="shot pf-img" data-pf="${pfEsc(it.path)}" data-cap="${pfEsc(pfCap(it))}" alt="${pfEsc(pfCap(it))}"${typeof got==="string"?` src="${pfEsc(got)}"`:""}>`;
}
// The review strip's proof pictures (review.js rvShotsHtml puts them after its own).
function proofStripFigs(c){
  return pfItems(c).filter(it=>it.kind==="image"&&it.state==="ok").map(it=>`<figure class="pf-fig"><figcaption>${pfEsc(pfCap(it))} · proof</figcaption><div class="rv-shot">${pfImg(it)}</div></figure>`);
}
function pfRow(it,withImages){
  const name=pfEsc(pfCap(it)),p=pfEsc(it.path);
  if(it.state==="missing")return`<div class="pf-row pf-miss" data-pf-row="${p}">Not there any more: ${name}</div>`;
  if(it.state==="refused")return`<div class="pf-row pf-ref" data-pf-row="${p}">Can't show ${name}: ${pfEsc(it.why||"not allowed")}</div>`;
  if(it.state!=="ok")return"";
  const btns=(open)=>`<span class="pf-btns">${open?`<button type="button" class="btn ghost sm" data-pf-open="${p}">Open</button>`:""}<button type="button" class="btn ghost sm" data-pf-reveal="${p}">Show in folder</button></span>`;
  if(it.kind==="doc"){
    const s=pfNow,open=s&&s.open.has(it.path),got=s&&s.docs.get(it.path);
    const body=got==null?`<p class="pf-wait">${open?"Reading…":""}</p>`:typeof got==="object"?`<p class="pf-gone">Can't show ${name}: ${pfEsc(got.err)}</p>`:got;
    return`<details class="pf-doc" data-pf="${p}"${open?" open":""}><summary><span class="pf-n">${name}</span>${btns(true)}</summary><div class="pf-body">${body}</div></details>`;
  }
  if(it.kind==="image")return withImages?`<figure class="pf-fig pf-row" data-pf-row="${p}"><figcaption>${name}</figcaption>${pfImg(it)}</figure>`:"";
  return`<div class="pf-row pf-file" data-pf-row="${p}"><span class="pf-n">${name}</span>${btns(it.kind==="pdf")}</div>`;
}
// The Proof section: docs, PDFs and other files (and pictures too when withImages), or "".
function proofSectionHtml(c,withImages){
  const rows=pfItems(c).map(it=>pfRow(it,withImages)).filter(Boolean);
  if(!rows.length)return"";
  return`<section class="pf" aria-label="Proof"><h4>Proof</h4>${withImages?`<div class="pf-grid">${rows.filter(r=>r.startsWith("<figure")).join("")}</div>`:""}${rows.filter(r=>!r.startsWith("<figure")).join("")}</section>`;
}
// The hand-over steps: secret-looking text stays hidden (stkMd's guard on the raw text); else its
// paths become chips and file names, and the rest is drawn by stkMd as before.
function proofSteps(c){
  const raw=String(c.nextStep||"");
  if(typeof SECRET!=="undefined"&&SECRET.test(raw))return stkMd(raw);
  const {text,marks}=proofChips(raw,pfItems(c));
  return pfSwap(stkMd(text),marks,pfEsc);
}
// A report drawn for the Proof section: Markdown with a short list of tags (no forms, buttons,
// styles or pictures), links other than http and https with no href.
function docHtml(text,plain){
  if(plain)return`<pre class="pf-txt">${pfEsc(text)}</pre>`;
  const raw=window.marked?marked.parse(String(text||""),{gfm:true,breaks:false}):pfEsc(text);
  const hadImg=/<img\b/i.test(raw);
  const clean=window.DOMPurify?DOMPurify.sanitize(raw,PF_PURIFY):pfEsc(text);
  const t=document.createElement("template");t.innerHTML=clean;
  for(const a of t.content.querySelectorAll("a")){if(pfLinkOk(a.getAttribute("href"))==="none")a.removeAttribute("href")}
  return(hadImg?`<p class="pf-note">Images in this report don't show here: open it to see them.</p>`:"")+`<div class="pf-md">${t.innerHTML}</div>`;
}

// --- Filling the box -------------------------------------------------------------------------

const pfFind=(root,attr,path)=>[...root.querySelectorAll(`[${attr}]`)].find(n=>n.getAttribute(attr)===path)||null;
function pfReadImg(path){
  const s=pfNow;if(!s||s.reading.has(path)||s.imgs.has(path))return;
  s.reading.add(path);const id=s.id;
  invoke("proof_read",{id,path}).then(r=>{if(pfNow!==s)return;s.imgs.set(path,r&&r.url?r.url:{err:"no picture"})},e=>{if(pfNow!==s)return;s.imgs.set(path,{err:typeof errText==="function"?errText(e):String(e)})})
    .finally(()=>{if(pfNow!==s)return;s.reading.delete(path);pfPaint()});
}
function pfReadDoc(path){
  const s=pfNow;if(!s||s.reading.has(path)||s.docs.has(path))return;
  s.reading.add(path);
  invoke("proof_read",{id:s.id,path}).then(r=>{if(pfNow!==s)return;
    const note=r&&r.cut?`<p class="pf-note">Cut at 1 MB: open it to read the rest.</p>`:"";
    s.docs.set(path,r&&typeof r.text==="string"?docHtml(r.text,!!r.plain)+note:{err:"no text"})},
  e=>{if(pfNow!==s)return;s.docs.set(path,{err:typeof errText==="function"?errText(e):String(e)})})
    .finally(()=>{if(pfNow!==s)return;s.reading.delete(path);pfPaint()});
}
// Puts what has been read into the panel's current box, in place.
function pfPaint(){
  const s=pfNow;if(!s||typeof panel==="undefined"||!panel||panel.id!==s.id)return;
  const box=panel.el.querySelector(".htbox");if(box)proofFill(box);
}
// After review.js draws the box: kept pictures and reports go in, and reads still owed start.
function proofFill(box){
  const s=pfNow;if(!box||!s)return;
  for(const img of box.querySelectorAll("img.pf-img[data-pf]")){
    const p=img.getAttribute("data-pf"),got=s.imgs.get(p);
    if(typeof got==="string"){if(img.getAttribute("src")!==got)img.src=got}
    else if(got&&typeof got==="object"){const n=document.createElement("p");n.className="pf-gone";n.textContent=`Can't show ${img.dataset.cap||pfName(p)}: ${got.err}`;img.replaceWith(n)}
    else pfReadImg(p);
  }
  for(const d of box.querySelectorAll("details.pf-doc[data-pf]")){
    const p=d.getAttribute("data-pf"),got=s.docs.get(p),body=d.querySelector(".pf-body");
    if(got!=null&&body&&!body.dataset.filled){body.innerHTML=typeof got==="string"?got:`<p class="pf-gone">Can't show ${pfEsc(pfName(p))}: ${pfEsc(got.err)}</p>`;body.dataset.filled="1"}
    else if(got==null&&d.open)pfReadDoc(p);
  }
}

// --- Clicks ----------------------------------------------------------------------------------

document.addEventListener("toggle",e=>{
  const d=e.target;if(!d||!d.matches||!d.matches("details.pf-doc")||!pfNow)return;
  const p=d.getAttribute("data-pf");if(d.open){pfNow.open.add(p);if(!pfNow.docs.has(p)){const w=d.querySelector(".pf-wait");if(w)w.textContent="Reading…";pfReadDoc(p)}}else pfNow.open.delete(p);
},true);
function pfOpenItem(path,reveal){
  const s=pfNow;if(!s)return;
  invoke("proof_open",{id:s.id,path,reveal}).catch(e=>{if(typeof status==="function")status(`Couldn't open it: ${typeof errText==="function"?errText(e):e}`,true)});
}
function pfChip(btn){
  const s=pfNow,root=btn.closest(".panel")||document;if(!s)return;
  const path=btn.getAttribute("data-pf-chip"),it=(s.items||[]).find(x=>x.path===path);if(!it)return;
  const show=n=>{if(n)n.scrollIntoView({block:"nearest",behavior:"smooth"})};
  if(it.state==="dup"){
    const img=[...root.querySelectorAll("img.shot[data-local][src]")].find(i=>i.getAttribute("data-local")===it.dupOf);
    if(img&&typeof openShot==="function"){img.dataset.cap=img.dataset.cap||pfName(it.dupOf);openShot(img)}
    return;
  }
  if(it.state==="ok"&&it.kind==="image"){
    const img=pfFind(root,"data-pf",path);
    if(img&&img.getAttribute("src")&&typeof openShot==="function")openShot(img);else show(img);
    return;
  }
  if(it.state==="ok"&&it.kind==="doc"){
    const d=[...root.querySelectorAll("details.pf-doc")].find(x=>x.getAttribute("data-pf")===path);
    if(d){d.open=true;show(d)}
    return;
  }
  show(pfFind(root,"data-pf-row",path));
}
document.addEventListener("click",e=>{
  const t=e.target&&e.target.closest?e.target:null;if(!t)return;
  // A proof picture in the Chat view's Proof section (the review strip has its own handler).
  const img=t.closest(".pf img.pf-img[src]");if(img&&!document.querySelector(".shotview")&&typeof openShot==="function"){e.preventDefault();e.stopPropagation();openShot(img);return}
  const chip=t.closest("button.pf-chip");if(chip){e.preventDefault();e.stopPropagation();pfChip(chip);return}
  const o=t.closest("button[data-pf-open],button[data-pf-reveal]");
  if(o){e.preventDefault();e.stopPropagation();const reveal=o.hasAttribute("data-pf-reveal");pfOpenItem(o.getAttribute(reveal?"data-pf-reveal":"data-pf-open"),reveal);return}
  // A report's link with no href (not http or https): nothing to follow.
  const a=t.closest(".pf-md a");if(a&&!a.getAttribute("href"))e.preventDefault();
},true);

window.Proof={pfLines,proofChips,pfSwap,pfLinkOk,pfName,pfState,pfItems,proofFill,docHtml,PF_TAGS,PF_PURIFY};
