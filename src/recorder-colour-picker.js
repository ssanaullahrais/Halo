// In-widget colour controls stay inside the native overlay's tracked input regions.
export function mountColourPicker(input,button,panel,onToggle){
 const presets=['#2563eb','#06b6d4','#7c3aed','#ec4899','#ef4444','#f97316','#eab308','#22c55e','#ffffff','#111827'];
 panel.innerHTML=`<div class="colour-plane" role="slider" tabindex="0" aria-label="Colour saturation and brightness"><i></i></div><input class="colour-hue" type="range" min="0" max="359" value="221" aria-label="Colour hue"><div class="colour-presets">${presets.map(c=>`<button type="button" style="--colour:${c}" data-colour="${c}" aria-label="Use ${c}"></button>`).join('')}</div><label class="colour-hex">Colour <input type="text" maxlength="7" spellcheck="false" aria-label="Hex colour" value="${input.value}"></label>`;
 const plane=panel.querySelector('.colour-plane'),dot=plane.querySelector('i'),slider=panel.querySelector('.colour-hue'),hex=panel.querySelector('.colour-hex input');let hue=221,saturation=.84,value=.92,internal=false;
 function read(){const rgb=input.value.slice(1).match(/../g).map(v=>parseInt(v,16)/255),max=Math.max(...rgb),min=Math.min(...rgb),delta=max-min;value=max;saturation=max?delta/max:0;if(delta){const [r,g,b]=rgb;hue=60*(max===r?((g-b)/delta+6)%6:max===g?(b-r)/delta+2:(r-g)/delta+4);}}
 function paint(){button.style.background=input.value;button.title=`Drawing colour ${input.value}`;plane.style.setProperty('--hue',hue);dot.style.left=`${saturation*100}%`;dot.style.top=`${(1-value)*100}%`;slider.value=String(Math.round(hue));hex.value=input.value;hex.setAttribute('aria-invalid','false');plane.setAttribute('aria-valuetext',input.value);for(const b of panel.querySelectorAll('[data-colour]'))b.setAttribute('aria-pressed',String(b.dataset.colour===input.value.toLowerCase()));}
 function apply(colour){input.value=colour.toLowerCase();internal=true;input.dispatchEvent(new Event('input',{bubbles:true}));internal=false;paint();}
 function hsv(){const c=value*saturation,x=c*(1-Math.abs((hue/60)%2-1)),m=value-c;const rgb=hue<60?[c,x,0]:hue<120?[x,c,0]:hue<180?[0,c,x]:hue<240?[0,x,c]:hue<300?[x,0,c]:[c,0,x];apply('#'+rgb.map(v=>Math.round((v+m)*255).toString(16).padStart(2,'0')).join(''));}
 function position(){const r=button.getBoundingClientRect();panel.style.left=`${Math.max(8,Math.min(innerWidth-panel.offsetWidth-8,r.left+r.width/2-panel.offsetWidth/2))}px`;panel.style.top=`${Math.max(8,document.getElementById('annotationTools').getBoundingClientRect().top-panel.offsetHeight-8)}px`;}
 function close(){panel.hidden=true;button.setAttribute('aria-expanded','false');onToggle();}
 button.onclick=()=>{if(!panel.hidden){close();return;}panel.hidden=false;read();paint();position();button.setAttribute('aria-expanded','true');onToggle();hex.focus();};
 input.addEventListener('input',()=>{if(!internal)read();paint();});input.addEventListener('change',()=>{read();paint();});
 slider.oninput=()=>{hue=Number(slider.value);hsv();};
 function point(e){const r=plane.getBoundingClientRect();saturation=Math.max(0,Math.min(1,(e.clientX-r.left)/r.width));value=1-Math.max(0,Math.min(1,(e.clientY-r.top)/r.height));hsv();}
 plane.onpointerdown=e=>{e.preventDefault();plane.setPointerCapture(e.pointerId);point(e);};plane.onpointermove=e=>{if(plane.hasPointerCapture(e.pointerId))point(e);};
 plane.onkeydown=e=>{if(['ArrowLeft','ArrowRight','ArrowUp','ArrowDown'].includes(e.key)){e.preventDefault();saturation=Math.max(0,Math.min(1,saturation+(e.key==='ArrowRight'?.02:e.key==='ArrowLeft'?-.02:0)));value=Math.max(0,Math.min(1,value+(e.key==='ArrowUp'?.02:e.key==='ArrowDown'?-.02:0)));hsv();}};
 for(const b of panel.querySelectorAll('[data-colour]'))b.onclick=()=>{apply(b.dataset.colour);read();paint();};
 hex.oninput=()=>{const colour=hex.value.startsWith('#')?hex.value:'#'+hex.value;if(/^#[0-9a-f]{6}$/i.test(colour)){apply(colour);read();paint();}else hex.setAttribute('aria-invalid','true');};
 panel.addEventListener('keydown',e=>{e.stopPropagation();if(e.key==='Escape'||(e.key==='Enter'&&e.target===hex)){e.preventDefault();close();button.focus();}});
 document.addEventListener('pointerdown',e=>{if(!panel.hidden&&!panel.contains(e.target)&&!button.contains(e.target))close();});
 window.addEventListener('resize',()=>{if(!panel.hidden){position();onToggle();}});read();paint();return {close};
}
