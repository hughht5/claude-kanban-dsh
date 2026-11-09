// One xterm.js terminal on one session's PTY (card #95), shared by the card panel (panel.js) and
// the Router pane (router.js).
//
// - It replays the session's output (session_snapshot) once per open, and again only when the
//   session is a new run (session_run), never on every refresh: a replay wipes the screen.
// - Each "pty" event carries the session's output length after it (`end`) and its run. Events
//   that arrive while the snapshot is loading wait, then whatever the snapshot already holds is
//   skipped, so nothing is lost or drawn twice.
// - While a replay draws, xterm's answers to terminal queries in that old output (cursor
//   position, device attributes, colours) are dropped, never typed into the session.
// - A key with no running session, or one the app refuses, shows a short note instead of
//   vanishing.
const termB64=s=>Uint8Array.from(atob(s),c=>c.charCodeAt(0));
const termLinks=new Set();
let termPtyOn=false;
function termListen(){
  if(termPtyOn)return;termPtyOn=true;
  window.__TAURI__.event.listen("pty",e=>{const p=e.payload;if(!p)return;for(const L of termLinks)if(L.key===p.card)termOut(L,p)});
}

// `term` is the xterm; `key` the card id or the Router's key; `idle` the line shown when no
// session runs.
function termLink(term,key,idle){
  const L={term,key,idle,run:null,end:0,pending:null,replaying:false,attaching:null,noteAt:0,disposed:false};
  termLinks.add(L);termListen();
  term.onData(d=>termInput(L,d));
  return L;
}
function termUnlink(L){if(!L)return;L.disposed=true;termLinks.delete(L)}

function termNote(L,text){
  const now=Date.now();if(now-L.noteAt<4000)return;L.noteAt=now;
  L.term.write(`\r\n\x1b[2m[${text}]\x1b[0m\r\n`);
}
function termInput(L,d){
  if(L.disposed||L.replaying)return;
  if(L.run==null){termNote(L,L.idle);return}
  window.__TAURI__.core.invoke("session_input",{cardId:L.key,data:d}).catch(e=>{
    const t=typeof e==="string"?e:(e&&e.message)||String(e);
    termNote(L,`Not sent to the session: ${t}`);
    if(/no running session/i.test(t))termAttach(L);
  });
}
function termOut(L,p){
  if(L.disposed)return;
  if(L.pending){L.pending.push(p);return}
  if(L.run==null||(p.run&&p.run!==L.run)){termAttach(L);return} // a new run: replay it
  if(p.end!=null&&p.end<=L.end)return;
  if(p.end!=null)L.end=p.end;
  L.term.write(termB64(p.data));
}

// Attaches to the session if it is a run this terminal hasn't replayed yet. Cheap when nothing
// changed (one session_run call), so callers may call it on every refresh. `force` replays anyway.
function termAttach(L,force){
  if(L.disposed)return Promise.resolve();
  if(L.attaching)return L.attaching;
  L.attaching=(async()=>{
    const inv=window.__TAURI__.core.invoke;
    try{
      const run=await inv("session_run",{cardId:L.key}).catch(()=>null);
      if(L.disposed)return;
      if(!run){
        if(L.run!=null||force||!L.shownIdle){L.term.reset();L.term.write(L.idle+"\r\n");L.shownIdle=true}
        L.run=null;return;
      }
      if(run===L.run&&!force)return;
      L.pending=[];
      await inv("session_resize",{cardId:L.key,cols:L.term.cols,rows:L.term.rows}).catch(()=>{});
      const s=await inv("session_snapshot",{cardId:L.key}).catch(()=>null);
      const waiting=L.pending;L.pending=null;
      if(L.disposed)return;
      if(!s){L.run=null;return}
      L.run=s.run;L.end=s.end;L.shownIdle=false;
      L.term.reset();
      if(s.data){L.replaying=true;L.term.write(termB64(s.data),()=>{L.replaying=false})}
      for(const p of waiting)if(!p.run||p.run===L.run)termOut(L,p);
    }finally{L.attaching=null}
  })();
  return L.attaching;
}
