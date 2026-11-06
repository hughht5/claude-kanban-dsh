// Light, dark or the system's choice, set on <html data-theme> before the page draws (this
// script loads in <head>), remembered per window. The header button cycles System → Light →
// Dark. board.css holds the colours for both.
(function(){
  const KEY="theme",ORDER=["system","light","dark"],LABEL={system:"◐ System",light:"☀ Light",dark:"☾ Dark"};
  const get=()=>{try{return localStorage.getItem(KEY)||"system"}catch(e){return "system"}};
  const apply=t=>{if(t==="system")document.documentElement.removeAttribute("data-theme");else document.documentElement.setAttribute("data-theme",t);
    document.dispatchEvent(new CustomEvent("themechange",{detail:effective()}))};
  const effective=()=>{const t=get();return t==="system"?(matchMedia("(prefers-color-scheme: dark)").matches?"dark":"light"):t};
  window.themeIsDark=()=>effective()==="dark";
  apply(get());
  matchMedia("(prefers-color-scheme: dark)").addEventListener("change",()=>apply(get()));
  document.addEventListener("DOMContentLoaded",()=>{
    const h=document.querySelector("header");if(!h)return;
    const b=document.createElement("button");b.type="button";b.className="btn ghost";b.id="themeBtn";
    const draw=()=>{b.textContent=LABEL[get()];b.title="Colour theme: "+get()+". Click to change."};draw();
    b.onclick=()=>{const n=ORDER[(ORDER.indexOf(get())+1)%ORDER.length];try{localStorage.setItem(KEY,n)}catch(e){}apply(n);draw()};
    const s=[...h.querySelectorAll("button")].find(x=>x.textContent.includes("Settings"));
    if(s)s.after(b);else h.appendChild(b);
  });
})();
