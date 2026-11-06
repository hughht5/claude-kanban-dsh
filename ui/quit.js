// Cmd+Q on a Mac with card sessions running (card #117, router decision D4): the app asks here
// before it quits, because quitting suspends every session. The app sends "confirm-quit" only
// on a Mac (src-tauri/src/platform/macos.rs), so on Windows this never shows. The page says at
// once that it showed the question ("quit-shown"); if it doesn't within 3 s, the app asks with
// a native alert instead. The answer goes back as "quit-answer": quit, hide or cancel.
(()=>{
  const T=window.__TAURI__;if(!T)return;
  const send=(ev,payload)=>T.event.emit(ev,payload).catch(()=>{});
  let dlg=null;
  function ask(running){
    send("quit-shown");
    if(dlg){dlg.querySelector(".qn").textContent=label(running);return}
    const d=document.createElement("dialog");
    d.id="quitDlg";d.setAttribute("aria-label","Quit");
    d.innerHTML=`<div class="dlg"><div class="dlg-head"><div class="ttl"><h2>Quit Claude Kanban DSH?</h2></div></div>
      <div class="dlg-body"><p><span class="qn"></span> running. Quitting suspends them, and the ones mid-turn resume when the app opens again.</p>
      <div class="qa"><button class="btn" type="button" data-a="quit">Quit</button><button class="btn ghost" type="button" data-a="hide">Hide window</button><button class="btn ghost" type="button" data-a="cancel">Cancel</button></div></div></div>`;
    d.querySelector(".qn").textContent=label(running);
    const answer=a=>{send("quit-answer",a);dlg=null;if(d.open)d.close();d.remove()};
    d.querySelectorAll("button[data-a]").forEach(b=>b.onclick=()=>answer(b.dataset.a));
    // Esc is Cancel.
    d.addEventListener("cancel",e=>{e.preventDefault();answer("cancel")});
    document.body.appendChild(d);dlg=d;
    try{d.showModal()}catch(e){d.setAttribute("open","")}
    const q=d.querySelector('[data-a="cancel"]');if(q)q.focus();
  }
  function label(n){return n===1?"1 card session is":`${n} card sessions are`}
  T.event.listen("confirm-quit",e=>ask((e.payload&&e.payload.running)||0)).catch(()=>{});
})();
