// Dev-stack health on a card's login buttons (src-tauri/src/stack.rs): up or down, the branch
// the stack's checkout is on, and when it's down, the command that restarts it. The app never
// runs WSL commands, so "Restart stack" shows the command to paste into a WSL terminal.
window.KBStack=(()=>{
  const call=(cmd,args)=>window.__TAURI__.core.invoke(cmd,args);
  const esc=s=>String(s??"").replace(/[&<>"']/g,c=>({"&":"&amp;","<":"&lt;",">":"&gt;",'"':"&quot;","'":"&#39;"}[c]));
  const opened=new Set();   // card id + url whose restart command Hugh opened; kept across re-renders
  function branchText(h){
    if(!h.branch)return"";
    let t=` · branch <code>${esc(h.branch)}</code>`;
    if(h.branchFrom==="card")t+=` <span class="meta">(from the card)</span>`;
    if(h.cardBranch)t+=` <span class="warn">the card says <code>${esc(h.cardBranch)}</code></span>`;
    return t;
  }
  function paint(row,h,key){
    const old=row.querySelector(".stackst");if(old)old.hidden=true;
    const open=row.querySelector("[data-open]");if(open)open.hidden=!h.up;
    let box=row.querySelector(".stackhealth");
    if(!box){box=document.createElement("div");box.className="stackhealth";row.appendChild(box)}
    if(!h.local){box.hidden=true;return}
    box.hidden=false;
    const r=h.restart;
    box.innerHTML=`<span class="${h.up?"up":"down"}">${h.up?"Stack up":"Stack down: the dev stack isn&#39;t answering"}</span>${branchText(h)}`+
      (r?` <button class="btn ghost sm" type="button" data-restart>Restart stack</button>
        <div class="restartcmd"${opened.has(key)?"":" hidden"}><p class="note">${esc(r.note)}</p>
        <div class="cmdrow"><code class="cmd">${esc(r.command)}</code><button class="btn ghost sm" type="button" data-copycmd>Copy</button></div></div>`:"");
    const btn=box.querySelector("[data-restart]");
    if(btn)btn.onclick=()=>{const c=box.querySelector(".restartcmd");c.hidden=!c.hidden;if(c.hidden)opened.delete(key);else opened.add(key)};
    const cp=box.querySelector("[data-copycmd]");
    if(cp)cp.onclick=()=>{const done=()=>{cp.textContent="Copied"};
      const sel=()=>{const r2=document.createRange();r2.selectNodeContents(box.querySelector(".cmd"));const s=getSelection();s.removeAllRanges();s.addRange(r2);cp.textContent="Selected: press Ctrl+C"};
      try{navigator.clipboard.writeText(r.command).then(done,sel)}catch(e){sel()}};
  }
  async function check(card){
    const rows=[...document.querySelectorAll(".login[data-url]")];
    const seen=new Map();   // one check per url
    for(const row of rows){
      const url=row.dataset.url;
      if(!seen.has(url))seen.set(url,call("stack_health",{url,worktree:card&&card.worktreePath||null,branch:card&&card.branch||null}).catch(()=>null));
      const h=await seen.get(url);
      if(!h||!row.isConnected)continue;
      paint(row,h,(card&&card.id||"")+" "+url);
    }
  }
  return{check};
})();
