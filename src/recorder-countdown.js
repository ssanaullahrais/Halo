import {invoke} from '@tauri-apps/api/core';
import {listen} from '@tauri-apps/api/event';
import {createElement,LoaderCircle} from 'lucide';
import './recorder-countdown.css';

const card=document.getElementById('countdownCard'),count=document.getElementById('count'),ring=document.getElementById('countdownProgress'),starting=document.getElementById('countdownStarting'),caption=document.getElementById('countdownCaption');
const circumference=2*Math.PI*65;
let currentId=-1,seconds=3,renderGeneration=0;
starting.append(createElement(LoaderCircle,{'aria-hidden':'true'}));
ring.style.strokeDasharray=String(circumference);

function tick(value){
 if(value===0){count.hidden=true;starting.hidden=false;caption.textContent='Starting capture…';return;}
 count.hidden=false;starting.hidden=true;count.textContent=value;
 ring.style.strokeDashoffset=String(circumference*(1-value/seconds));
 count.getAnimations().forEach(a=>a.cancel());
 if(!matchMedia('(prefers-reduced-motion: reduce)').matches)count.animate([{opacity:.4,transform:'scale(.9)'},{opacity:1,transform:'scale(1)'}],{duration:210,easing:'cubic-bezier(.2,.8,.2,1)'});
}

async function prepare(data){
 if(data.id<currentId)return;
 const generation=++renderGeneration;currentId=data.id;seconds=data.seconds||3;
 card.getAnimations().forEach(a=>a.cancel());card.classList.remove('complete');
 count.getAnimations().forEach(a=>a.cancel());count.hidden=false;starting.hidden=true;count.textContent=seconds;caption.textContent='Get ready';
 ring.style.transition='none';ring.style.strokeDashoffset='0';
 // This persistent WebView loads before the first Record click. Measure with
 // the real stylesheet and font before the backend allows any visible frame.
 card.getBoundingClientRect();await document.fonts.ready;
 if(generation!==renderGeneration)return;
 card.getBoundingClientRect();ring.style.removeProperty('transition');
 await invoke('recorder_countdown_ready',{id:currentId});
}

await listen('halo://countdown-reset',e=>prepare(e.payload));
await listen('halo://countdown-tick',e=>{if(e.payload.id===currentId)tick(e.payload.value);});
await listen('halo://countdown-complete',e=>{if(e.payload.id!==currentId)return;caption.textContent='Recording started';card.classList.add('complete');});
await prepare(await invoke('recorder_countdown_data'));
const cancel=()=>invoke('recorder_cancel_countdown',{id:currentId});
document.getElementById('cancelCountdown').onclick=cancel;
window.onkeydown=e=>{if(e.key==='Escape')cancel();};
