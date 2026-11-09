// Test apps (cards #115, #147). Each card that takes a test-app turn gets a test app of its own (a
// slot: Test, Test 2, ...), and each slot keeps its own turn line (src-tauri). This file draws what
// the "testapp" event and testapp_status say: a "Test apps" chip in the header that opens a popover
// (a row per slot in use: its card, turn, line and install, with Release and Free; the cards waiting
// for a test app; held restarts; the old DSH lock folder), a banner when a test-app restart is
// pending. A card's tile shows its test app on its status chip (doing.js, card #150), which asks
// KBTestApp.tileChip and reads the status through window.testappState, told of a change by the
// document's "testapp-status" event. In every test app it also tells the app, at most every 30 s, that Hugh is using it
// (testapp_input). An older backend without these commands shows nothing. Nothing here focuses the
// window or opens a toast. The pure parts (KBTestApp) are set before the Tauri check, for tests.
(()=>{
  const esc=s=>String(s??"").replace(/[&<>"']/g,c=>({"&":"&amp;","<":"&lt;",">":"&gt;",'"':"&quot;","'":"&#39;"}[c]));
  const ord=n=>{n=Number(n);if(!Number.isFinite(n)||n<1)return"";const t=n%100,o=n%10;
    return n+(t>=11&&t<=13?"th":o===1?"st":o===2?"nd":o===3?"rd":"th")};
  const KIND={tester:"testing",install:"install"};
  const PHASE={waiting:"waiting",installing:"installing",started:"started"};
  const nameOf=x=>x?String(x.label||(x.card!=null&&x.card!==""&&typeof x.card!=="object"?"#"+String(x.card).replace(/^#/,""):"")||(x.number?"#"+x.number:"")||x.owner||"a card"):"";
  const numOf=x=>{const n=Number(String(x??"").replace(/^#/,""));return Number.isInteger(n)&&n>0?n:null};
  // A status entry (a holder, a place in a line) is this card's when its cardId or number matches.
  const isCard=(x,c)=>!!(x&&c&&((x.cardId!=null&&x.cardId!==""&&String(x.cardId)===String(c.id))||(numOf(x.card)!=null&&numOf(x.card)===Number(c.number))));
  const slotLabel=n=>{n=Number(n);return n===1?"Test":n>1?"Test "+n:"Test app"};
  const labelOf=a=>a&&(a.label||slotLabel(a.slot))||"Test app";
  const appName=a=>a&&a.name||"Claude Kanban DSH "+labelOf(a);
  const portOf=a=>a&&(Number(a.port)>0?Number(a.port):Number(a.slot)>0?9333+Number(a.slot):null);
  const lineOf=x=>x&&Array.isArray(x.line)?x.line:[];
  const slotLine=s=>s&&Array.isArray(s.slotLine)?s.slotLine:[];
  const placeOf=(x,i)=>ord(x&&x.place!=null?x.place:i+1);
  // The slots in use. An older status (card #115: one test app, with its holder, line and install
  // at the top) reads as slot 1.
  const slotsOf=s=>{if(!s||typeof s!=="object")return[];if(Array.isArray(s.slots))return s.slots.filter(x=>x&&typeof x==="object");
    return s.holder||lineOf(s).length||s.install?[{slot:1,channel:"test",label:"Test",name:"Claude Kanban DSH Test",port:9334,card:null,
      holder:s.holder||null,line:lineOf(s),install:s.install||null,pending:s.pending||null,hughInputAt:s.hughInputAt||null}]:[]};

  // The chip on a card's tile, or null: {t: text, c: its class, title}. A card with a test app of its
  // own names it, with "testing" while its session has that app's turn or its place in that app's
  // line; a card waiting for a test app to come free its place; a stack's child its lead's test app.
  function tileChip(c,all,s){
    if(!c||typeof c!=="object")return null;
    const slots=slotsOf(s),a=c.testApp&&typeof c.testApp==="object"?c.testApp:null,mine=x=>isCard(x,c);
    const wl=slotLine(s),wi=wl.findIndex(mine);
    if(!a&&wi>=0){const x=wl[wi],p=placeOf(x,wi);
      return{t:`Test app: ${p} in line`,c:x.live===false?"tap-paused":"tap-wait",
        title:`Every test app is taken: this card waits for one to come free, ${p} in line.`+(x.live===false?" Its session is paused: its place waits for it.":"")}}
    // Its own test app's slot, or (an older status) a slot it holds a turn on or waits in line for.
    const sl=a?slots.find(x=>(a.slot!=null&&Number(x.slot)===Number(a.slot))||(!!a.channel&&x.channel===a.channel))||null
      :slots.find(x=>mine(x.holder))||slots.find(x=>lineOf(x).some(mine))||null;
    if(a||sl){
      const app=a||sl,L=labelOf(app),port=portOf(app),h=sl&&sl.holder,ln=lineOf(sl),i=ln.findIndex(mine);
      const where=`${appName(app)}${port?` (CDP port ${port})`:""}`;
      let t=L,cls="tap-own",title=a?`This card's build goes in ${where}.`:`This card's session has a turn on ${where}.`;
      if(h&&mine(h)){const tester=h.kind!=="install";t+=tester?" · testing":" · installing";cls="tap-hold"+(h.overdue?" tap-over":"");
        if(a)title+=tester?" Its session has the turn now.":" Its session has an install turn now.";
        if(h.waking)title+=" Its session was stopped: the app is starting it.";
        if(h.overdue)title+=" The turn is overdue."}
      else if(i>=0){const p=placeOf(ln[i],i),waking=!!ln[i].waking,paused=!waking&&ln[i].live===false;t+=waking?" · starting":` · ${p} in line`;cls=paused?"tap-paused":"tap-wait";
        title=(a?title+` Its session is ${p} in line for a turn`:`This card's session is ${p} in line for a turn on ${where}`)+(waking?": its turn has come, and the app is starting its stopped session.":paused?", but paused: the turn waits for it.":".")}
      return{t,c:cls,title};
    }
    const k=numOf(c.stackOf);
    if(k&&Array.isArray(all)){const lead=all.find(x=>x&&Number(x.number)===k),la=lead&&lead.testApp&&typeof lead.testApp==="object"?lead.testApp:null;
      if(la)return{t:`${labelOf(la)} · stack`,c:"tap-stack",title:`Hugh tests this stack in ${appName(la)}, on lead #${k}'s install.`}}
    return null;
  }

  // The header chip's text.
  function headText(s){
    const sl=slotsOf(s),n=sl.filter(x=>x.card).length,w=slotLine(s).length,more=w?` · ${w} waiting`:"";
    if(n)return`Test apps: ${n} in use`+more;
    const i=sl.map(x=>x.install).find(Boolean),h=sl.map(x=>x.holder).find(Boolean);
    if(i&&i.phase==="installing")return`Test apps: ${nameOf(i)} installing`+more;
    if(h)return`Test apps: ${nameOf(h)} ${KIND[h.kind]||h.kind||""}`.trim()+more;
    if(i)return`Test apps: ${nameOf(i)} ${PHASE[i.phase]||i.phase||""}`.trim()+more;
    if(w)return`Test apps: ${w} waiting`;
    if(s&&s.pending)return"Test apps: restart pending";
    if(s&&s.hold)return"Test apps: restarts held";
    if(s&&s.mkdir)return"Test apps: old lock folder";
    if(sl.some(x=>x.otherBoard))return"Test apps: another board's turn";
    return"Test apps";
  }
  window.KBTestApp={tileChip,headText,slotsOf};

  const T=window.__TAURI__;if(!T)return;
  const call=(cmd,args)=>T.core.invoke(cmd,args);
  const errText=e=>e&&typeof e==="object"?(e.message||JSON.stringify(e)):String(e);
  const hhmm=t=>{const d=new Date(t);return t&&!isNaN(d)?d.toLocaleTimeString("en-GB",{hour:"2-digit",minute:"2-digit"}):""};
  const reasonOf=w=>{if(w==null||w==="")return"";if(Array.isArray(w))return w.map(reasonOf).filter(Boolean).join(", ");
    if(typeof w==="object")return nameOf(w);return String(w)};
  const future=t=>{const n=Date.parse(t||"");return Number.isFinite(n)&&n>Date.now()};

  let st=null;           // the latest status, or null (none yet, or an older backend)
  let open=false;        // the popover is open
  let confirmAt={};      // {release-N|free-N|mkdir: time of the first click}: a second click within 5 s acts
  let busy=false,msg="",msgErr=false,barMsg="",refocus=null;   // refocus: {where, act} after an action redraws

  // The header chip and its popover, before the filter.
  const header=document.querySelector("header");
  const wrap=document.createElement("span");wrap.className="tapwrap";wrap.hidden=true;
  wrap.innerHTML=`<button type="button" class="tapchip" id="testappChip" aria-haspopup="dialog" aria-expanded="false" aria-controls="testappPop"></button>
    <div class="tappop" id="testappPop" role="dialog" aria-label="Test apps" hidden></div>`;
  if(header){const f=header.querySelector(".filter");header.insertBefore(wrap,f||null)}
  const chip=wrap.querySelector("#testappChip"),pop=wrap.querySelector("#testappPop");

  // The banner, with the other banners above the header.
  const bar=document.createElement("div");
  bar.id="testappBar";bar.className="updatebar testappbar";bar.setAttribute("role","status");bar.hidden=true;
  document.body.insertBefore(bar,header||document.body.firstChild);

  const held=s=>!!(s&&s.hold&&(!s.hold.until||future(s.hold.until)));
  const anything=s=>!!(s&&(slotsOf(s).length||slotLine(s).length||s.pending||s.hold||s.mkdir));
  const heldFor=h=>{const g=Date.parse(h.givenAt||"");if(Number.isFinite(g))return Math.max(0,Math.floor((Date.now()-g)/60000));
    return Number.isFinite(Number(h.heldMinutes))?Number(h.heldMinutes):null};

  function drawChip(){
    const s=st;
    if(!anything(s)){wrap.hidden=true;if(open)close(false);return}
    wrap.hidden=false;
    chip.textContent=headText(s);
    const warn=!!(slotsOf(s).some(x=>x.holder&&x.holder.overdue)||(s.mkdir&&s.mkdir.stale));
    chip.classList.toggle("warn",warn);
    chip.title=warn?"Test apps: something needs a look. Click for details.":"Test apps and their turns. Click for details.";
  }

  const btn=(act,text,cls,title)=>`<button type="button" class="btn sm${cls?" "+cls:""}" data-act="${esc(act)}"${title?` title="${esc(title)}"`:""}${busy?" disabled":""}>${esc(text)}</button>`;
  const armed=k=>confirmAt[k]&&Date.now()-confirmAt[k]<5000;

  // One slot's row: its card, its turn, its line, its install, and Release (the turn) and Free (the card).
  function slotRow(x){
    const L=labelOf(x),n=Number(x.slot)>0?Number(x.slot):1,h=x.holder,c=x.card,parts=[];
    const since=c&&hhmm(c.since);
    parts.push(`<span>${c?`<b>${esc(nameOf(c))}</b>${since?` · since ${esc(since)}`:""}`:"no card"}</span>`);
    if(h){const m=heldFor(h),ends=hhmm(h.expiresAt);
      parts.push(`<span>Turn: <b>${esc(nameOf(h))}</b> · ${esc(KIND[h.kind]||h.kind||"")}${h.waking?` · <span class="tapw">starting its session</span>`:""}${m!=null?` · held for ${esc(m)} min`:""}${ends?` · ends at ${esc(ends)}`:""}${h.overdue?` · <span class="tapw">overdue</span>`:""}</span>`)}
    const ln=lineOf(x);
    if(ln.length)parts.push(`<span class="tapk">Waiting for a turn</span><ol class="tapline">${ln.map((y,i)=>
      `<li><span class="tapplace">${esc(placeOf(y,i))}</span><span>${esc(nameOf(y))} · ${esc(KIND[y.kind]||y.kind||"")}${y.waking?` · <span class="tapw">starting its session</span>`:y.live===false?` · <span class="tapw">waits for its session</span>`:""}</span></li>`).join("")}</ol>`);
    const i=x.install;
    if(i){const w=reasonOf(i.waitingFor),r=hhmm(i.restartAt);
      parts.push(`<span>Install: <b>${esc(nameOf(i))}</b> · ${esc(PHASE[i.phase]||i.phase||"")}${w?` · waiting for ${esc(w)}`:""}${r?` · restarts at ${esc(r)}`:""}</span>`)}
    if(x.pending)parts.push(`<span>Restart pending (${esc(x.pending.label||"an install")})</span>`);
    if(x.otherBoard!=null&&x.otherBoard!=="")parts.push(`<span>Another board has a turn on it: ${esc(x.otherBoard)}</span>`);
    if(x.hughInputAt)parts.push(`<span class="tapnote">Your last input in it: ${esc(hhmm(x.hughInputAt))}</span>`);
    const rk="release-"+n,fk="free-"+n;
    const acts=(h?btn(rk,armed(rk)?`Release ${nameOf(h)}?`:"Release",armed(rk)?"danger":"ghost",`End ${nameOf(h)}'s turn on ${L} now.`):"")+
      (c?btn(fk,armed(fk)?`Free ${L}?`:"Free",armed(fk)?"danger":"ghost",`Take ${L} from ${nameOf(c)}: its turn and its place in line go too, and its session is told.`):"");
    return`<div class="taprow tapslot${h&&h.overdue?" warn":""}" data-slot="${esc(n)}"><div class="tapmain"><span class="tapk tapname">${esc(L)}${portOf(x)?` · port ${esc(portOf(x))}`:""}</span>
      ${parts.join("")}</div>${acts?`<span class="tapacts">${acts}</span>`:""}</div>`;
  }

  function drawPop(){
    if(!open||!st)return;
    const s=st,rows=[],slots=slotsOf(s);
    const ae=document.activeElement,keep=ae&&pop.contains(ae)&&ae.dataset&&ae.dataset.act?ae.dataset.act:refocus&&refocus.where==="pop"?refocus.act:null;
    for(const x of slots)rows.push(slotRow(x));
    if(!slots.length)rows.push(`<div class="tapnote">No test app has a card or a turn now.</div>`);
    const wl=slotLine(s);
    if(wl.length)rows.push(`<div class="taprow"><div class="tapmain"><span class="tapk">Waiting for a test app</span><ol class="tapline">${wl.map((x,i)=>
      `<li><span class="tapplace">${esc(placeOf(x,i))}</span><span>${esc(nameOf(x))}${x.live===false?` · <span class="tapw">waits for its session</span>`:""}</span></li>`).join("")}</ol></div></div>`);
    if(held(s))rows.push(`<div class="taprow"><div class="tapmain"><span class="tapk">Restarts</span><span>Restarts held until ${esc(hhmm(s.hold.until))}</span></div>
      <span class="tapacts">${btn("hold","Hold 30 min more","ghost")}${btn("letrestart","Let it restart","ghost")}</span></div>`);
    else rows.push(`<div class="taprow"><div class="tapmain"><span class="tapk">Restarts</span><span>${future(s.allowUntil)?`Allowed now, until ${esc(hhmm(s.allowUntil))}`:s.pending?`A restart of ${esc(s.pending.slotLabel||"a test app")} is pending`:"Not held"}</span></div>
      ${btn("hold","Hold restarts 30 min","ghost")}</div>`);
    const k=s.mkdir;
    if(k)rows.push(`<div class="taprow${k.stale?" warn":""}"><div class="tapmain"><span class="tapk">DSH lock</span>
      <span>Old DSH lock folder: owner ${esc(k.owner||"unknown")}, ${esc(Number.isFinite(Number(k.ageMinutes))?Number(k.ageMinutes):"?")} min${k.stale?` · <span class="tapw">stale</span>`:""}</span></div>
      ${btn("mkdir",armed("mkdir")?"Release the folder?":"Release",armed("mkdir")?"danger":"ghost")}</div>`);
    const lim=Number.isFinite(Number(s.limitMinutes))?` · ${esc(s.limitMinutes)} min a turn`:"",ceil=Number(s.ceiling)>0?` · at most ${esc(s.ceiling)} test app${Number(s.ceiling)===1?"":"s"}`:"";
    pop.innerHTML=`<div class="taphead">Test apps${lim||ceil?` <span class="tapnote">${lim}${ceil}</span>`:""}</div>${rows.join("")}
      <div class="msg${msgErr?" err":""}" id="testappMsg">${esc(msg)}</div>`;
    if(keep){const b=pop.querySelector(`[data-act="${CSS.escape(keep)}"]`);if(b&&!b.disabled)b.focus()}
    // Keep the popover inside the window at narrow widths.
    pop.style.left="0px";const r=pop.getBoundingClientRect(),over=r.right-(window.innerWidth-16);
    if(over>0)pop.style.left=Math.min(0,-over)+"px";
  }

  function drawBar(){
    const s=st,p=s&&s.pending;
    if(!p){bar.hidden=true;bar.innerHTML="";barMsg="";return}
    const ae=document.activeElement,keep=ae&&bar.contains(ae)&&ae.dataset&&ae.dataset.act?ae.dataset.act:refocus&&refocus.where==="bar"?refocus.act:null;
    const sl=slotsOf(s),x=sl.find(y=>p.slot!=null&&Number(y.slot)===Number(p.slot))||sl.find(y=>y.pending)||null;
    const L=p.slotLabel||(x?labelOf(x):""),w=x&&x.install?reasonOf(x.install.waitingFor):"";
    const h=held(s);
    bar.innerHTML=`<span class="ut"><b>Test app restart pending</b> (${esc(p.label||"an install")})${L?` · ${esc(L)}`:""}${w?` · waiting for ${esc(w)}`:""}${h?` · Held until ${esc(hhmm(s.hold.until))}`:""}</span>
      <span class="ua">${btn("hold",h?"Hold 30 min more":"Hold 30 min","")}${h?btn("letrestart","Let it restart","ghost"):""}</span>
      <span class="msg${barMsg?" err":""}">${esc(barMsg)}</span>`;
    bar.hidden=false;
    if(keep){const b=bar.querySelector(`[data-act="${CSS.escape(keep)}"]`);if(b&&!b.disabled)b.focus()}
  }

  // Card tiles: the status chip (doing.js) reads the latest status from here.
  window.testappState=()=>st;
  const told=()=>{try{document.dispatchEvent(new Event("testapp-status"))}catch(e){}};

  function draw(){drawChip();drawPop();drawBar()}
  function take(s){if(s&&typeof s==="object"){st=s;draw();told()}}

  // key: "release-N", "free-N" (slot N), "mkdir", "hold" or "letrestart".
  async function act(key,where){
    const [name,n]=String(key).split("-"),slot=Number(n)>0?Number(n):1;
    const cmd={release:"testapp_release",free:"testapp_free",mkdir:"testapp_release_mkdir",hold:"testapp_hold",letrestart:"testapp_let_restart"}[name];
    if(!cmd||busy)return;
    if(name==="release"||name==="free"||name==="mkdir"){
      if(!armed(key)){confirmAt[key]=Date.now();drawPop();setTimeout(()=>{if(confirmAt[key]&&!armed(key)){delete confirmAt[key];drawPop()}},5100);return}
      delete confirmAt[key]}
    busy=true;msg="";msgErr=false;barMsg="";refocus={where,act:key};draw();
    try{const s=await call(cmd,name==="release"||name==="free"?{slot}:undefined);busy=false;take(s);if(!s)draw()}
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

  // Hugh's input, in any test app ("test", "test2", ...): a real key, press or wheel while this
  // window has focus, outside the banner and the popover (pressing those is not testing the app).
  call("app_channel").then(ch=>{
    if(!String(ch||"").startsWith("test"))return;
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
