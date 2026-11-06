// The panel header's model and effort switch, and the "Queued (cap N)" badges for starts waiting
// under the cap on running sessions. Uses board.js's helpers (esc, invoke, render, errText) and
// panel.js's (panel, refreshInfo, foot); panel.js calls liveSwitch(info) after each refresh.
const LS_MODELS=["claude-opus-5-5","claude-fable-5-1","claude-sonnet-5-5","claude-haiku-5-5"];
const LS_EFFORTS=["low","medium","high","xhigh","max"];
let startQueue={cap:10,running:0,queued:[]};

// On a card: shown while its start waits for a place under the cap.
function queuedBadge(id){
  const q=startQueue.queued.find(x=>x.card===id);if(!q)return"";
  return`<span class="tag qbadge" title="Waiting to start: number ${q.position} in line. ${startQueue.running} running; it starts when one ends.">Queued (cap ${esc(startQueue.cap)})</span>`;
}

function liveSwitch(info){
  if(!panel)return;const host=panel.el.querySelector("#p-live");if(!host)return;
  const run=info.running;
  const cur={model:(run&&run.model)||info.model,effort:(run&&run.effort)||info.effort};
  const pend=info.switch||{};
  const key=JSON.stringify([panel.id,!!run,cur,pend,info.queued,info.cap]);
  if(host.dataset.key===key||host.contains(document.activeElement))return; // don't close an open dropdown
  host.dataset.key=key;
  const opts=(list,val)=>[...new Set([val,...list].filter(Boolean))].map(v=>`<option value="${esc(v)}"${v===val?" selected":""}>${esc(v)}</option>`).join("");
  const m=pend.model||cur.model,e=pend.effort||cur.effort;
  const when=run?"Typed into the session as /model or /effort when it is next free; the conversation carries on.":"Applies when the card's next session starts.";
  const waiting=[pend.model&&`model ${pend.model}`,pend.effort&&`effort ${pend.effort}`].filter(Boolean).join(", ");
  host.innerHTML=`${info.queued?`<span class="tag qbadge" title="Number ${esc(info.queued)} in line. It starts when a running session ends.">Queued (cap ${esc(info.cap)})</span><button class="ibtn" id="ls-unq" type="button" title="Take this card's start out of the queue">Cancel start</button>`:""}
    <select id="ls-model" class="ls-sel" aria-label="Model" title="Model. ${esc(when)}">${opts(LS_MODELS,m)}</select>
    <select id="ls-effort" class="ls-sel" aria-label="Effort" title="Effort. ${esc(when)}">${opts(LS_EFFORTS,e)}</select>
    ${waiting?`<span class="ls-wait" title="Waiting for the session to be free (after its current turn)">switching to ${esc(waiting)}…</span>`:""}`;
  const set=async(field,value)=>{
    try{const r=await invoke("set_session_model_effort",{cardId:panel.id,model:field==="model"?value:null,effort:field==="effort"?value:null});
      if(r&&r.message)foot(`<span>${esc(r.message)}</span>`);}
    catch(err){foot(`<span class="err">Couldn't change it: ${esc(errText(err))}</span>`)}
    host.dataset.key="";document.activeElement&&document.activeElement.blur&&document.activeElement.blur();
    setTimeout(()=>refreshInfo(),1200); // refresh after the footer message has been read
  };
  host.querySelector("#ls-model").onchange=ev=>set("model",ev.target.value);
  host.querySelector("#ls-effort").onchange=ev=>set("effort",ev.target.value);
  const unq=host.querySelector("#ls-unq");
  if(unq)unq.onclick=async()=>{try{await invoke("session_unqueue",{cardId:panel.id})}catch(err){foot(`<span class="err">${esc(errText(err))}</span>`)}host.dataset.key="";refreshInfo()};
}

(()=>{
  const st=document.createElement("style");
  st.textContent=`.ph .live{display:inline-flex;align-items:center;gap:6px;margin-right:4px}
.ls-sel{font:inherit;font-size:12px;padding:3px 4px;border:1px solid var(--line);border-radius:6px;background:var(--panel);color:var(--fg);max-width:150px}
.ls-wait{font-size:12px;color:var(--muted)}
.tag.qbadge,.ph .qbadge{color:var(--warn);border-color:color-mix(in srgb,var(--warn) 45%,transparent)}`;
  document.head.appendChild(st);
})();

(async()=>{
  try{startQueue=await invoke("session_queue")}catch(e){}
  render();
  window.__TAURI__.event.listen("session-queue",e=>{
    startQueue=e.payload||startQueue;render();
    if(panel&&panel.el.querySelector("#p-live"))refreshInfo();
  });
})();
