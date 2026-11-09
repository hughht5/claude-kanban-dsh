// One xterm.js terminal on one session's PTY (card #95), shared by the card panel (panel.js) and
// the Router pane (router.js).
//
// - It replays the session's output (session_snapshot) once per open, and again only when the
//   session is a new run (session_run), never on every refresh: a replay wipes the screen.
// - Each "pty" event carries the session's output length after it (`end`) and its run. Events
//   that arrive while the snapshot is loading wait, then whatever the snapshot already holds is
//   skipped, so nothing is lost or drawn twice.
// - While a replay draws, xterm's answers to terminal queries in that old output (cursor
//   position, device attributes, colours) are dropped, never typed into the session. Hugh's own
//   keys are kept: typed while the terminal attaches, they go in once it has.
// - A key with no running session, or one the app refuses, shows a short note instead of
//   vanishing.
const termB64=s=>Uint8Array.from(atob(s),c=>c.charCodeAt(0));
// What xterm sends on its own in answer to a query: CSI reports (cursor position R, device
// attributes c, status n, mode $y, window t, keyboard flags u), OSC and DCS replies.
const TERM_REPLY=/^\x1b(?:\[[?>=]?[\d;]*(?:\$y|[cnRtu])|\][\s\S]*|P[\s\S]*)$/;
const termLinks=new Set();
let termPtyOn=false;
function termListen(){
  if(termPtyOn)return;termPtyOn=true;
  window.__TAURI__.event.listen("pty",e=>{const p=e.payload;if(!p)return;for(const L of termLinks)if(L.key===p.card)termOut(L,p)});
}

// `term` is the xterm; `key` the card id or the Router's key; `idle` the line shown when no
// session runs.
function termLink(term,key,idle){
  const L={term,key,idle,run:null,end:0,pending:null,replaying:false,attaching:null,again:false,keys:[],noteAt:0,shownIdle:false,disposed:false};
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
  if(L.disposed)return;
  if(L.replaying&&TERM_REPLY.test(d))return;
  if(L.attaching){L.keys.push(d);return} // typed as the terminal opens: sent once it has attached
  if(L.run==null){termNote(L,L.idle);return}
  window.__TAURI__.core.invoke("session_input",{cardId:L.key,data:d}).catch(e=>{
    const t=typeof e==="string"?e:(e&&e.message)||String(e);
    termNote(L,`Not sent to the session: ${t}`);
    termAttach(L); // perhaps it has ended or restarted
  });
}
function termOut(L,p){
  if(L.disposed)return;
  if(L.pending){L.pending.push(p);return}
  // Not this terminal's run (none yet, or a new one): replay it. If an attach is already under
  // way it looks again when it ends; the chunk is in the snapshot that replay takes.
  if(L.run==null||(p.run&&p.run!==L.run)){if(L.attaching)L.again=true;else termAttach(L);return}
  // The same run: draw it, even while a refresh checks the run (the end count stops doubles).
  if(p.end!=null&&p.end<=L.end)return;
  if(p.end!=null)L.end=p.end;
  L.term.write(termB64(p.data));
}
function termIdle(L){
  if(L.run!=null||!L.shownIdle){L.term.reset();L.term.write(L.idle+"\r\n");L.shownIdle=true}
  L.run=null;
}

// Attaches to the session if it is a run this terminal hasn't replayed yet. Cheap when nothing
// changed (one session_run call), so callers may call it on every refresh.
function termAttach(L){
  if(L.disposed)return Promise.resolve();
  if(L.attaching){L.again=true;return L.attaching}
  L.attaching=(async()=>{
    const inv=window.__TAURI__.core.invoke;
    try{
      const run=await inv("session_run",{cardId:L.key}).catch(()=>null);
      if(L.disposed)return;
      if(!run){termIdle(L);return}
      if(run===L.run)return;
      L.pending=[];
      await inv("session_resize",{cardId:L.key,cols:L.term.cols,rows:L.term.rows}).catch(()=>{});
      const s=await inv("session_snapshot",{cardId:L.key}).catch(()=>null);
      const waiting=L.pending;L.pending=null;
      if(L.disposed)return;
      if(!s){termIdle(L);return}
      L.run=s.run;L.end=s.end;L.shownIdle=false;
      L.term.reset();
      if(s.data){L.replaying=true;L.term.write(termB64(s.data),()=>{L.replaying=false})}
      L.attaching=null;
      for(const p of waiting)if(!p.run||p.run===L.run)termOut(L,p);
    }finally{L.attaching=null}
  })().then(()=>{
    if(L.disposed)return;
    if(L.again){L.again=false;return termAttach(L)}
    const keys=L.keys.splice(0);for(const d of keys)termInput(L,d);
  });
  return L.attaching;
}
