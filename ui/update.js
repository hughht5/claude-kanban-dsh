// One-click updates: a banner along the top of the board when `scripts\install.ps1` has staged
// a newer build (src-tauri/src/update.rs). Restart now suspends the sessions, installs and
// reopens the app in the background; Later hides it until the app restarts or a newer build
// is staged.
(()=>{
  const T=window.__TAURI__;if(!T)return;
  const call=(cmd,args)=>T.core.invoke(cmd,args);
  const esc=s=>String(s??"").replace(/[&<>"']/g,c=>({"&":"&amp;","<":"&lt;",">":"&gt;",'"':"&quot;","'":"&#39;"}[c]));
  const when=t=>{const d=new Date(t);return isNaN(d)?String(t||""):d.toLocaleString(undefined,{day:"numeric",month:"short",hour:"2-digit",minute:"2-digit"})};
  const errText=e=>e&&typeof e==="object"?(e.message||JSON.stringify(e)):String(e);

  const bar=document.createElement("div");
  bar.id="updateBar";bar.className="updatebar";bar.setAttribute("role","status");bar.hidden=true;
  const anchor=document.querySelector("header");
  document.body.insertBefore(bar,anchor||document.body.firstChild);

  let shown=null,busy=false;
  function render(st){
    if(busy)return;
    const r=st&&st.ready;shown=r||null;
    // The helper records a failed install (on a Mac since card #117, on Windows since #134). It
    // comes first, even with the same update still staged, and stays until OK; then the update
    // shows as before. On Windows the update can install with kb.exe held, so still the old one
    // (the installer's exit 3, card #134): not a failure, so worded apart, and also until OK.
    if(st&&st.failed)return notice("install",`<b>The last update didn't install</b> — ${esc(st.failed)}. This is the version you had before`);
    if(st&&st.kb_notice)return notice("kb",`<b>Updated, but kb.exe is still the old version</b> — held by ${esc(st.kb_notice)}. New sessions use the old kb until the next install replaces it`);
    if(!r){bar.hidden=true;bar.innerHTML="";return}
    bar.innerHTML=`<span class="ut"><b>Update ready</b> — ${esc(r.version)} (${esc(r.commit)}, built ${esc(when(r.built_at))})</span>
      <span class="ua"><button class="btn" type="button" id="updNow">Restart now</button><button class="btn ghost" type="button" id="updLater">Later</button></span>
      <span class="msg" id="updMsg"></span>`;
    bar.hidden=false;
    bar.querySelector("#updNow").onclick=restart;
    bar.querySelector("#updLater").onclick=()=>{const b=r.built_at;bar.hidden=true;call("update_later",{builtAt:b}).catch(()=>{})};
  }
  // A notice with OK, which clears that kind only ("install" or "kb"), then shows what is next.
  function notice(kind,html){
    bar.innerHTML=`<span class="ut">${html}; the helper's log is in the data folder's update/restart.log.</span>
        <span class="ua"><button class="btn ghost" type="button" id="updOk">OK</button></span>`;
    bar.hidden=false;
    bar.querySelector("#updOk").onclick=()=>{call("update_failure_seen",{kind}).catch(()=>{}).then(()=>call("update_status")).then(render,()=>{})};
  }
  async function restart(){
    const msg=bar.querySelector("#updMsg");
    bar.querySelectorAll("button").forEach(b=>b.disabled=true);
    busy=true;msg.className="msg";msg.textContent="Suspending the sessions, then installing. The board reopens in the background in a moment.";
    try{await call("update_restart")}
    catch(e){busy=false;msg.className="msg err";msg.textContent="Couldn't restart: "+errText(e);bar.querySelectorAll("button").forEach(b=>b.disabled=false)}
  }
  call("update_status").then(render,()=>{});
  T.event.listen("update-ready",e=>render(e.payload)).catch(()=>{});
})();
