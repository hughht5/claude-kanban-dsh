// Time in each column (card #111). The card panel's "Time in columns" section, and each column
// header's tooltip with the board's median. The store works the times out from the card's
// revisions (crates/store/src/times.rs); this file only draws them.
//
// - The panel draws from a cache keyed by card id and version, so a live update never blanks the
//   section; it fetches only when the card has changed (or another opens). A failed fetch isn't
//   retried for the same card version.
// - The board's medians are cached by the app (src-tauri/src/times.rs, 5 minutes); the page asks
//   at most once a minute, and after a failure waits a minute too.
// - The column the card is in now keeps counting: once a minute only its time's text is updated,
//   so focus, the fold and any tooltip stay put.
const TIMES_MAIN=["Planning","Plan review","In progress","Human test"]; // the rows Hugh named; the rest fold away
const TIMES_ASK_EVERY=60*1000;
let timesCache=null;   // {id, version, data, at}: the last card's times, and when they came
let timesWant=null;    // "id@version" being fetched
let timesFailed=null;  // "id@version" whose fetch failed: not asked again
let colMedians=null,colMediansAsked=0,colMediansPend=false;
let colMediansFailed=false; // the last column_medians failed: the bar says so; asked again after the back-off
let timesOther=false;  // the "Other columns" fold, as Hugh last left it
try{timesOther=localStorage.getItem("wb.timesOther")==="1"}catch(e){}

// "<1m", "45m", "3h 20m", "2d 4h".
function fmtSpan(ms){
  const m=Math.floor(Math.max(0,ms||0)/60000);
  if(m<1)return"<1m";
  if(m<60)return m+"m";
  const h=Math.floor(m/60);
  if(h<24)return`${h}h ${m%60}m`;
  return`${Math.floor(h/24)}d ${h%24}h`;
}
const plural=(n,w)=>`${n} ${w}${n===1?"":"s"}`;

function medianOf(col){return Array.isArray(colMedians)?colMedians.find(m=>m.column===col):null}
// The tooltip for a column's median, in the panel (`head` false) or on its header.
function medianTitle(col,head){
  const m=medianOf(col);if(!m)return"";
  const span=`the last ${m.days} days`;
  if(m.ms==null)return`No visit to this column finished in ${span}`;
  return head?`Cards spend a median of ${fmtSpan(m.ms)} here (${plural(m.visits,"visit")} that finished in ${span})`
    :`Median time here, over the ${plural(m.visits,"visit")} that finished in ${span} (test cards left out)`;
}
function loadMedians(){
  if(colMediansPend||Date.now()-colMediansAsked<TIMES_ASK_EVERY)return;
  colMediansPend=true;colMediansAsked=Date.now();
  invoke("column_medians").then(m=>{colMedians=Array.isArray(m)?m:[];colMediansFailed=false;timesHeads();timesRedraw();timesBarRedraw()})
    // Asked again on the next open after TIMES_ASK_EVERY, not on every render.
    .catch(()=>{colMediansFailed=true;try{timesBarRedraw()}catch(e){}})
    .finally(()=>{colMediansPend=false});
}

// The column headers' tooltips: board.js calls this after each render.
function timesHeads(){
  loadMedians();
  document.querySelectorAll(".board .col[data-col]").forEach(el=>{
    const h=el.querySelector(".colhead");if(h)h.title=medianTitle(el.dataset.col,true);
  });
}

// A card's time in one column, counting on from when the times came if it is there now.
function timeNow(row){return row.ms+(row.running&&timesCache?Date.now()-timesCache.at:0)}
// The time's text, marked so the minute tick can update it alone.
function timeSpan(row){return`<span class="tms" data-col="${esc(row.column)}">${esc(fmtSpan(timeNow(row)))}</span>`}

function timesRow(row){
  const m=medianOf(row.column);
  const mine=row.visits?`${timeSpan(row)}${row.visits>1?`<span class="tvis"> · ${esc(plural(row.visits,"visit"))}</span>`:""}`:"—";
  return`<div class="trow${row.running?" now":""}"><span class="tcol">${esc(row.column)}${row.running?`<span class="tnow" title="The card is in this column now: its time keeps counting">now</span>`:""}</span>`+
    `<span class="tval">${mine}</span><span class="tmed" title="${esc(medianTitle(row.column,false))}">${m?`median ${m.ms==null?"—":esc(fmtSpan(m.ms))}`:""}</span></div>`;
}

function timesInner(data){
  const rows=Array.isArray(data.columns)?data.columns:[];
  const main=TIMES_MAIN.map(c=>rows.find(r=>r.column===c)).filter(Boolean);
  const other=rows.filter(r=>!TIMES_MAIN.includes(r.column));
  const spent=other.filter(r=>r.visits);
  const sumLine=spent.length?spent.map(r=>`${esc(r.column)} ${timeSpan(r)}`).join(", "):"none";
  const notes=[];
  if(data.imported&&data.startedAt)notes.push(`Counted from when it was imported, ${new Date(data.startedAt).toLocaleDateString(undefined,{day:"numeric",month:"short",year:"numeric"})}.`);
  if(data.unknownMs>0)notes.push(`${fmtSpan(data.unknownMs)} before its first recorded move isn't counted: the board doesn't know which column it was in.`);
  return main.map(timesRow).join("")+
    `<details class="tother"${timesOther?" open":""}><summary>Other columns: ${sumLine}</summary>${other.map(timesRow).join("")}</details>`+
    notes.map(n=>`<p class="tnote">${esc(n)}</p>`).join("");
}

// The section for board.js renderDetail: drawn from the cache, fetched when the card changed.
function timesSection(c){
  loadMedians();
  const fresh=timesCache&&timesCache.id===c.id&&timesCache.version===c.version;
  if(!fresh)fetchTimes(c);
  const mine=timesCache&&timesCache.id===c.id;
  const body=mine?timesInner(timesCache.data):timesFailed===c.id+"@"+c.version?`<div class="quiet">Couldn't read the times.</div>`:`<div class="quiet">Reading…</div>`;
  return`<h3>Time in columns</h3><div class="times" id="p-times" data-id="${esc(c.id)}">${body}</div>`;
}
function fetchTimes(c){
  const key=c.id+"@"+c.version;if(timesWant===key||timesFailed===key)return;timesWant=key;
  invoke("card_times",{cardId:c.id}).then(data=>{
    if(timesWant!==key)return;
    timesCache={id:c.id,version:c.version,data,at:Date.now()};timesRedraw();
  }).catch(e=>{
    if(timesWant!==key)return;
    timesFailed=key;
    const box=document.getElementById("p-times");
    if(box&&box.dataset.id===c.id&&!(timesCache&&timesCache.id===c.id))box.innerHTML=`<div class="quiet">Couldn't read the times: ${esc(typeof errText==="function"?errText(e):String(e))}</div>`;
  }).finally(()=>{if(timesWant===key)timesWant=null});
}
// Redraws only the section (new times or medians), so nothing Hugh is typing is touched.
function timesRedraw(){
  const box=document.getElementById("p-times");
  if(!box||!timesCache||box.dataset.id!==timesCache.id)return;
  box.innerHTML=timesInner(timesCache.data);
}
// The minute tick: only the running column's time text changes.
function timesTick(){
  const box=document.getElementById("p-times");
  if(!box||!timesCache||box.dataset.id!==timesCache.id)return;
  for(const row of (timesCache.data.columns||[]).filter(r=>r.running)){
    box.querySelectorAll(`.tms[data-col="${CSS.escape(row.column)}"]`).forEach(s=>{const t=fmtSpan(timeNow(row));if(s.textContent!==t)s.textContent=t});
  }
}
document.addEventListener("toggle",e=>{
  const d=e.target;if(!d||!d.matches||!d.matches(".times details.tother"))return;
  timesOther=d.open;try{localStorage.setItem("wb.timesOther",timesOther?"1":"0")}catch(err){}
},true);
setInterval(timesTick,60*1000);

// The board's time bar (card #120): one stacked bar of the median time in each working column,
// with a legend, drawn in the session search under its results (search.js's #ss-times). Medians
// over the visits that finished in the last N days (column_medians), not averages. Backlog,
// Deploy complete and Done are left out: a card waits there, it isn't worked on.
const TIMES_BAR_SKIP=["Backlog","Deploy complete","Done","Archive"];
const TIMES_BAR_TONE={"Planning":"var(--muted)","Plan review":"var(--warn)","In progress":"var(--accent)","Human test":"var(--warn)","Ship it":"var(--info)",
  "Blocked":"color-mix(in srgb,var(--muted) 55%,var(--panel))"};
function timesBarHtml(){
  if(!Array.isArray(colMedians))return`<div class="tbar-ttl">Median time in each column</div><div class="quiet tbar-none">${colMediansFailed?"Times not available":"Reading the times…"}</div>`;
  const order=typeof BOARD_COLS!=="undefined"?BOARD_COLS:[];
  const rows=colMedians.filter(m=>m&&!TIMES_BAR_SKIP.includes(m.column)&&m.ms>0)
    .sort((a,b)=>{const i=order.indexOf(a.column),j=order.indexOf(b.column);return(i<0?99:i)-(j<0?99:j)});
  const days=(colMedians.find(m=>m&&m.days)||{}).days;
  const ttl=`<div class="tbar-ttl">Median time in each column${days?` <span>finished visits, last ${esc(days)} days</span>`:""}</div>`;
  if(!rows.length)return ttl+`<div class="quiet tbar-none">No visit to a column finished${days?` in the last ${esc(days)} days`:""}.</div>`;
  const all=rows.reduce((t,m)=>t+m.ms,0);
  const tone=(c,i)=>TIMES_BAR_TONE[c]||`color-mix(in srgb,var(--muted) ${40+15*(i%3)}%,var(--panel))`;
  const say=rows.map(m=>`${m.column} ${fmtSpan(m.ms)}`).join(", ");
  return ttl+`<div class="colbar" role="img" aria-label="${esc(say)}">${rows.map((m,i)=>`<i style="width:${(100*m.ms/all).toFixed(2)}%;background:${tone(m.column,i)}" title="${esc(medianTitle(m.column,true))}"></i>`).join("")}</div>`
    +`<div class="legend">${rows.map((m,i)=>`<span title="${esc(medianTitle(m.column,true))}"><i style="background:${tone(m.column,i)}"></i>${esc(m.column)} ${esc(fmtSpan(m.ms))}</span>`).join("")}</div>`;
}
function timesBarRedraw(){
  const el=document.getElementById("ss-times");if(!el)return;
  const h=timesBarHtml();if(el.dataset.html!==h){el.innerHTML=h;el.dataset.html=h}
}
