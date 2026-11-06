// The Agents feed (card #120): what the router, critic, tester, reviewer, reproducer, merger and
// monitor did, newest first. One module, two places: the header's "Agents" drawer (below) and
// card #119's Router pane, which mounts its own copy.
//
//   mountAgentsFeed(el, {filter, source, onCard, compact}) -> {refresh(), destroy(), older()}
//
// - filter: passed through as the query ({cards, agents, kinds, since, limit}).
// - source(query): a Promise of {items, next}. The default asks the app: invoke("agents_feed",
//   {query}). "Show older" asks again with {...filter, before: next}.
// - Items: {seq, at, card, agent, kind, verdict, field, text}, newest first, drawn in the order
//   given. kind "verdict" rows name the check from field ("TESTER passed"); card #0 is "Router".
// - onCard(n): a click on a "#n" in a row.
// - compact: narrow rows for a 360 px pane (the agent above the text).
// It refreshes on mount, on refresh(), and on the app's "cards-changed" event at most once every
// 2 s; never on a board render. A refresh keeps the scroll position.
// The pure helpers are on window.AgentsFeed, for tools/ui-tests/feed.test.mjs.
(function(root){
  "use strict";
  const AGENTS=["router","critic","tester","reviewer","reproducer","merger","monitor","hugh"];
  // "hugh": his own messages to the Router, in its log (card #119).
  const WORD={router:"Router",critic:"Critic",tester:"Tester",reviewer:"Reviewer",reproducer:"Reproducer",merger:"Merger",monitor:"Monitor",hugh:"Hugh"};
  const THROTTLE_MS=2000;
  const MON=["Jan","Feb","Mar","Apr","May","Jun","Jul","Aug","Sep","Oct","Nov","Dec"];
  const esc=s=>String(s??"").replace(/[&<>"']/g,ch=>({"&":"&amp;","<":"&lt;",">":"&gt;",'"':"&quot;","'":"&#39;"}[ch]));
  // board.js's SECRET, when the page has it: a line that looks like it holds a secret isn't shown.
  const secret=s=>{try{return typeof SECRET!=="undefined"&&SECRET.test(String(s||""))}catch(e){return false}};
  const pad=n=>String(n).padStart(2,"0");

  // "the router", "card-tester", "Critic" -> one of AGENTS, else "other".
  function agentOf(a){
    const k=String(a||"").trim().toLowerCase().replace(/^the\s+/,"").replace(/^card-/,"");
    return AGENTS.includes(k)?k:"other";
  }
  const agentWord=a=>{const k=agentOf(a);return k==="other"?String(a||"Agent"):WORD[k]};
  // Card #0 is the router's own card: its items are the Router's, shown as "Router", never "#0".
  const isRouterCard=item=>Number(item&&item.card)===0;
  const itemAgent=item=>{const k=agentOf(item&&item.agent);return k==="other"&&isRouterCard(item)?"router":k};
  // A verdict item's card field, named as its check: "TESTER passed", "MONITOR repeating".
  const FIELD_WORD={test:"Tester",review:"Reviewer",critique:"Critic",repro:"Reproducer",merge:"Merger",route:"Router",stuck:"Monitor"};
  // The time cell: HH:MM today, else the date; the full time in the title.
  function timeParts(at,now){
    const d=at?new Date(at):null;
    if(!d||isNaN(d))return{text:"",title:""};
    const n=now instanceof Date?now:new Date();
    const today=d.getFullYear()===n.getFullYear()&&d.getMonth()===n.getMonth()&&d.getDate()===n.getDate();
    const text=today?`${pad(d.getHours())}:${pad(d.getMinutes())}`:`${d.getDate()} ${MON[d.getMonth()]}${d.getFullYear()===n.getFullYear()?"":" "+d.getFullYear()}`;
    let title="";
    try{title=d.toLocaleString("en-GB",{weekday:"short",day:"numeric",month:"short",year:"numeric",hour:"2-digit",minute:"2-digit"})}catch(e){title=d.toISOString()}
    return{text,title};
  }
  const cardLink=n=>`<button type="button" class="fe-card" data-card="${n}" title="Open card #${n}">#${n}</button>`;
  // The row's text, escaped, with each "#n" a button for onCard. The item's card goes first when
  // the text doesn't name it (not card #0, the router's own card).
  function textHtml(item){
    let t=String(item&&item.text||"");
    if(secret(t))return`<span class="hidden-secret">hidden: looks like a secret</span>`;
    const n=Number(item&&item.card);
    if(Number.isInteger(n)&&n>0&&!new RegExp(`#${n}(?!\\d)`).test(t))t=`#${n} ${t}`;
    return t.split(/(#\d+)(?!\d)/).map((p,i)=>!(i%2)?esc(p):Number(p.slice(1))===0?"Router":cardLink(Number(p.slice(1)))).join("");
  }
  const VERDICT_TONE={pass:"ok",passed:"ok",fixed:"fix",fail:"bad",failed:"bad",unclear:"warn",cleared:"none",needed:"none"};
  function verdictHtml(item){
    const v=String(item&&item.verdict||"").trim();if(!v)return"";
    const fw=item.kind==="verdict"&&item.field?FIELD_WORD[item.field]||String(item.field):"";
    // The field is named only when it isn't the row's own agent: "PASSED", not "TESTER TESTER passed".
    if(fw&&fw.toLowerCase()===WORD[itemAgent(item)]?.toLowerCase())return`<span class="fe-v ${VERDICT_TONE[v.toLowerCase()]||"warn"}">${esc(v.toUpperCase())}</span> `;
    if(fw)return`<span class="fe-v ${VERDICT_TONE[v.toLowerCase()]||"warn"}">${esc(fw.toUpperCase())} ${esc(v)}</span> `;
    // Skipped when the text already says it (the mockup's "PASS." in the text).
    if(String(item.text||"").toUpperCase().includes(v.toUpperCase()))return"";
    return`<span class="fe-v ${VERDICT_TONE[v.toLowerCase()]||"warn"}">${esc(v.toUpperCase())}</span> `;
  }
  // A key that is the same for the same event on every page: the app's own cursor fields.
  const itemKey=i=>[i&&i.kind,i&&i.seq,i&&i.field,i&&i.at,i&&i.card].map(x=>x??"").join("|");
  function rowHtml(item,opts){
    const k=itemAgent(item),tp=timeParts(item&&item.at,opts&&opts.now);
    return`<li class="fe ag-${k}" data-key="${esc(itemKey(item))}"><time${item&&item.at?` datetime="${esc(item.at)}"`:""} title="${esc(tp.title)}">${esc(tp.text)}</time>`
      +`<span class="ag ${k}">${esc(k==="other"?agentWord(item&&item.agent):WORD[k])}</span><span class="fe-t">${verdictHtml(item)}${textHtml(item)}</span></li>`;
  }
  function rowsHtml(items,opts){
    const list=Array.isArray(items)?items:[];
    return`<ol class="feed${opts&&opts.compact?" compact":""}" aria-label="What the agents did, newest first">${list.map(i=>rowHtml(i,opts)).join("")}</ol>`;
  }
  // An older page goes on the end; an item already there is not added again.
  function mergeOlder(have,more){
    const seen=new Set(have.map(itemKey)),out=have.slice();
    for(const i of Array.isArray(more)?more:[]){const k=itemKey(i);if(!seen.has(k)){seen.add(k);out.push(i)}}
    return out;
  }
  // The newest page after a refresh: new items go first. With no overlap (a gap), the fresh page
  // replaces what was there, and `gap` says so (the caller takes the fresh cursor).
  function mergeNewer(have,fresh){
    const f=Array.isArray(fresh)?fresh:[];
    const fk=new Set(f.map(itemKey));
    if(have.length&&f.length&&!have.some(i=>fk.has(itemKey(i))))return{items:f.slice(),gap:true};
    const out=f.slice();for(const i of have)if(!fk.has(itemKey(i)))out.push(i);
    return{items:out,gap:!have.length};
  }
  const norm=r=>Array.isArray(r)?{items:r,next:null}:{items:Array.isArray(r&&r.items)?r.items:[],next:r&&r.next!=null?r.next:null};
  // At most one call every `ms`; a call inside the window runs once at its end.
  function throttle(fn,ms,clock){
    const now=clock||(()=>Date.now());let last=-Infinity,timer=null;
    const t=function(){const w=last+ms-now();
      if(w<=0){last=now();fn();return}
      if(!timer)timer=setTimeout(()=>{timer=null;last=now();fn()},w)};
    t.cancel=()=>{if(timer){clearTimeout(timer);timer=null}};
    return t;
  }
  const defaultSource=q=>typeof invoke==="function"?invoke("agents_feed",{query:q}):Promise.reject(new Error("The app isn't there to ask"));
  const errText_=e=>typeof errText==="function"?errText(e):(e&&e.message)||String(e);
  // The element that scrolls the feed: el or the nearest parent that scrolls.
  function scroller(el){
    for(let n=el;n&&n.nodeType===1;n=n.parentElement){
      try{if(n.scrollHeight>n.clientHeight&&/(auto|scroll)/.test(getComputedStyle(n).overflowY))return n}catch(e){return null}
    }
    return null;
  }

  // The first feed row whose bottom is below the scroller's top, with its key and its offset
  // from that top.
  function firstInView(sc){
    try{
      const t=sc.getBoundingClientRect().top;
      for(const li of sc.querySelectorAll("li.fe[data-key]")){
        const r=li.getBoundingClientRect();
        if(r.bottom>t)return{key:li.getAttribute("data-key"),off:r.top-t};
      }
    }catch(e){}
    return null;
  }

  function mountAgentsFeed(el,opts){
    opts=opts||{};
    const filter=Object.assign({},opts.filter||{});
    const source=typeof opts.source==="function"?opts.source:defaultSource;
    const st={items:[],next:null,loaded:false,busy:false,again:false,err:"",html:"",gone:false,older:false};
    if(el.classList){el.classList.add("af");if(opts.compact)el.classList.add("af-compact")}
    const onClick=e=>{
      const t=e.target&&e.target.closest?e.target:null;if(!t)return;
      const b=t.closest(".fe-card");if(b&&el.contains(b)){e.preventDefault();const n=Number(b.dataset.card);if(typeof opts.onCard==="function"&&Number.isInteger(n))opts.onCard(n);return}
      const o=t.closest("[data-older]");if(o&&el.contains(o)){e.preventDefault();older()}
    };
    if(el.addEventListener)el.addEventListener("click",onClick);
    function html(){
      const parts=[];
      if(!st.loaded&&!st.err)parts.push(`<p class="af-msg">Reading what the agents did…</p>`);
      else if(st.err&&!st.items.length)parts.push(`<p class="af-msg err">Couldn't read the feed: ${esc(st.err)}</p>`);
      else if(!st.items.length)parts.push(`<p class="af-msg">Nothing from the agents yet.</p>`);
      if(st.items.length)parts.push(rowsHtml(st.items,{compact:!!opts.compact}));
      if(st.err&&st.items.length)parts.push(`<p class="af-msg err">Couldn't read more: ${esc(st.err)}</p>`);
      if(st.items.length&&st.next!=null)parts.push(`<button type="button" class="ibtn af-older" data-older${st.older?" disabled":""}>${st.older?"Reading…":"Show older"}</button>`);
      return parts.join("");
    }
    function draw(){
      if(st.gone)return;
      const h=html();if(h===st.html)return;st.html=h;
      // Keep the place: a reader scrolled down stays on the same rows when new ones come on top.
      // The first row in view is the anchor: after the redraw it is put back where it was, so
      // rows added at the top move the view by their height, and rows added at the bottom ("Show
      // older") or the "Reading…" button move it not at all.
      const sc=typeof getComputedStyle==="function"?scroller(el):null,top=sc?sc.scrollTop:0;
      const anchor=sc&&top>0?firstInView(sc):null;
      const fo=typeof document!=="undefined"&&document.activeElement&&el.contains&&el.contains(document.activeElement)?document.activeElement:null;
      const fk=fo?(fo.dataset&&fo.dataset.card?`.fe-card[data-card="${fo.dataset.card}"]`:fo.hasAttribute&&fo.hasAttribute("data-older")?"[data-older]":null):null;
      el.innerHTML=h;
      if(anchor){
        let n=null;for(const li of el.querySelectorAll("li.fe[data-key]"))if(li.getAttribute("data-key")===anchor.key){n=li;break}
        // From where it is now (the new HTML may have clamped it), by how far the anchor moved.
        if(n)sc.scrollTop=sc.scrollTop+(n.getBoundingClientRect().top-sc.getBoundingClientRect().top-anchor.off);
        else sc.scrollTop=top;
      }
      if(fk){const n=el.querySelector(fk);if(n)n.focus({preventScroll:true})}
    }
    async function refresh(){
      if(st.gone)return;
      if(st.busy){st.again=true;return}
      st.busy=true;
      try{
        const r=norm(await source(Object.assign({},filter)));
        if(st.gone)return;
        const m=mergeNewer(st.items,r.items);
        st.items=m.items;if(m.gap||!st.loaded)st.next=r.next;
        st.loaded=true;st.err="";
      }catch(e){if(st.gone)return;st.err=errText_(e);st.loaded=true}
      finally{st.busy=false}
      draw();
      if(st.again){st.again=false;await refresh()}
    }
    async function older(){
      if(st.gone||st.next==null||st.older)return;
      st.older=true;draw();
      try{
        const r=norm(await source(Object.assign({},filter,{before:st.next})));
        if(st.gone)return;
        st.items=mergeOlder(st.items,r.items);st.next=r.next;st.err="";
      }catch(e){if(st.gone)return;st.err=errText_(e)}
      finally{st.older=false}
      draw();
    }
    // A hidden feed doesn't read on the app's events (opts.active says whether it is shown); its
    // owner refreshes it when it is shown again.
    const active=typeof opts.active==="function"?opts.active:()=>true;
    const poke=throttle(()=>{if(active())refresh()},THROTTLE_MS);
    let unlisten=null;
    try{
      const ev=root.__TAURI__&&root.__TAURI__.event;
      if(ev&&typeof ev.listen==="function")Promise.resolve(ev.listen("cards-changed",()=>{if(active())poke()})).then(u=>{if(st.gone){try{u()}catch(e){}}else unlisten=u}).catch(()=>{});
    }catch(e){}
    draw();
    const first=refresh();
    return{
      refresh,older,
      ready:first, // for tests: the first fetch
      items:()=>st.items.slice(),
      destroy(){
        st.gone=true;poke.cancel();
        if(unlisten){try{unlisten()}catch(e){}unlisten=null}
        if(el.removeEventListener)el.removeEventListener("click",onClick);
      },
    };
  }

  root.AgentsFeed={AGENTS,WORD,THROTTLE_MS,agentOf,agentWord,timeParts,textHtml,rowHtml,rowsHtml,itemKey,mergeOlder,mergeNewer,throttle,norm};
  root.mountAgentsFeed=mountAgentsFeed;

  // The header's "Agents" drawer: a right-docked pane (rpane.css), not a <dialog>, so Human test
  // review keys (P, F, U, arrows) still work with it open. Esc closes it when the focus is in it,
  // or on the board. Ctrl+Shift+A opens or closes it, over a card panel too.
  if(typeof document==="undefined"||!document.querySelector||!document.querySelector("header"))return;
  let pane=null,feed=null,back=null;
  const btn=document.createElement("button");
  btn.className="ibtn agentsbtn";btn.id="agentsBtn";btn.type="button";btn.textContent="Agents";
  btn.title="What the agents did, newest first (Ctrl+Shift+A)";btn.setAttribute("aria-pressed","false");btn.setAttribute("aria-controls","agentsPane");
  {const sb=document.getElementById("searchBtn"),hd=document.querySelector("header");if(sb)sb.after(btn);else hd.append(btn)}
  const isOpen=()=>!!pane&&!pane.hidden;
  function openCard(n){
    const list=typeof cards!=="undefined"&&Array.isArray(cards)?cards:[];
    const c=list.find(x=>x&&x.number===n&&!(typeof isSystemCard==="function"&&isSystemCard(x)));
    if(!c){if(typeof status==="function")status(`Card #${n} isn't on the board`,true);return}
    if(typeof openPanel==="function")openPanel(c.id);
  }
  function build(){
    pane=document.createElement("aside");pane.className="rpane solo";pane.id="agentsPane";pane.tabIndex=-1;pane.hidden=true;
    pane.setAttribute("aria-labelledby","ap-ttl");
    pane.innerHTML=`<div class="rpane-head"><h2 id="ap-ttl">Agents</h2><span class="rpane-sub">What they did, newest first</span><button class="ibtn x" type="button" id="ap-x" aria-label="Close the Agents pane">✕</button></div><div class="rpane-feed" id="ap-feed"></div>`;
    document.body.append(pane);
    pane.querySelector("#ap-x").onclick=()=>closePane();
    pane.addEventListener("keydown",e=>{if(e.key==="Escape"){e.preventDefault();e.stopPropagation();closePane()}});
  }
  function openPane(){
    if(!pane)build();
    back=document.activeElement;
    pane.hidden=false;document.body.classList.add("rpane-open");btn.setAttribute("aria-pressed","true");
    if(feed)feed.refresh();else feed=mountAgentsFeed(pane.querySelector("#ap-feed"),{onCard:openCard,active:isOpen});
    // The focus goes in only from the board; over a card in review it stays where it is, so the keys go on working.
    if(!(typeof panel!=="undefined"&&panel))pane.focus({preventScroll:true});
  }
  function closePane(){
    if(!isOpen())return;
    const had=pane.contains(document.activeElement);
    pane.hidden=true;document.body.classList.remove("rpane-open");btn.setAttribute("aria-pressed","false");
    // The feed stays mounted, so it opens again where it was; while closed it ignores the app's
    // events (active: isOpen), and openPane refreshes it.
    if(had){const b=back&&back.isConnected&&back!==document.body?back:btn;try{b.focus({preventScroll:true})}catch(e){}}
    back=null;
  }
  btn.onclick=()=>isOpen()?closePane():openPane();
  window.addEventListener("keydown",e=>{
    if(e.key==="Escape"&&isOpen()&&!e.defaultPrevented){
      const a=document.activeElement,onBoard=!a||a===document.body||!!(a.closest&&a.closest("main.board,header"));
      if(onBoard&&!(typeof panel!=="undefined"&&panel)&&!document.querySelector("dialog[open]")){e.preventDefault();closePane()}
      return;
    }
    if(!((e.ctrlKey||e.metaKey)&&e.shiftKey&&!e.altKey&&String(e.key).toLowerCase()==="a"))return;
    // Typing keeps its keys: an input, a textarea, a select, anything editable, and the terminal.
    const a=document.activeElement;
    if(a&&a!==document.body){
      const tag=String(a.tagName||"").toLowerCase();
      if(tag==="input"||tag==="textarea"||tag==="select"||a.isContentEditable)return;
      if(a.closest&&a.closest(".xterm,.term"))return;
    }
    if(document.querySelector("dialog[open]"))return;
    e.preventDefault();e.stopPropagation();isOpen()?closePane():openPane();
  },true);
  root.agentsPane={open:openPane,close:closePane,isOpen};
})(typeof window!=="undefined"?window:globalThis);
