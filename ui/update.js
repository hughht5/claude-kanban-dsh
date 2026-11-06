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
    // On a Mac the helper records a failed install. It comes first, even with the same update
    // still staged, and stays until OK (card #117); then the update shows as before.
    if(st&&st.failed){
      bar.innerHTML=`<span class="ut"><b>The last update didn't install</b> — ${esc(st.failed)}. This is the version you had before; the helper's log is in the data folder's update/restart.log.</span>
        <span class="ua"><button class="btn ghost" type="button" id="updOk">OK</button></span>`;
      bar.hidden=false;
      bar.querySelector("#updOk").onclick=()=>{call("update_failure_seen").catch(()=>{}).then(()=>call("update_status")).then(render,()=>{})};
      return;
    }
    if(!r){bar.hidden=true;bar.innerHTML="";return}
    bar.innerHTML=`<span class="ut"><b>Update ready</b> — ${esc(r.version)} (${esc(r.commit)}, built ${esc(when(r.built_at))})</span>
      <span class="ua"><button class="btn" type="button" id="updNow">Restart now</button><button class="btn ghost" type="button" id="updLater">Later</button></span>
      <span class="msg" id="updMsg"></span>`;
    bar.hidden=false;
    bar.querySelector("#updNow").onclick=restart;
    bar.querySelector("#updLater").onclick=()=>{const b=r.built_at;bar.hidden=true;call("update_later",{builtAt:b}).catch(()=>{})};
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
