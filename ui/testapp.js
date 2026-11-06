// Turns on the shared test app (card #115). The app gives one card session at a time the test
// app; the rest wait in line (src-tauri). This file draws what the "testapp" event and
// testapp_status say: a "Test app" chip in the header that opens a popover (the holder, the line,
// an install, held restarts, the old DSH lock folder), a banner when a test-app restart is
// pending, and a chip on each card that holds or waits for a turn. In the test app it also tells
// the app, at most every 30 s, that Hugh is using it (testapp_input). An older backend without
// these commands shows nothing. Nothing here focuses the window or opens a toast.
(()=>{
  const T=window.__TAURI__;if(!T)return;
  const call=cmd=>T.core.invoke(cmd);
  const esc=s=>String(s??"").replace(/[&<>"']/g,c=>({"&":"&amp;","<":"&lt;",">":"&gt;",'"':"&quot;","'":"&#39;"}[c]));
  const errText=e=>e&&typeof e==="object"?(e.message||JSON.stringify(e)):String(e);
  const hhmm=t=>{const d=new Date(t);return t&&!isNaN(d)?d.toLocaleTimeString("en-GB",{hour:"2-digit",minute:"2-digit"}):""};
  const ord=n=>{n=Number(n);if(!Number.isFinite(n)||n<1)return"";const t=n%100,o=n%10;
    return n+(t>=11&&t<=13?"th":o===1?"st":o===2?"nd":o===3?"rd":"th")};
  const KIND={tester:"testing",install:"install"};
  const PHASE={waiting:"waiting",installing:"installing",started:"started"};
  const nameOf=x=>x?String(x.label||(x.card!=null&&x.card!==""?"#"+String(x.card).replace(/^#/,""):"")||x.owner||"a card"):"";
  const reasonOf=w=>{if(w==null||w==="")return"";if(Array.isArray(w))return w.map(reasonOf).filter(Boolean).join(", ");
    if(typeof w==="object")return nameOf(w);return String(w)};
  const future=t=>{const n=Date.parse(t||"");return Number.isFinite(n)&&n>Date.now()};

  let st=null;           // the latest status, or null (none yet, or an older backend)
  let open=false;        // the popover is open
  let confirmAt={};      // {holder|mkdir: time of the first click}: a second click within 5 s releases
  let busy=false,msg="",msgErr=false,barMsg="",refocus=null;   // refocus: {where, act} after an action redraws

  // The header chip and its popover, before the filter.
  const header=document.querySelector("header");
  const wrap=document.createElement("span");wrap.className="tapwrap";wrap.hidden=true;
  wrap.innerHTML=`<button type="button" class="tapchip" id="testappChip" aria-haspopup="dialog" aria-expanded="false" aria-controls="testappPop"></button>
    <div class="tappop" id="testappPop" role="dialog" aria-label="Test app turns" hidden></div>`;
  if(header){const f=header.querySelector(".filter");header.insertBefore(wrap,f||null)}
  const chip=wrap.querySelector("#testappChip"),pop=wrap.querySelector("#testappPop");

  // The banner, with the other banners above the header.
  const bar=document.createElement("div");
  bar.id="testappBar";bar.className="updatebar testappbar";bar.setAttribute("role","status");bar.hidden=true;
  document.body.insertBefore(bar,header||document.body.firstChild);

  const held=s=>!!(s&&s.hold&&(!s.hold.until||future(s.hold.until)));
  const line=s=>s&&Array.isArray(s.line)?s.line:[];
  const anything=s=>!!(s&&(s.holder||line(s).length||s.install||s.pending||s.hold||s.mkdir));
  const heldFor=h=>{const g=Date.parse(h.givenAt||"");if(Number.isFinite(g))return Math.max(0,Math.floor((Date.now()-g)/60000));
    return Number.isFinite(Number(h.heldMinutes))?Number(h.heldMinutes):null};

  function chipText(s){
    const n=line(s).length,more=n?` · ${n} waiting`:"";
    const i=s.install,h=s.holder;
    if(i&&i.phase==="installing")return`Test app: ${nameOf(i)} installing`+more;
    if(h)return`Test app: ${nameOf(h)}`+more;
    if(i)return`Test app: ${nameOf(i)} ${PHASE[i.phase]||i.phase||""}`.trim()+more;
    if(n)return`Test app: ${n} waiting`;
    if(s.pending)return"Test app: restart pending";
    if(s.hold)return"Test app: restarts held";
    if(s.mkdir)return"Test app: old lock folder";
    return"Test app";
  }

  function drawChip(){
    const s=st;
    if(!anything(s)){wrap.hidden=true;if(open)close(false);return}
    wrap.hidden=false;
    chip.textContent=chipText(s);
    const warn=!!((s.holder&&s.holder.overdue)||(s.mkdir&&s.mkdir.stale));
    chip.classList.toggle("warn",warn);
    chip.title=warn?"Test app turns: something needs a look. Click for details.":"Test app turns. Click for details.";
  }

  const btn=(act,text,cls)=>`<button type="button" class="btn sm${cls?" "+cls:""}" data-act="${esc(act)}"${busy?" disabled":""}>${esc(text)}</button>`;
  const armed=k=>confirmAt[k]&&Date.now()-confirmAt[k]<5000;

  function drawPop(){
    if(!open||!st)return;
    const s=st,rows=[];
    const ae=document.activeElement,keep=ae&&pop.contains(ae)&&ae.dataset&&ae.dataset.act?ae.dataset.act:refocus&&refocus.where==="pop"?refocus.act:null;
    const h=s.holder;
    if(h){
      const m=heldFor(h),ends=hhmm(h.expiresAt);
      rows.push(`<div class="taprow${h.overdue?" warn":""}"><div class="tapmain"><span class="tapk">Has the test app</span>
        <span><b>${esc(nameOf(h))}</b> · ${esc(KIND[h.kind]||h.kind||"")}${m!=null?` · held for ${esc(m)} min`:""}${ends?` · ends at ${esc(ends)}`:""}${h.overdue?` · <span class="tapw">overdue</span>`:""}</span></div>
        ${btn("release",armed("holder")?`Release ${nameOf(h)}?`:"Release",armed("holder")?"danger":"ghost")}</div>`);
    }
    const ln=line(s);
    if(ln.length)rows.push(`<div class="taprow"><div class="tapmain"><span class="tapk">Waiting</span><ol class="tapline">${ln.map((x,i)=>
      `<li><span class="tapplace">${esc(ord(x.place!=null?x.place:i+1))}</span><span>${esc(nameOf(x))} · ${esc(KIND[x.kind]||x.kind||"")}${x.live===false?` · <span class="tapw">waits for its session</span>`:""}</span></li>`).join("")}</ol></div></div>`);
    const i=s.install;
    if(i){const w=reasonOf(i.waitingFor),r=hhmm(i.restartAt);
      rows.push(`<div class="taprow"><div class="tapmain"><span class="tapk">Install</span>
        <span><b>${esc(nameOf(i))}</b> · ${esc(PHASE[i.phase]||i.phase||"")}${w?` · waiting for ${esc(w)}`:""}${r?` · restarts at ${esc(r)}`:""}</span></div></div>`)}
    if(held(s))rows.push(`<div class="taprow"><div class="tapmain"><span class="tapk">Restarts</span><span>Restarts held until ${esc(hhmm(s.hold.until))}</span></div>
      <span class="tapacts">${btn("hold","Hold 30 min more","ghost")}${btn("letrestart","Let it restart","ghost")}</span></div>`);
    else rows.push(`<div class="taprow"><div class="tapmain"><span class="tapk">Restarts</span><span>${future(s.allowUntil)?`Allowed now, until ${esc(hhmm(s.allowUntil))}`:s.pending?"A restart is pending":"Not held"}</span></div>
      ${btn("hold","Hold restarts 30 min","ghost")}</div>`);
    const k=s.mkdir;
    if(k)rows.push(`<div class="taprow${k.stale?" warn":""}"><div class="tapmain"><span class="tapk">DSH lock</span>
      <span>Old DSH lock folder: owner ${esc(k.owner||"unknown")}, ${esc(Number.isFinite(Number(k.ageMinutes))?Number(k.ageMinutes):"?")} min${k.stale?` · <span class="tapw">stale</span>`:""}</span></div>
      ${btn("mkdir",armed("mkdir")?"Release the folder?":"Release",armed("mkdir")?"danger":"ghost")}</div>`);
    if(s.hughInputAt)rows.push(`<div class="tapnote">Your last input in the test app: ${esc(hhmm(s.hughInputAt))}</div>`);
    pop.innerHTML=`<div class="taphead">Test app turns${Number.isFinite(Number(s.limitMinutes))?` <span class="tapnote">· ${esc(s.limitMinutes)} min a turn</span>`:""}</div>${rows.join("")}
      <div class="msg${msgErr?" err":""}" id="testappMsg">${esc(msg)}</div>`;
    if(keep){const b=pop.querySelector(`[data-act="${CSS.escape(keep)}"]`);if(b&&!b.disabled)b.focus()}
    // Keep the popover inside the window at narrow widths.
    pop.style.left="0px";const r=pop.getBoundingClientRect(),over=r.right-(window.innerWidth-16);
    if(over>0)pop.style.left=Math.min(0,-over)+"px";
  }

  function drawBar(){
    const s=st;
    if(!s||!s.pending){bar.hidden=true;bar.innerHTML="";barMsg="";return}
    const ae=document.activeElement,keep=ae&&bar.contains(ae)&&ae.dataset&&ae.dataset.act?ae.dataset.act:refocus&&refocus.where==="bar"?refocus.act:null;
    const w=s.install?reasonOf(s.install.waitingFor):"";
    const h=held(s);
    bar.innerHTML=`<span class="ut"><b>Test app restart pending</b> (${esc(s.pending.label||"an install")})${w?` · waiting for ${esc(w)}`:""}${h?` · Held until ${esc(hhmm(s.hold.until))}`:""}</span>
      <span class="ua">${btn("hold",h?"Hold 30 min more":"Hold 30 min","")}${h?btn("letrestart","Let it restart","ghost"):""}</span>
      <span class="msg${barMsg?" err":""}">${esc(barMsg)}</span>`;
    bar.hidden=false;
    if(keep){const b=bar.querySelector(`[data-act="${CSS.escape(keep)}"]`);if(b&&!b.disabled)b.focus()}
  }

  // Card tiles: the holder's card says "Test app"; a waiting card its place in the line.
  function tileChips(){
    const m=new Map(),s=st;
    if(!s)return m;
    const idOf=x=>{if(x.cardId)return String(x.cardId);
      const n=Number(String(x.card??"").replace(/^#/,""));
      if(n>0&&typeof cards!=="undefined"&&Array.isArray(cards)){const c=cards.find(c=>c.number===n);if(c)return String(c.id)}
      return null};
    line(s).forEach((x,i)=>{const id=idOf(x);if(!id||m.has(id))return;
      m.set(id,x.live===false?{t:"Test app: turn waits for its session",c:"tap-paused"}:{t:"Test app: "+ord(x.place!=null?x.place:i+1),c:"tap-wait"})});
    if(s.holder){const id=idOf(s.holder);if(id)m.set(id,{t:"Test app",c:"tap-hold"+(s.holder.overdue?" tap-over":"")})}
    return m;
  }
  // Idempotent: it changes a tile only when its chip is wrong, so the observer settles at once.
  function decorate(){
    const board=document.getElementById("board");if(!board)return;
    const want=tileChips();
    for(const tile of board.querySelectorAll(".card[data-id]")){
      const w=want.get(tile.dataset.id),have=tile.querySelector(":scope .tapcard");
      if(!w){if(have)have.remove();continue}
      const cls="tag chip tapcard "+w.c;
      if(have&&have.textContent===w.t&&have.className===cls)continue;
      if(have)have.remove();
      const el=document.createElement("span");el.className=cls;el.textContent=w.t;
      el.title=w.c.startsWith("tap-hold")?"This card's session has the test app now.":
        w.c==="tap-paused"?"This card is in line for the test app, but its session is paused: the turn waits for it.":"This card is in line for the test app.";
      const host=tile.querySelector(":scope > .tags")||tile.querySelector(":scope .stk-sub")||tile;
      host.prepend(el);
    }
  }
  {const board=document.getElementById("board");
   if(board)new MutationObserver(()=>decorate()).observe(board,{childList:true,subtree:true})}

  function draw(){drawChip();drawPop();drawBar();decorate()}
  function take(s){if(s&&typeof s==="object"){st=s;draw()}}

  async function act(name,where){
    const cmd={release:"testapp_release",mkdir:"testapp_release_mkdir",hold:"testapp_hold",letrestart:"testapp_let_restart"}[name];
    if(!cmd||busy)return;
    if(name==="release"||name==="mkdir"){const k=name==="release"?"holder":"mkdir";
      if(!armed(k)){confirmAt[k]=Date.now();drawPop();setTimeout(()=>{if(confirmAt[k]&&!armed(k)){delete confirmAt[k];drawPop()}},5100);return}
      delete confirmAt[k]}
    busy=true;msg="";msgErr=false;barMsg="";refocus={where,act:name};draw();
    try{const s=await call(cmd);busy=false;take(s);if(!s)draw()}
    catch(e){busy=false;const t="Couldn't do that: "+errText(e);if(where==="bar")barMsg=t;else{msg=t;msgErr=true}draw()}
    refocus=null;
  }

  function openPop(){open=true;msg="";msgErr=false;confirmAt={};pop.hidden=false;chip.setAttribute("aria-expanded","true");drawPop()}
  function close(back){open=false;confirmAt={};pop.hidden=true;pop.innerHTML="";chip.setAttribute("aria-expanded","false");if(back&&!wrap.hidden)chip.focus()}
  chip.addEventListener("click",()=>open?close(false):openPop());
  pop.addEventListener("click",e=>{const b=e.target.closest("[data-act]");if(b)act(b.dataset.act,"pop")});
  bar.addEventListener("click",e=>{const b=e.target.closest("[data-act]");if(b)act(b.dataset.act,"bar")});
  document.addEventListener("pointerdown",e=>{if(open&&!wrap.contains(e.target))close(false)},true);
  document.addEventListener("keydown",e=>{if(open&&e.key==="Escape"){e.preventDefault();e.stopPropagation();close(wrap.contains(document.activeElement))}},true);
  // "held for N min" and the hold's end move on with the clock.
  setInterval(()=>{if(st&&(open||!bar.hidden))draw()},30000);

  // Hugh's input, in the test app only: a real key, press or wheel while this window has focus,
  // outside the banner and the popover (pressing those is not testing the app).
  call("app_channel").then(ch=>{
    if(ch!=="test")return;
    let last=0;
    const seen=e=>{
      if(!e.isTrusted||!document.hasFocus())return;
      const t=e.target;if(t&&t.nodeType===1&&(bar.contains(t)||wrap.contains(t)))return;
      const now=Date.now();if(now-last<30000)return;last=now;
      call("testapp_input").catch(()=>{});
    };
    for(const ev of ["keydown","pointerdown","wheel"])window.addEventListener(ev,seen,{capture:true,passive:true});
  },()=>{});

  call("testapp_status").then(take,()=>{st=null;draw()});
  T.event.listen("testapp",e=>take(e.payload)).catch(()=>{});
})();
