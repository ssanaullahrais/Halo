import {invoke} from '@tauri-apps/api/core';
import {listen} from '@tauri-apps/api/event';

const button=document.getElementById('checkUpdates'),install=document.getElementById('installUpdate'),label=document.getElementById('updateMessage');
function paint(s){
 const working=['checking','downloading','installing'].includes(s.phase);
 button.disabled=working;install.hidden=s.phase!=='ready';install.disabled=working;
 label.textContent=s.message+(s.phase==='downloading'&&s.percent!==null?` (${s.percent}%)`:'');
 if(s.version)label.textContent=`Halo ${s.version} · ${label.textContent}`;
}
if(button){
 button.onclick=async()=>{try{paint(await invoke('check_updates'));}catch{label.textContent='Could not check for updates. Try again later.';button.disabled=false;}};
 install.onclick=async()=>{install.disabled=true;try{await invoke('install_update');}catch(e){label.textContent=String(e).includes('Finish your')?String(e):'Could not install the update. Please try again.';install.disabled=false;}};
 listen('halo://update',e=>paint(e.payload));invoke('update_status').then(paint);
}
