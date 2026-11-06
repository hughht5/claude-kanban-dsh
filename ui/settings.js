// Settings: a full overlay, opened from the gear in the header or "settings" in the Ctrl+K bar.
// Columns (prompt, model, effort), card templates, the browser-profile map for logins, and the
// session cap. Everything is saved in the board's store through the app's commands; nothing
// here is kept in the page. Uses board.js's $, esc, invoke and errText.
(()=>{
const MODELS=[["claude-opus-5-5","Opus 5.5"],["claude-fable-5-1","Fable 5.1"],["claude-sonnet-5-5","Sonnet 5.5"],["claude-haiku-5-5","Haiku 5.5"]];
const EFFORTS=["low","medium","high","xhigh","max"];
// The store refuses these too; checking here keeps the typed text and says why at once.
const TOKEN=/customtoken|idtoken|eyJ[A-Za-z0-9_-]{6,}|sk_(live|test)/i;
const SECTIONS=[["columns","Columns"],["templates","Templates"],["profiles","Browser profiles"],["sessions","Sessions"],["stacks","Stacks"],["router","Router"]];
let root=null,section="columns",showArchivedTpl=false;
try{section=localStorage.getItem("settings.section")||"columns"}catch(e){}
if(!SECTIONS.some(s=>s[0]===section))section="columns";

function openSettings(which){
  if(which)section=which;
  if(root){show();return}
  root=document.createElement("div");root.className="st-wrap";
  root.innerHTML=`<div class="st-back"></div>
  <div class="st" role="dialog" aria-modal="true" aria-labelledby="st-h">
   <div class="st-head"><h2 id="st-h">Settings</h2><span class="meta">Saved in this board's store. Changes apply to the next prompt or session.</span>
    <button class="ibtn x" id="st-close" type="button" aria-label="Close settings">✕</button></div>
   <div class="st-body"><nav class="st-nav" aria-label="Settings sections">${SECTIONS.map(([k,l])=>`<button type="button" data-sec="${k}">${l}</button>`).join("")}</nav>
    <section class="st-main" id="st-main" tabindex="-1"></section></div>
  </div>`;
  document.body.appendChild(root);
  root.querySelector(".st-back").onclick=closeSettings;
  root.querySelector("#st-close").onclick=closeSettings;
  root.querySelectorAll("[data-sec]").forEach(b=>b.onclick=()=>{section=b.dataset.sec;try{localStorage.setItem("settings.section",section)}catch(e){}show()});
  document.addEventListener("keydown",keys,true);
  show();
  root.querySelector("#st-close").focus();
}
function closeSettings(){if(!root)return;document.removeEventListener("keydown",keys,true);root.remove();root=null;const g=$("#settingsBtn");if(g)g.focus()}
function keys(e){if(e.key==="Escape"&&root){e.preventDefault();e.stopPropagation();closeSettings()}}
window.openSettings=openSettings;window.closeSettings=closeSettings;

function show(){
  root.querySelectorAll("[data-sec]").forEach(b=>b.setAttribute("aria-current",b.dataset.sec===section?"page":"false"));
  const m=root.querySelector("#st-main");m.innerHTML=`<p class="meta">Loading…</p>`;
  ({columns,templates,profiles,sessions,stacks,router})[section](m).catch(e=>{m.innerHTML=`<p class="msg err">Couldn't load this: ${esc(errText(e))}</p>`});
}
const note=(el,t,kind)=>{el.textContent=t;el.className="msg "+(kind||"")};
const opt=(v,cur,label)=>`<option value="${esc(v)}"${v===cur?" selected":""}>${esc(label??v)}</option>`;

// Columns: each column's prompt, model and effort, with Reset to the shipped values.
async function columns(m){
  const rows=await invoke("list_columns");
  m.innerHTML=`<h3>Columns</h3>
  <p class="note">A column's prompt is typed into the card's session when Hugh moves the card in (columns set to send no prompt skip it). {n}, {title}, {repo}, {issue}, {from} and {reason} are filled in. Model and effort apply when a session next starts.</p>
  ${rows.map((r,i)=>colBox(r,i)).join("")}`;
  rows.forEach((r,i)=>wireCol(m,r,i));
}
function colBox(r,i){
  const c=r.column,eff=c.effort||"";
  return `<div class="st-box" data-i="${i}"><div class="st-row"><b class="st-name">${esc(c.name)}</b>
   <span class="tag" data-state>${r.isDefault?"default":"changed"}</span>${c.mode==="none"?`<span class="meta">sends no prompt on a move</span>`:`<span class="meta">${c.auto_spawn?"starts a session on a move":"prompts a running session only"}</span>`}</div>
   <label class="f">Prompt<textarea class="st-prompt" rows="9" spellcheck="false" placeholder="No prompt"></textarea></label>
   <div class="st-row"><label class="f">Model<select class="tx st-model"><option value="">The app's default</option>${MODELS.map(([v,n])=>`<option value="${v}">${n}</option>`).join("")}</select></label>
    <label class="f">Effort<select class="colsel st-effort">${opt("",eff,"the app's default")}${EFFORTS.map(x=>opt(x,eff)).join("")}</select></label></div>
   <div class="row"><button class="btn st-save" type="button">Save</button><button class="btn ghost st-reset" type="button">Reset to default</button><span class="msg"></span></div></div>`;
}
function wireCol(m,r,i){
  const box=m.querySelector(`.st-box[data-i="${i}"]`),c=r.column;
  const p=box.querySelector(".st-prompt"),mo=box.querySelector(".st-model"),ef=box.querySelector(".st-effort"),msg=box.querySelector(".msg"),tag=box.querySelector("[data-state]");
  const fill=x=>{p.value=x.prompt_text||"";if(x.model&&![...mo.options].some(o=>o.value===x.model)){const o=document.createElement("option");o.value=o.textContent=x.model;mo.appendChild(o)}mo.value=x.model||"";ef.value=x.effort||""};
  fill(c);
  const same=(x,d)=>d&&(x.prompt_text||null)===(d.prompt_text||null)&&(x.model||null)===(d.model||null)&&(x.effort||null)===(d.effort||null);
  const mark=x=>{tag.textContent=same(x,r.default)?"default":"changed"};
  box.querySelector(".st-save").onclick=async()=>{
    if(TOKEN.test(p.value)){note(msg,"Not saved: the prompt looks like it holds a token or key.","err");return}
    try{const x=await invoke("update_column",{name:c.name,prompt:p.value,model:mo.value.trim(),effort:ef.value});fill(x);mark(x);note(msg,"Saved","ok")}
    catch(e){note(msg,"Not saved: "+errText(e),"err")}};
  box.querySelector(".st-reset").onclick=async()=>{
    if(!confirm(`Put ${c.name}'s prompt, model and effort back to their defaults? Your text for it is replaced.`))return;
    try{const x=await invoke("reset_column",{name:c.name});fill(x);mark(x);note(msg,"Reset to default","ok")}
    catch(e){note(msg,"Couldn't reset: "+errText(e),"err")}};
}

// Templates: editable, addable and archivable (never deleted).
async function templates(m){
  const list=await invoke("list_templates",{archived:true});
  const shown=list.filter(t=>showArchivedTpl||!t.archived),nArch=list.filter(t=>t.archived).length;
  m.innerHTML=`<h3>Templates</h3>
  <p class="note">A template fills the Add card form (a title prefix and a description) and gives the card's first session its starting prompt, sent before the column prompt. Archived templates leave the picker; cards made from them keep them.</p>
  <div class="row"><button class="btn" id="st-tadd" type="button">Add template</button><button class="archtog" id="st-tarch" type="button" aria-pressed="${showArchivedTpl}">${showArchivedTpl?"Hide":"Show"} archived (${nArch})</button></div>
  <div id="st-tlist">${shown.map(tplBox).join("")||`<p class="meta">No templates. Add one.</p>`}</div>`;
  m.querySelector("#st-tarch").onclick=()=>{showArchivedTpl=!showArchivedTpl;templates(m)};
  m.querySelector("#st-tadd").onclick=()=>{const l=m.querySelector("#st-tlist");if(l.querySelector('.st-box[data-id=""]'))return;
    if(!l.querySelector(".st-box"))l.innerHTML="";
    l.insertAdjacentHTML("afterbegin",tplBox({id:"",name:"",titlePrefix:"",body:"",startPrompt:"",archived:false}));wireTpl(m,l.querySelector(".st-box"));l.querySelector(".st-tname").focus()};
  m.querySelectorAll("#st-tlist .st-box").forEach(b=>wireTpl(m,b,list.find(t=>t.id===b.dataset.id)));
}
function tplBox(t){
  return `<div class="st-box${t.archived?" st-arch":""}" data-id="${esc(t.id)}">
   <div class="st-row"><label class="f st-grow">Name<input class="tx st-tname" maxlength="80" value="${esc(t.name)}" placeholder="Bug"></label>
    <label class="f">Title prefix<input class="tx st-tprefix" maxlength="60" value="${esc(t.titlePrefix)}" placeholder="Bug: "></label>${t.archived?`<span class="tag">archived</span>`:""}</div>
   <label class="f">Description (fills the card's description)<textarea class="st-tbody" rows="5">${esc(t.body)}</textarea></label>
   <label class="f">Starting prompt (the session's first message, with the column prompt)<textarea class="st-tstart" rows="4" spellcheck="false">${esc(t.startPrompt)}</textarea></label>
   <div class="row"><button class="btn st-tsave" type="button">${t.id?"Save":"Add"}</button>${t.id?`<button class="btn ghost st-tarchive" type="button">${t.archived?"Restore":"Archive"}</button>`:`<button class="btn ghost st-tcancel" type="button">Cancel</button>`}<span class="msg"></span></div></div>`;
}
function wireTpl(m,box,t){
  const msg=box.querySelector(".msg"),v=s=>box.querySelector(s).value;
  box.querySelector(".st-tsave").onclick=async()=>{
    const template={id:box.dataset.id||null,name:v(".st-tname").trim(),titlePrefix:v(".st-tprefix"),body:v(".st-tbody"),startPrompt:v(".st-tstart")};
    if(!template.name){note(msg,"Give the template a name.","err");return}
    if(TOKEN.test([template.name,template.titlePrefix,template.body,template.startPrompt].join(" "))){note(msg,"Not saved: something here looks like a token or key.","err");return}
    try{const x=await invoke("save_template",{template});
      if(!box.dataset.id){templates(m);return}
      box.dataset.id=x.id;note(msg,"Saved","ok")}
    catch(e){note(msg,"Not saved: "+errText(e),"err")}};
  const c=box.querySelector(".st-tcancel");if(c)c.onclick=()=>templates(m);
  const a=box.querySelector(".st-tarchive");
  if(a)a.onclick=async()=>{try{await invoke("archive_template",{id:box.dataset.id,on:!(t&&t.archived)});templates(m)}catch(e){note(msg,"Couldn't change it: "+errText(e),"err")}};
}

// Browser profiles: which Chrome profile a login opens in, by its browser label or its link.
async function profiles(m){
  const s=await invoke("get_setting",{key:"launch.profiles"});
  let rows=[];try{rows=JSON.parse(s.value)}catch(e){rows=JSON.parse(s.default)}
  const defaults=JSON.parse(s.default);
  m.innerHTML=`<h3>Browser profiles</h3>
  <p class="note">A login opens in Chrome with the profile folder of the row whose name is its browser label; if no name matches, the first row whose URL pattern matches its link (* matches anything). Anything else opens in the default browser. The folder is Chrome's own name for the profile, such as Default or Profile 1 (see chrome://version).</p>
  <div class="st-table" role="table" aria-label="Browser profiles"><div class="st-tr st-th" role="row"><span role="columnheader">Name (browser label)</span><span role="columnheader">URL pattern (optional)</span><span role="columnheader">Profile folder</span><span></span></div><div id="st-prows"></div></div>
  <div class="row"><button class="btn ghost" id="st-padd" type="button">Add row</button><button class="btn" id="st-psave" type="button">Save</button><button class="btn ghost" id="st-pdef" type="button">Reset to defaults</button><span class="msg" id="st-pmsg"></span></div>`;
  const body=m.querySelector("#st-prows"),msg=m.querySelector("#st-pmsg");
  const draw=list=>{body.innerHTML=list.map((r,i)=>`<div class="st-tr" role="row" data-i="${i}">
     <input class="tx" aria-label="Name" data-k="name" value="${esc(r.name)}" placeholder="Chrome · work (Profile 1)">
     <input class="tx" aria-label="URL pattern" data-k="urlPattern" value="${esc(r.urlPattern||"")}" placeholder="http://127.0.0.1:*?as=admin*" spellcheck="false">
     <input class="tx" aria-label="Profile folder" data-k="profileDir" value="${esc(r.profileDir)}" placeholder="Profile 1" spellcheck="false">
     <button class="ibtn x" type="button" data-del="${i}" aria-label="Remove this row">✕</button></div>`).join("")||`<p class="meta">No rows: every login opens in the default browser.</p>`;
    body.querySelectorAll("[data-del]").forEach(b=>b.onclick=()=>{const l=read();l.splice(+b.dataset.del,1);draw(l)})};
  const read=()=>[...body.querySelectorAll(".st-tr")].map(tr=>{const o={};tr.querySelectorAll("[data-k]").forEach(x=>o[x.dataset.k]=x.value.trim());return o});
  draw(rows);
  m.querySelector("#st-padd").onclick=()=>{const l=read();l.push({name:"",urlPattern:"",profileDir:""});draw(l);const ins=body.querySelectorAll('[data-k="name"]');if(ins.length)ins[ins.length-1].focus()};
  m.querySelector("#st-pdef").onclick=()=>{draw(defaults);note(msg,"Defaults filled in: Save to keep them.","")};
  m.querySelector("#st-psave").onclick=async()=>{
    try{const v=await invoke("set_setting",{key:"launch.profiles",value:JSON.stringify(read())});draw(JSON.parse(v));note(msg,"Saved","ok")}
    catch(e){note(msg,"Not saved: "+errText(e),"err")}};
}

// Sessions: the most card sessions that run at once, idle pausing, and the subagent test gate.
async function sessions(m){
  const s=await invoke("get_setting",{key:"sessions.max_running"});
  m.innerHTML=`<h3>Sessions</h3>
  <div class="st-box"><label class="f">Max running sessions<input class="tx st-num" id="st-max" type="number" min="1" max="100" step="1" value="${esc(s.value)}"></label>
  <p class="meta">The most card sessions running at once, from 1 to 100 (default ${esc(s.default)}).</p>
  <div class="row"><button class="btn" id="st-msave" type="button">Save</button><span class="msg" id="st-mmsg"></span></div></div>`;
  const msg=m.querySelector("#st-mmsg");
  m.querySelector("#st-msave").onclick=async()=>{
    const v=m.querySelector("#st-max").value.trim();
    if(!/^\d+$/.test(v)||+v<1||+v>100){note(msg,"Give a whole number from 1 to 100.","err");return}
    try{const x=await invoke("set_setting",{key:"sessions.max_running",value:v});m.querySelector("#st-max").value=x;note(msg,"Saved","ok")}
    catch(e){note(msg,"Not saved: "+errText(e),"err")}};
  // The private folder: your own column prompts (prompts\) and repo list (repos.json), kept out of the public repo.
  const pv=await invoke("get_setting",{key:"private.dir"});
  const pbox=document.createElement("div");pbox.className="st-box";
  pbox.innerHTML=`<label class="f">Private folder<input class="tx" id="st-priv" placeholder="~\\kanban-private" value="${esc(pv.value)}"></label>
  <p class="meta">A folder of your own, outside this app's repo. Its <code>prompts\\</code> folder replaces the example column prompts (they name it as {prompts}), and its <code>repos.json</code> lists the repos a card can be for. Empty uses the examples.</p>
  <div class="row"><button class="btn" id="st-psave" type="button">Save</button><span class="msg" id="st-pmsg"></span></div>`;
  m.appendChild(pbox);
  const pm=pbox.querySelector("#st-pmsg");
  pbox.querySelector("#st-psave").onclick=async()=>{
    try{const x=await invoke("set_setting",{key:"private.dir",value:pbox.querySelector("#st-priv").value});pbox.querySelector("#st-priv").value=x;note(pm,"Saved","ok")}
    catch(e){note(pm,"Not saved: "+errText(e),"err")}};
  // Pausing idle sessions, as Kangentic does.
  const p=await invoke("get_setting",{key:"sessions.idle_pause_minutes"});
  const box=document.createElement("div");box.className="st-box";
  box.innerHTML=`<label class="f">Pause a session after it has been idle for (minutes)<input class="tx st-num" id="st-idle" type="number" min="0" max="1440" step="1" value="${esc(p.value)}"></label>
  <p class="meta">A paused session is stopped with its conversation kept, and resumes when you send it a message. 0 never pauses (default ${esc(p.default)}). A card you have open isn't paused.</p>
  <div class="row"><button class="btn" id="st-isave" type="button">Save</button><span class="msg" id="st-imsg"></span></div>`;
  m.appendChild(box);
  const im=box.querySelector("#st-imsg");
  box.querySelector("#st-isave").onclick=async()=>{
    const v=box.querySelector("#st-idle").value.trim();
    if(!/^\d+$/.test(v)||+v>1440){note(im,"Give a whole number from 0 to 1440.","err");return}
    try{const x=await invoke("set_setting",{key:"sessions.idle_pause_minutes",value:v});box.querySelector("#st-idle").value=x;note(im,"Saved","ok")}
    catch(e){note(im,"Not saved: "+errText(e),"err")}};
  // The stuck-session monitor (card #99).
  const k=await invoke("get_setting",{key:"sessions.stuck_check_minutes"});
  const kb=document.createElement("div");kb.className="st-box";kb.dataset.setting="stuck";
  kb.innerHTML=`<label class="f">Look for stuck card sessions every (minutes)<input class="tx st-num" id="st-stuck" type="number" min="0" max="1440" step="1" value="${esc(k.value)}"></label>
  <p class="meta">A session that makes the same call with the same result over and over gets one nudge, and you are told if it is still at it. You are told about a working session with nothing written for 20 minutes. One idle on a subagent that stopped running gets one message at its prompt. A failed one, or one waiting at a question, gets the chip only. Each shows "May be stuck" on its card. 0 turns it off (default ${esc(k.default)}). A dead subagent is caught only if this is shorter than the idle pause above, because a paused session isn't checked.</p>
  <div class="row"><button class="btn" id="st-ksave" type="button">Save</button><span class="msg" id="st-kmsg"></span></div>`;
  m.appendChild(kb);
  const km=kb.querySelector("#st-kmsg");
  kb.querySelector("#st-ksave").onclick=async()=>{
    const v=kb.querySelector("#st-stuck").value.trim();
    if(!/^\d+$/.test(v)||+v>1440){note(km,"Give a whole number from 0 to 1440.","err");return}
    try{const x=await invoke("set_setting",{key:"sessions.stuck_check_minutes",value:v});kb.querySelector("#st-stuck").value=x;note(km,"Saved","ok")}
    catch(e){note(km,"Not saved: "+errText(e),"err")}};
  // The app's subagent rules (cards #76, #96): each an on/off setting, saved at once. One entry each.
  for(const r of SUBAGENT_RULES)await ruleBox(m,r);
  await ruleBox(m,TODO_TOOLS_RULE); // card #106
  await smallReposBox(m);
}

// Small-change auto-ship (card #104): the repos whose small, fully checked cards go on to Ship it
// by themselves. Empty is off. Each row names the checkout and branch that repo's ship pushes.
// Every change is kept in the settings log, shown here, and Hugh is told by a notification.
const SMALL_KEY="sessions.small_goes_on_repos";
async function smallReposBox(m){
  let g=null,err="";try{g=await invoke("get_setting",{key:SMALL_KEY})}catch(e){err=errText(e)}
  let rows=[];try{rows=JSON.parse(g&&g.value||"[]")}catch(e){rows=[]}if(!Array.isArray(rows))rows=[];
  const b=document.createElement("div");b.className="st-box st-small";m.appendChild(b);
  const row=(r,i)=>`<div class="row st-srow" data-i="${i}"><input class="st-sr" placeholder="owner/name" value="${esc(r.repo||"")}" aria-label="Repo">
    <input class="st-sc" placeholder="C:\\Users\\you\\repo" value="${esc(r.checkout||"")}" aria-label="Checkout folder">
    <input class="st-sb" placeholder="main" value="${esc(r.branch||"")}" aria-label="Branch"><button class="btn ghost st-sx" type="button">Remove</button></div>`;
  async function draw(msgText,kind){
    let log=[];try{log=await invoke("get_setting_log",{key:SMALL_KEY,limit:5})}catch(e){}
    b.innerHTML=`<p class="st-radio"><b>Let small, fully checked cards go on to Ship it by themselves</b></p>
    <p class="meta">For the repos listed here, a card whose change is docs (.md, .rst) or CSS only, at most 30 lines in 5 files, nothing about sign-in, data or migrations, with its card-reviewer and card-tester both passed on its final commit, waits 10 minutes in Human test (with a Stop button) and then goes into Ship it with no human test. Each row names the checkout and branch its ship pushes. Empty is off (the default).</p>
    ${rows.map(row).join("")||`<p class="meta">No repos: every card stops in Human test for you.</p>`}
    <div class="row"><button class="btn ghost" id="st-sadd" type="button">Add a repo</button><button class="btn" id="st-ssave" type="button"${g?"":" disabled"}>Save</button><span class="msg" id="st-smsg"></span></div>
    ${log.length?`<p class="meta">Changes:</p><ul class="log">${log.map(x=>{let v=[];try{v=JSON.parse(x.value)}catch(e){}const list=Array.isArray(v)&&v.length?v.map(r=>esc(r.repo)).join(", "):"none (off)";
      return`<li><time>${esc(x.at)}</time>${esc(x.by)}: ${list}</li>`}).join("")}</ul>`:""}`;
    const msg=b.querySelector("#st-smsg");
    if(!g)note(msg,"Couldn't read this setting: "+err,"err");else if(msgText)note(msg,msgText,kind);
    const read=()=>{rows=[...b.querySelectorAll(".st-srow")].map(d=>({repo:d.querySelector(".st-sr").value.trim(),checkout:d.querySelector(".st-sc").value.trim(),branch:d.querySelector(".st-sb").value.trim()}))};
    b.querySelectorAll(".st-sx").forEach(x=>x.onclick=()=>{read();rows.splice(+x.closest(".st-srow").dataset.i,1);draw()});
    b.querySelector("#st-sadd").onclick=()=>{read();rows.push({repo:"",checkout:"",branch:"main"});draw()};
    b.querySelector("#st-ssave").onclick=async()=>{read();
      try{const x=await invoke("set_setting",{key:SMALL_KEY,value:JSON.stringify(rows)});rows=JSON.parse(x);await draw("Saved","ok")}
      catch(e){note(msg,"Not saved: "+errText(e),"err")}};
  }
  await draw();
}

// Card #106: Claude Code's to-do tools for card sessions, so each card shows its step progress.
const TODO_TOOLS_RULE={key:"sessions.todo_tools",label:"Card sessions keep a to-do list",
  help:"Gives card sessions Claude Code's to-do tools (TaskCreate, TaskUpdate), which it no longer offers newer models by itself, so each card shows its progress: \"Step 4 of 7: Writing the tests\" with a bar. It applies to sessions started after the change. Default on."};

const SUBAGENT_RULES=[
  {key:"sessions.test_before_human_test",label:"Test every card with a subagent before Human test",
   help:"A card's session can move it from In progress to Human test only after its card-tester subagent has passed it. The app records each verdict on the card. Default on."},
  {key:"sessions.review_before_human_test",label:"Review every card's diff with a subagent before Human test",
   help:"A card's session can move it from In progress to Human test only after its card-reviewer subagent has passed its diff: mistakes, risky changes, work the card didn't ask for, the repo's hard rules. Its third FAIL sends the card to Blocked. Default on."},
  {key:"sessions.critic_before_build",label:"Critique every plan with a subagent before building",
   help:"At the end of Planning, a card's session has its card-critic subagent check the plan against the card and the code. The session can move the card into In progress only after a pass; your own moves are never held. Its third FAIL sends the card to Blocked. Off: sessions skip the step. Default on."},
  {key:"sessions.small_go_on",label:"Let a card routed small start building once its plan critique passes",
   help:"When the card-router subagent sizes a card small and its card-critic passes the plan, the app moves it from Planning to In progress by itself at the end of the session's turn, and says so in its log. Not a stack's cards. Off: small cards wait for you, like the rest. Default on."},
  {key:"sessions.repro_before_human_test",label:"Prove reported bugs with the reproducer",
   help:"A bug report on a card (Found a problem, Fail in Human test, Report a bug, or a subagent's FAIL that names a bug) has its card-reproducer subagent reproduce the bug before the fix and rerun the same steps after it. The card's session can move it to Human test only once the latest run says Fixed (or Not reproduced). The steps and screenshots are kept on the card. Off: reports are only logged. Default on."},
];

// One on/off rule's checkbox, saved as soon as it changes.
async function ruleBox(m,r){
  let g=null,err="";try{g=await invoke("get_setting",{key:r.key})}catch(e){err=errText(e)}
  const b=document.createElement("div");b.className="st-box";b.dataset.rule=r.key;
  b.innerHTML=`<label class="st-radio"><input type="checkbox"${g&&g.value!=="off"?" checked":""}${g?"":" disabled"}> ${esc(r.label)}</label>
  <p class="meta">${esc(r.help)}</p>
  <div class="row"><span class="msg"></span></div>`;
  m.appendChild(b);
  const msg=b.querySelector(".msg"),cb=b.querySelector("input");
  if(!g)note(msg,"Couldn't read this setting: "+err,"err");
  cb.onchange=async()=>{
    try{const x=await invoke("set_setting",{key:r.key,value:cb.checked?"on":"off"});cb.checked=x!=="off";note(msg,"Saved","ok")}
    catch(e){cb.checked=!cb.checked;note(msg,"Not saved: "+errText(e),"err")}};
}

// Stacks: when a stack (a lead card and its children) moves to Human test.
async function stacks(m){
  const s=await invoke("get_setting",{key:"stacks.human_test_when"});
  const cur=s.value==="first"?"first":"all";
  const radio=(v,label)=>`<label class="st-radio"><input type="radio" name="st-stk" value="${v}"${v===cur?" checked":""}> ${label}</label>`;
  m.innerHTML=`<h3>Stacks</h3>
  <div class="st-box"><fieldset class="st-set"><legend>Move a stack to Human test</legend>
   ${radio("all","when every card is ready")}${radio("first","when the first card is ready")}</fieldset>
  <p class="meta">A card is ready once it is in Human test or later. With "the first card", the stack moves as soon as one is; the rest keep their own state inside it (default: ${s.default==="first"?"the first card":"every card"}).</p>
  <div class="row"><span class="msg" id="st-smsg"></span></div></div>`;
  const msg=m.querySelector("#st-smsg");
  m.querySelectorAll('input[name="st-stk"]').forEach(r=>r.onchange=async()=>{
    try{await invoke("set_setting",{key:"stacks.human_test_when",value:r.value});note(msg,"Saved","ok")}
    catch(e){note(msg,"Not saved: "+errText(e),"err");stacks(m)}});
}

// The Router (card #119): the one long-running session for the whole board. Its model, its
// effort, and whether it may start cards. Each is saved as soon as it changes; a model or effort
// change applies the next time the Router starts.
const ROUTER_MODELS=[["claude-sonnet-5-5","Sonnet 5.5"],["claude-opus-5-5","Opus 5.5"],["claude-haiku-5-5","Haiku 5.5"],["claude-fable-5-1","Fable 5.1"]];
async function router(m){
  const get=async k=>{try{return await invoke("get_setting",{key:k})}catch(e){return {value:"",default:"",err:errText(e)}}};
  const [mo,ef,ms]=await Promise.all([get("router.model"),get("router.effort"),get("router.may_start_cards")]);
  const model=mo.value||mo.default||"claude-sonnet-5-5",effort=ef.value||ef.default||"low",may=(ms.value||ms.default||"off")==="on";
  const models=ROUTER_MODELS.some(([v])=>v===model)?ROUTER_MODELS:[...ROUTER_MODELS,[model,model]];
  m.innerHTML=`<h3>Router</h3>
  <p class="note">The Router is one long-running Claude Code session for the whole board: it routes cards, checks them and sends them back. Open it with the Router button in the header. A model or effort change applies the next time it starts.</p>
  <div class="st-box" data-setting="router"><div class="st-row">
    <label class="f">Model<select class="tx" id="st-rmodel">${models.map(([v,n])=>opt(v,model,n)).join("")}</select></label>
    <label class="f">Effort<select class="colsel" id="st-reffort">${EFFORTS.map(x=>opt(x,effort)).join("")}</select></label></div>
   <p class="meta">Default: Sonnet 5.5, effort low.</p>
   <label class="st-radio"><input type="checkbox" id="st-rmay"${may?" checked":""}> Router may start cards</label>
   <p class="meta">Router may start cards: lets the Router move a card from Backlog to Planning and start a card's session. Off: it routes, checks and sends back only.</p>
   <div class="row"><span class="msg" id="st-rmsg"></span></div></div>`;
  const msg=m.querySelector("#st-rmsg"),err=mo.err||ef.err||ms.err;
  if(err)note(msg,"Couldn't read a Router setting: "+err,"err");
  const save=async(key,value,undo)=>{
    try{await invoke("set_setting",{key,value});note(msg,"Saved","ok")}
    catch(e){undo();note(msg,"Not saved: "+errText(e),"err")}};
  const sm=m.querySelector("#st-rmodel"),se=m.querySelector("#st-reffort"),cb=m.querySelector("#st-rmay");
  let pm=sm.value,pe=se.value;
  sm.onchange=()=>save("router.model",sm.value,()=>{sm.value=pm}).then(()=>{pm=sm.value});
  se.onchange=()=>save("router.effort",se.value,()=>{se.value=pe}).then(()=>{pe=se.value});
  cb.onchange=()=>save("router.may_start_cards",cb.checked?"on":"off",()=>{cb.checked=!cb.checked});
}

// The Add card form's template picker: choosing one fills the title prefix and description
// (only where Hugh hasn't typed his own), and the card keeps the template's id as templateId.
window.templatePicker=async function(sel,title,desc){
  let list=[];try{list=await invoke("list_templates",{archived:false})}catch(e){}
  if(!list.length){sel.closest("label").hidden=true;return}
  sel.innerHTML=`<option value="">No template</option>`+list.map(t=>`<option value="${esc(t.id)}">${esc(t.name)}</option>`).join("");
  let prev=null;
  sel.onchange=()=>{const t=list.find(x=>x.id===sel.value)||{titlePrefix:"",body:""};
    const pt=prev?prev.titlePrefix:"",pb=prev?prev.body:"";
    if(!title.value||title.value===pt)title.value=t.titlePrefix;
    else if(pt&&title.value.startsWith(pt))title.value=t.titlePrefix+title.value.slice(pt.length);
    else if(t.titlePrefix&&!title.value.startsWith(t.titlePrefix))title.value=t.titlePrefix+title.value;
    if(!desc.value.trim()||desc.value===pb)desc.value=t.body;
    prev=sel.value?t:null;title.focus();title.setSelectionRange(title.value.length,title.value.length)};
};

{const g=$("#settingsBtn");if(g){g.hidden=false;g.onclick=()=>openSettings()}}
})();
