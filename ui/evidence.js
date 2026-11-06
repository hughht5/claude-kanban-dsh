// The card-tester's before-and-after screenshots on a card (card #101). The app keeps them in the
// app-only card field "evidence", newest run first:
//   [{run, at, verdict: passed|failed|unclear, shots: [{n, kind: before|after, caption, bytes,
//     width, height, file, path}]}]
// evidenceHtml(c) gives the Evidence box (root class "ev"), or "" when the card has none. Other
// boxes (the Human test box, the stack overlay) may call it too; the card's details keep only the
// first .ev, so the evidence shows once. showEvidence(root) loads the shots on view and opens one
// full size on a click, in panel.js's viewer. Uses board.js's esc and panel.js's shotImg,
// showShots and openShot, only when they run.
const EV_VERDICT={passed:"PASS",failed:"FAIL",unclear:"UNCLEAR"};
const EV_KIND={before:"Before",after:"After"};

function evWhen(at){
  const t=Date.parse(at||"");if(!t)return String(at||"");
  try{return new Date(t).toLocaleString("en-GB",{day:"numeric",month:"short",year:"numeric",hour:"2-digit",minute:"2-digit"})}catch(e){return String(at)}
}
function evRunLine(r){
  const v=EV_VERDICT[r.verdict];
  // The build the test app ran when the shots were taken (cdp.mjs stamps it in each PNG).
  const b=typeof r.build==="string"&&r.build?r.build:"";
  const build=b?` · <span class="ev-b" title="The test app's build when it was shot">${b==="mixed"?"mixed builds":`build ${esc(b.slice(0,9))}`}</span>`:"";
  return`<div class="ev-run">card-tester · <b class="ev-v${v?` ev-${r.verdict}`:""}">${esc(v||String(r.verdict||"no verdict").toUpperCase())}</b>${r.at?` · <time datetime="${esc(r.at)}" title="${esc(r.at)}">${esc(evWhen(r.at))}</time>`:""}${build}</div>`;
}
// Whether run r is the card's latest test: the same tester run and the same time as its test record.
function evIsLatest(c,r){const t=c&&c.test;return!!(t&&r&&t.agentId===r.run&&t.at===r.at)}
function evShots(r){
  const shots=(Array.isArray(r.shots)?r.shots:[]).filter(s=>s&&typeof s.path==="string"&&s.path);
  if(!shots.length)return`<div class="ev-none">No shots in this run.</div>`;
  shots.sort((a,b)=>(Number(a.n)||0)-(Number(b.n)||0)); // the tester's order: before, then after
  return`<div class="ev-grid">${shots.map(s=>`<figure class="ev-shot"><div class="ev-k">${esc(EV_KIND[s.kind]||s.kind||"Shot")}</div>${shotImg(s.path,r.at)}${s.caption?`<figcaption>${esc(s.caption)}</figcaption>`:""}</figure>`).join("")}</div>`;
}
function evidenceHtml(c){
  const runs=(c&&Array.isArray(c.evidence)?c.evidence:[]).filter(r=>r&&typeof r==="object");
  if(!runs.length)return"";
  const [latest,...older]=runs;
  const hasBefore=Array.isArray(latest.shots)&&latest.shots.some(s=>s&&s.kind==="before");
  // The card was tested again since these shots, and that test took none: say so, above them.
  const stale=c.test&&c.test.at&&!evIsLatest(c,latest)?`<p class="ev-stale">Not from the latest test (${esc(evWhen(c.test.at))}), which took no shots. These are from an earlier run.</p>`:"";
  return`<section class="ev" aria-label="Evidence"><h3>Evidence</h3>${stale}${evRunLine(latest)}${evShots(latest)}`
    +(hasBefore?`<p class="ev-note">Before: the new build, just before the tester acted.</p>`:"")
    +(older.length?`<details class="ev-old"><summary>Earlier runs (${older.length})</summary>${older.map(r=>`<div class="ev-past">${evRunLine(r)}${evShots(r)}</div>`).join("")}</details>`:"")
    +`</section>`;
}
// Wires root once (it survives redraws of its contents): a click on a loaded shot opens it in
// panel.js's viewer, unless a viewer is already open; opening "Earlier runs" loads its shots.
function showEvidence(root){
  if(!root||!root.querySelector)return;
  if(!root.dataset.evWired){
    root.dataset.evWired="1";
    root.addEventListener("click",e=>{
      const img=e.target&&e.target.closest?e.target.closest(".ev img.shot[src]"):null;if(!img||!root.contains(img))return;
      e.preventDefault();e.stopPropagation();
      if(document.querySelector(".shotview"))return; // one viewer at a time
      openShot(img);
    });
    root.addEventListener("toggle",e=>{const t=e.target;if(t&&t.tagName==="DETAILS"&&t.open&&t.closest(".ev"))showShots(t)},true);
  }
  showShots(root);
}
