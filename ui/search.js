// Search sessions (card #110; restyled as the mockup's .sr rows and given the time bar by card #120): one search over every card's session history (the last 90 days),
// from the header button or Ctrl+Shift+F, on the board or over a card panel. Results group by
// card; a hit opens that card's panel with the words in its conversation search (#p-q).
// Backend text never goes into innerHTML: every part is set with textContent.
// Uses board.js's $, invoke, errText, safe; panel.js's openPanel, panel, setQuery.
(function(){
  const PAUSE=250,FIRST=3,POLL=2000;
  const d=$("#searchDlg");if(!d)return;
  let seq=0,timer=null,poll=null,back=null,res=null,query="",open=new Set(),wasRunning=false;

  d.innerHTML=`<div class="ss">
    <div class="ss-head"><h2 id="ss-ttl">Search sessions</h2>
      <button class="ibtn x" id="ss-x" type="button" aria-label="Close">✕</button></div>
    <div class="ss-bar"><input type="search" id="ss-q" class="ss-q" placeholder="Words from any card's conversation" aria-label="Search every card's session history" autocomplete="off" spellcheck="false">
      <div class="ss-note">Searches the last 90 days</div>
      <div class="ss-status" id="ss-status" role="status" aria-live="polite"></div>
      <div class="ss-index" id="ss-index" aria-live="polite" hidden></div></div>
    <div class="ss-res" id="ss-res" aria-labelledby="ss-ttl"></div>
    <div class="ss-times" id="ss-times" aria-label="Median time in each column"></div></div>`;
  d.setAttribute("aria-labelledby","ss-ttl");
  const inp=d.querySelector("#ss-q"),out=d.querySelector("#ss-res"),st=d.querySelector("#ss-status"),ix=d.querySelector("#ss-index");
  const el=(tag,cls,text)=>{const n=document.createElement(tag);if(cls)n.className=cls;if(text!=null)n.textContent=String(text);return n};
  const say=(t,err)=>{st.textContent=t||"";st.classList.toggle("err",!!err)};
  const longEnough=s=>{s=String(s||"");return s.split(/[\s"]+/).some(w=>[...w].length>=3)||/"[^"]{3,}"/.test(s)};
  const fmt=n=>Number(n||0).toLocaleString("en-GB");
  // "3 hits" at the right of a card's head (card #120, the mockup's .sr).
  const countText=g=>{const c=Number(g.count||0);const n=g.countCapped?(c===1000?"1,000+":fmt(c)+"+"):fmt(c);return`${n} hit${!g.countCapped&&c===1?"":"s"}`};
  const WHO={user:"Prompt",assistant:"Claude",task:"Task",cmdout:"Command output"};
  const whoText=h=>{const w=h.kind==="tool"?(h.who||"Tool"):(WHO[h.kind]||h.kind||"");return h.agent!=null&&h.agent!==""?`${w} · ${h.agent}`:w};
  function when(at){
    const t=at?new Date(at):null;if(!t||isNaN(t))return"";
    const o={day:"numeric",month:"short",hour:"2-digit",minute:"2-digit"};
    if(t.getFullYear()!==new Date().getFullYear())o.year="numeric";
    return t.toLocaleString("en-GB",o);
  }
  // The first marked part of a hit: words that are in the transcript as they are.
  const markOf=h=>{const p=(Array.isArray(h&&h.snippet)?h.snippet:[]).find(x=>x&&x.m&&String(x.t||"").trim());return p?String(p.t).trim():""};

  function showIndex(s){
    const on=!!(s&&s.running);
    ix.hidden=!on;ix.textContent=on?`Indexing: ${fmt(s.done)} of ${fmt(s.total)} transcripts`:"";
    if(on&&!poll&&d.open)poll=setInterval(pollStatus,POLL);
    if(!on&&poll){clearInterval(poll);poll=null}
    // Indexing just finished: search again, so the answer covers every transcript.
    if(wasRunning&&!on&&d.open&&longEnough(query))run(query);
    wasRunning=on;
  }
  async function pollStatus(){
    if(!d.open)return;
    try{const s=await invoke("search_status");if(d.open)showIndex(s)}catch(e){/* the next search says what is wrong */}
  }

  function schedule(){clearTimeout(timer);timer=setTimeout(()=>run(inp.value),PAUSE)}
  async function run(v){
    clearTimeout(timer);
    const q=String(v||"").trim(),my=++seq;query=q;
    if(!longEnough(q)){res=null;open=new Set();out.replaceChildren();say(q?"Type at least 3 letters":"");return}
    say("Searching…");
    let r;
    try{r=await invoke("search_sessions",{query:q,card:null,limit:null})}
    catch(e){if(my!==seq||!d.open)return;res=null;out.replaceChildren();say(errText(e),true);return}
    if(my!==seq||!d.open)return; // a newer query took over
    if(!r||r.status==="interrupted"){say("");return}
    if(r.indexing)showIndex(r.indexing);
    if(r.status==="short"){res=null;out.replaceChildren();say("Type at least 3 letters");return}
    res=r;open=new Set();draw();
  }

  function draw(){
    out.replaceChildren();
    const groups=Array.isArray(res&&res.cards)?res.cards:[];
    if(!groups.length){say("No matches");return}
    const total=Number(res.total||0);
    say(`${fmt(total)} match${total===1?"":"es"} in ${fmt(groups.length)} card${groups.length===1?"":"s"}`+(res.limited?" · Showing the newest 2,000 matches":""));
    const list=el("ul","ss-list");
    for(const g of groups){
      const li=el("li","ss-card"),hits=Array.isArray(g.hits)?g.hits:[];
      const head=el("button","ss-chead");head.type="button";
      head.title="Open this card with the words in its conversation search";
      if(Number.isInteger(g.number)&&g.number>0)head.append(el("b","num","#"+g.number));
      const t=el("span","ss-ctitle");t.innerHTML=safe(g.title||"Untitled card"); // safe() escapes, and hides what looks like a secret
      head.append(t,el("span","ss-col",g.archived?"Archived":(g.column||"")),el("span","ss-count",countText(g)));
      head.onclick=()=>go(g.card,hits[0]);
      li.append(head);
      const ul=el("ul","ss-hits"),all=open.has(g.card);
      hits.slice(0,all?hits.length:3).forEach(h=>ul.append(hitRow(g,h)));
      li.append(ul);
      if(!all&&hits.length>3){
        const more=el("button","ibtn ss-more",`Show all ${fmt(hits.length)}`);more.type="button";
        more.onclick=()=>{open.add(g.card);draw(); // then focus the first hit that was hidden
          const row=out.querySelector(`.ss-card[data-card="${CSS.escape(String(g.card))}"] .ss-hit[data-i="3"]`);if(row)row.focus()};
        li.append(more);
      }
      li.dataset.card=String(g.card);
      list.append(li);
    }
    out.append(list);
  }
  function hitRow(g,h){
    const li=el("li"),b=el("button","ss-hit");b.type="button";
    b.dataset.i=String((Array.isArray(g.hits)?g.hits:[]).indexOf(h));
    const top=el("span","ss-meta");
    top.append(el("span","ss-who",whoText(h)));
    const w=when(h.at);if(w){const tm=el("time","ss-at",w);tm.dateTime=String(h.at);top.append(tm)}
    if(h.earlier)top.append(el("span","ss-earlier","earlier session"));
    const sn=el("span","ss-snip");
    for(const p of Array.isArray(h.snippet)?h.snippet:[]){
      const s=String((p&&p.t)??"");if(!s)continue;
      if(p.m){const mk=document.createElement("mark");mk.textContent=s;sn.append(mk)}else sn.append(document.createTextNode(s));
    }
    b.append(top,sn);
    b.onclick=()=>go(g.card,h);
    li.append(b);return li;
  }

  // Opens the card's panel and runs its conversation search for the query. The panel draws its
  // DOM in openPanel and reads the transcript after, so the search waits for the first read. With
  // no match for the whole query (the panel looks for it as one phrase), it takes the hit's marked word.
  function go(id,hit){
    if(!id)return;
    const words=query,alt=markOf(hit);
    back=null;d.close(); // the close event comes later: no focus going back to where search opened
    if(typeof openPanel!=="function")return;
    openPanel(id);
    if(!panel||panel.id!==id){status("Couldn't open that card: it isn't on the board.",true);return}
    // The panel shows the card's latest conversation and its subagents only.
    if(hit&&hit.earlier)status("That match is from an earlier session of this card: the panel shows only its latest conversation.");
    else if(hit&&hit.agentId&&typeof showView==="function")showView(String(hit.agentId));
    const gen=panel.gen;
    const box=panel.el.querySelector("#p-q");box.value=words;
    const t0=Date.now();
    const apply=()=>{
      if(!panel||panel.id!==id)return;
      const ready=panel.gen===gen&&(panel.entries.length>0||panel.offset>0);
      if(!ready&&Date.now()-t0<4000){setTimeout(apply,100);return}
      const q=panel.el.querySelector("#p-q");
      const find=v=>{q.value=v;if(typeof setQuery==="function"){clearTimeout(panel.findTimer);setQuery(v)}else q.dispatchEvent(new Event("input",{bubbles:true}))};
      find(words);
      if(typeof setQuery==="function"&&!panel.find.hits.length&&alt&&alt.toLowerCase()!==words.toLowerCase())find(alt);
      q.focus();
    };
    apply();
  }

  function openSearch(){
    if(d.open){inp.focus();inp.select();return}
    back=document.activeElement;
    const f=$("#q");inp.value=f?f.value:"";
    try{d.showModal()}catch(e){d.setAttribute("open","")}
    // The time bar under the results (times.js, card #120): from the cached medians, asked again at most once a minute.
    if(typeof timesBarRedraw==="function"){try{loadMedians();timesBarRedraw()}catch(e){}}
    inp.focus();inp.select();
    wasRunning=false;pollStatus();
    run(inp.value);
  }
  d.addEventListener("close",()=>{
    clearTimeout(timer);clearInterval(poll);poll=null;seq++;wasRunning=false;
    const b=back;back=null;
    if(b&&b.isConnected&&typeof b.focus==="function")b.focus();
  });
  d.addEventListener("click",e=>{if(e.target===d)d.close()});
  d.querySelector("#ss-x").onclick=()=>d.close();
  inp.addEventListener("input",schedule);
  // Keys stay in the dialog, so the panel behind it (Esc closes it, Ctrl+F) doesn't act on them.
  d.addEventListener("keydown",e=>{
    e.stopPropagation();
    if(e.key==="Escape"){e.preventDefault();d.close();return}
    const hits=[...out.querySelectorAll(".ss-hit")],i=hits.indexOf(document.activeElement);
    if(e.key==="ArrowDown"||e.key==="ArrowUp"){
      if(e.target===inp&&e.key==="ArrowUp")return;
      if(!hits.length)return;
      e.preventDefault();
      if(e.key==="ArrowDown")hits[i<0?0:Math.min(i+1,hits.length-1)].focus();
      else if(i<=0)inp.focus();else hits[i-1].focus();
      const cur=document.activeElement;if(cur&&cur.scrollIntoView)cur.scrollIntoView({block:"nearest"});
      return;
    }
    if(e.key==="Enter"&&e.target===inp){
      e.preventDefault();
      // Enter in the box: search now; with results for these words already, open the first hit.
      if(res&&inp.value.trim()===query&&hits.length)hits[0].click();else run(inp.value);
    }
  });
  window.addEventListener("keydown",e=>{
    if(!((e.ctrlKey||e.metaKey)&&e.shiftKey&&!e.altKey&&String(e.key).toLowerCase()==="f"))return;
    const a=$("#addDlg");if(a&&a.open)return;
    e.preventDefault();e.stopPropagation();openSearch();
  },true);
  const btn=$("#searchBtn");if(btn)btn.onclick=openSearch;
  window.openSessionSearch=openSearch;
})();
