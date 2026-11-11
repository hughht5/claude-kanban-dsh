// The Agents drawer and the Router panel open by themselves when the app starts (card #189;
// Hugh, 11 Nov 2026: "Agents and router should open automatically on start"). Each one's ✕ still
// closes it, and nothing remembers that: the next start opens them again. Loaded after
// agents-feed.js and router.js. The drawer opens without taking the focus, so the board's keys
// work at once.
(root=>{
  function openAtStart(){
    try{if(root.agentsPane&&!root.agentsPane.isOpen())root.agentsPane.open({focus:false})}catch(e){console.error("agents at start",e)}
    try{if(typeof root.openRouterPanel==="function"&&!(root.sessionPaneShows&&root.sessionPaneShows()))root.openRouterPanel()}catch(e){console.error("router at start",e)}
  }
  root.openPanesAtStart=openAtStart;
  if(typeof document==="undefined")return;
  if(document.readyState==="loading")document.addEventListener("DOMContentLoaded",openAtStart);else openAtStart();
})(typeof window!=="undefined"?window:globalThis);
