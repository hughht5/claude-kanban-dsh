// Small-change auto-ship (card #104): a small, fully checked card in a repo Hugh listed waits 10
// minutes in Human test, then the app moves it into Ship it for him. This file draws its chip on
// the card (with Stop), the panel's "Small change" row, and Hugh's one-click opt-out. The app
// does the checks and the move; the page only shows them and sends Hugh's Stop.
let smallRepos=[];
async function smallLoad(){try{const s=await invoke("get_setting",{key:"sessions.small_goes_on_repos"});const v=JSON.parse(s.value||"[]");smallRepos=Array.isArray(v)?v:[]}catch(e){smallRepos=[]}}
const smallOf=c=>c&&c.smallChange&&typeof c.smallChange==="object"&&typeof c.smallChange.state==="string"?c.smallChange:null;
const smallListed=c=>!!(c&&typeof c.repo==="string"&&smallRepos.some(r=>r&&typeof r.repo==="string"&&r.repo.toLowerCase()===c.repo.trim().toLowerCase()));
const smallInStack=c=>!!(c&&(c.stackOf!=null||(Array.isArray(c.stack)&&c.stack.length)));
const smallWaiting=c=>{const s=smallOf(c);return !!(s&&s.state==="waiting"&&colOf(c)==="Human test"&&c.smallOptOut!==true)};
const smallText=(m,fmt)=>fmt==="chip"?(m?`Goes on in ${m} min`:"Goes on now"):(m?`in ${m} min`:"now");
// Minutes left, rounded up; 0 when it is due (the app moves it at its next look, within 20 s).
const smallLeft=s=>{const t=Date.parse(s&&s.goesAt||"");return t?Math.max(0,Math.ceil((t-Date.now())/60000)):0};
function smallChip(c){
  const s=smallOf(c);if(!s)return"";
  if(smallWaiting(c)){const m=smallLeft(s);
    return`<span class="tag chip small-wait" data-goes="${esc(s.goesAt||"")}" data-fmt="chip" title="Small and fully checked: the app moves it into Ship it for Hugh unless he stops it">${smallText(m,"chip")}</span><button class="tag chip small-stop" type="button" data-small-stop="${esc(c.id)}" title="Keep this card in Human test for Hugh">Stop</button>`}
  if(s.state==="went on"&&["Ship it","Deploy complete","Done"].includes(colOf(c)))
    return`<span class="tag chip small-went" title="It went on to Ship it by itself: a small, fully checked change">Went on by itself</span>`;
  return"";
}
const smallFiles=s=>Array.isArray(s.files)?s.files.map(f=>`${safe(f.path)} <span class="meta">(${esc(f.kind)}, +${esc(f.added)} −${esc(f.removed)})</span>`).join("<br>"):"";
const smallCommits=s=>Array.isArray(s.commits)?s.commits.map(x=>`<span class="mono">${esc(String(x.sha||"").slice(0,7))}</span> ${safe(x.subject)}`).join("<br>"):"";
const smallChecks=s=>{const t=s.test||{},r=s.review||{};return`tester PASS (${esc(t.tools)} tool calls) · reviewer PASS (${esc(r.tools)} tool calls, scope ${esc(r.scope||"?")})`};
const smallBtn=(c,on)=>`<button class="btn ghost" type="button" data-small-opt="${on?"on":"off"}" data-id="${esc(c.id)}">${on?"Don't let it go on by itself":"Let it go on by itself"}</button>`;
function smallBox(c){
  const s=smallOf(c),listed=smallListed(c),out=c.smallOptOut===true;
  if(!s&&!listed)return"";
  if(smallInStack(c)&&!s)return"";
  const at=s?Date.parse(s.at||""):0,when=at?` <span class="meta" title="${esc(s.at)}">${esc(ago(at))}</span>`:"";
  let body="";
  if(s&&s.state==="waiting"&&colOf(c)==="Human test"&&!out){const g=Date.parse(s.goesAt||""),m=smallLeft(s);
    body=`<p><b>This card would go on by itself</b> to Ship it <span data-goes="${esc(s.goesAt||"")}" data-fmt="panel">${smallText(m,"panel")}</span>${g?` (at ${esc(new Date(g).toLocaleTimeString([],{hour:"2-digit",minute:"2-digit"}))})`:""}, with no human test: the change is small and fully checked, and Hugh turned on small-change auto-ship for ${safe(c.repo)}.</p>
      <div>${smallFiles(s)}</div><div class="meta">${esc(s.lines)} lines · ${smallChecks(s)}</div><div class="row">${smallBtn(c,true)}</div>`}
  else if(s&&s.state==="went on"){
    body=`<p><b>Went on by itself</b>${when}: a small, fully checked change in ${safe(c.repo)}.</p><div>${smallFiles(s)}</div><div>${smallCommits(s)}</div>
      <div class="meta">${esc(s.lines)} lines, ${esc(s.bytes)} bytes · ${smallChecks(s)} · ships exactly <span class="mono">${esc(String(s.head||"").slice(0,12))}</span> to ${safe(s.branch)}</div>
      ${typeof evidenceHtml==="function"?evidenceHtml(c):""}`}
  else{
    const why=s&&s.state==="stopped"&&s.reason?`<p>Stopped${when}: ${safe(s.reason)}.</p>`:"";
    const rule=listed&&!smallInStack(c)?(out?`<p>Hugh turned small-change auto-ship off for this card.</p><div class="row">${smallBtn(c,false)}</div>`
      :`<p>If its change is small and fully checked (docs or CSS only, both subagents passed on the final commit), this card goes on to Ship it by itself after 10 minutes in Human test (Hugh's setting for ${safe(c.repo)}).</p><div class="row">${smallBtn(c,true)}</div>`):"";
    body=why+rule;
  }
  return body?`<div class="shipbox small-box"><h3>Small change</h3>${body}</div>`:"";
}
async function smallOpt(id,on){
  const c=cards.find(x=>x.id===id);if(!c)return;
  try{await invoke("small_opt_out",{id,version:c.version,on});status(on?`#${cardNum(c)||id} stays in Human test for you`:`#${cardNum(c)||id} may go on by itself again`)}
  catch(e){status("Couldn't change it: "+errText(e)+". Try again.",true);await load()}
}
function smallWire(c,root){root.querySelectorAll("[data-small-opt]").forEach(b=>b.onclick=()=>smallOpt(b.dataset.id,b.dataset.smallOpt==="on"))}
// Stop on the card itself: a click there must not open the panel or start a drag.
document.addEventListener("click",e=>{const b=e.target.closest("[data-small-stop]");if(!b)return;e.preventDefault();e.stopPropagation();smallOpt(b.dataset.smallStop,true)},true);
document.addEventListener("mousedown",e=>{if(e.target.closest("[data-small-stop]"))e.stopPropagation()},true);
// The countdown: each half minute the chips and the panel line change their text in place. No
// redraw, so a drag or the keyboard focus on the board is never interrupted.
setInterval(()=>{document.querySelectorAll("[data-goes][data-fmt]").forEach(el=>{const t=smallText(smallLeft({goesAt:el.dataset.goes}),el.dataset.fmt);if(el.textContent!==t)el.textContent=t})},30000);
(async()=>{await smallLoad();try{const ev=window.__TAURI__.event;
  ev.listen("settings-changed",async p=>{if(p&&p.payload==="sessions.small_goes_on_repos"){await smallLoad();render()}})}catch(e){}})();
