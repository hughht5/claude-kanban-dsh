// The resume offer: a bar along the top of the board listing the sessions that were running
// (idle) when the app last quit or crashed, with Resume all and Not now. Mid-turn sessions
// resume by themselves (restart recovery), so they aren't listed. A card resumed by hand, or
// moved to Done or archived, drops off; the bar goes when none is left. It stays across
// restarts until answered (`resume_offer` and `resume_offer_answer`, src-tauri/src/live_cmds.rs).
(()=>{
  const T=window.__TAURI__;if(!T)return;
  const call=(cmd,args)=>T.core.invoke(cmd,args);
  const esc=s=>String(s??"").replace(/[&<>"']/g,c=>({"&":"&amp;","<":"&lt;",">":"&gt;",'"':"&quot;","'":"&#39;"}[c]));
  const errText=e=>e&&typeof e==="object"?(e.message||JSON.stringify(e)):String(e);

  const bar=document.createElement("div");
  bar.id="resumeBar";bar.className="updatebar resumebar";bar.setAttribute("role","status");bar.hidden=true;
  const anchor=document.querySelector("header");
  document.body.insertBefore(bar,anchor||document.body.firstChild);

  let busy=false,timer=null;
  function render(list){
    if(busy)return;
    if(!Array.isArray(list)||!list.length){bar.hidden=true;bar.innerHTML="";return}
    const n=list.length;
    const names=list.map(c=>`<a href="#" data-card="${esc(c.card)}" title="Open #${esc(c.number)} (${esc(c.column)})">#${esc(c.number)} ${esc(c.title||"")}</a>`).join(", ");
    bar.innerHTML=`<span class="ut"><b>${n} session${n===1?" was":"s were"} running when the app closed</b>: <span class="rnames">${names}</span></span>
      <span class="ua"><button class="btn" type="button" id="resAll">Resume ${n===1?"it":"all"}</button><button class="btn ghost" type="button" id="resNot">Not now</button></span>
      <span class="msg" id="resMsg"></span>`;
    bar.hidden=false;
    bar.querySelectorAll("a[data-card]").forEach(a=>a.onclick=e=>{e.preventDefault();if(typeof openPanel==="function")openPanel(a.dataset.card)});
    bar.querySelector("#resAll").onclick=()=>answer(true);
    bar.querySelector("#resNot").onclick=()=>answer(false);
  }
  async function answer(resume){
    const msg=bar.querySelector("#resMsg");
    bar.querySelectorAll("button").forEach(b=>b.disabled=true);
    busy=true;
    try{
      const n=await call("resume_offer_answer",{resume});
      busy=false;
      if(resume&&n){bar.hidden=true;bar.innerHTML=""}
      refresh();
    }catch(e){
      busy=false;msg.className="msg err";msg.textContent="Couldn't answer: "+errText(e);
      bar.querySelectorAll("button").forEach(b=>b.disabled=false);
    }
  }
  function refresh(){call("resume_offer").then(render,()=>{})}
  // Board and session changes come in bursts: one look after they settle.
  const soon=()=>{clearTimeout(timer);timer=setTimeout(refresh,400)};
  refresh();
  ["cards-changed","session-changed","session-queue","resume-offer"].forEach(ev=>T.event.listen(ev,soon).catch(()=>{}));
})();
