// Pasting screenshots where text goes: a card's description (the Add card box and the card's
// Summary and Next step), a stack child's "Found a problem" and answer boxes, and the session's
// terminal. The message box has its own attachment strip (panel.js). The image is saved in the
// app's data folder, and its path goes in as text for Claude to read: "[Image: <path>]" in a
// description or a stack box, the bare path in the terminal, where Claude Code turns a pasted
// image path into an attached image.
(function(){
  const imagesOf=e=>[...((e.clipboardData&&e.clipboardData.files)||[])].filter(f=>/^image\//.test(f.type||""));
  const read=f=>new Promise((ok,bad)=>{const r=new FileReader();r.onload=()=>ok(r.result);r.onerror=()=>bad(r.error);r.readAsDataURL(f)});
  async function save(cardId,f){
    if(f.size>25*1024*1024)throw new Error("it is over 25 MB");
    const name=f.name&&f.name!=="image.png"?f.name:`screenshot${f.type==="image/jpeg"?".jpg":".png"}`;
    return window.__TAURI__.core.invoke("attachment_save",{cardId,name,data:await read(f)});
  }
  const errText=e=>typeof e==="string"?e:(e&&e.message)||String(e);
  const say=t=>{try{if(typeof status==="function")status(t)}catch(e){}};

  // Puts text in a textarea at the cursor, as if typed, so its own input handling (drafts,
  // saving) runs.
  function insert(ta,text){
    ta.focus();
    const s=ta.selectionStart??ta.value.length,e=ta.selectionEnd??s;
    const before=ta.value.slice(0,s),after=ta.value.slice(e);
    const pre=before&&!/\n$/.test(before)?"\n":"",post=after&&!/^\n/.test(after)?"\n":"";
    ta.value=before+pre+text+post+after;
    const at=(before+pre+text).length;ta.setSelectionRange(at,at);
    ta.dispatchEvent(new Event("input",{bubbles:true}));
  }

  const DESC={"a-desc":()=>"new-cards","f-summary":()=>typeof panel!=="undefined"&&panel?panel.id:"cards","f-next":()=>typeof panel!=="undefined"&&panel?panel.id:"cards"};
  // A stack child's overlay: the "Found a problem" and answer boxes (stacks.js), keyed by the child's
  // id in data-f. The image is saved under that child, and its path goes to the stack's session
  // with the note.
  const stkBox=t=>t&&t.tagName==="TEXTAREA"&&t.matches(".stk-kdlg textarea[data-f]")?t.dataset.f:null;
  document.addEventListener("paste",async e=>{
    const t=e.target;
    // Descriptions, and a stack child's problem or answer box.
    const kid=stkBox(t);
    if(t&&t.tagName==="TEXTAREA"&&(DESC[t.id]||kid)&&!t.disabled){
      const imgs=imagesOf(e);if(!imgs.length)return;
      if(!e.clipboardData.getData("text/plain"))e.preventDefault();
      const id=kid||DESC[t.id]();
      for(const f of imgs){
        try{
          const p=await save(id,f);
          // The board redraws its stacks on every change, so the box may be a new element by now.
          const box=kid?document.querySelector(`.stk-kdlg textarea[data-f="${CSS.escape(kid)}"]`):t;
          if(box)insert(box,`[Image: ${p}]`);else say("Saved the image, but the box was closed: "+p);
        }catch(err){say("Couldn't paste the image: "+errText(err))}
      }
      return;
    }
    // The terminal: xterm.js reads pastes from its own hidden textarea; an image paste is taken
    // before it sees it, and the saved file's path is pasted into the session instead.
    const term=t&&t.closest&&t.closest(".term");
    if(term&&typeof panel!=="undefined"&&panel&&panel.term){
      const imgs=imagesOf(e);if(!imgs.length)return;
      e.preventDefault();e.stopImmediatePropagation();
      if(!(panel.info&&panel.info.running)){say("Start or resume the session to paste into it.");return}
      const id=panel.id,tm=panel.term;
      for(const f of imgs){
        try{
          const p=await save(id,f);
          if(panel&&panel.id===id&&panel.term===tm)tm.paste(p+" ");
        }catch(err){say("Couldn't paste the image: "+errText(err))}
      }
    }
  },true);
})();
