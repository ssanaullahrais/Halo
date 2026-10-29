import {invoke} from '@tauri-apps/api/core';
import {listen} from '@tauri-apps/api/event';
import {createElement,Download,LoaderCircle} from 'lucide';

const install=document.getElementById('installUpdate'),label=document.getElementById('updateMessage');
let status={phase:'idle',message:'Updates are checked and installed automatically.'},installing=false;
function paint(next=status){
 status=next;
 const busy=installing||next.phase==='installing';
 install.hidden=!busy&&next.phase!=='ready';install.disabled=busy;
 install.replaceChildren(createElement(busy?LoaderCircle:Download,{'aria-hidden':'true'}),Object.assign(document.createElement('span'),{textContent:busy?'Updating…':'Update & restart'}));
 install.classList.toggle('is-working',busy);
 if(busy)install.setAttribute('aria-busy','true');else install.removeAttribute('aria-busy');
 label.textContent=(next.version?`Halo ${next.version} · `:'')+next.message+(next.phase==='downloading'&&next.percent!=null?` (${next.percent}%)`:'');
}
if(install){
 install.onclick=async()=>{
  if(installing||status.phase!=='ready')return;const pending=status;installing=true;paint({...pending,phase:'installing',message:'Installing update…'});
  try{await invoke('install_update');}
  catch(e){status={...pending,phase:'ready',message:String(e)};}
  finally{installing=false;paint();}
 };
 paint();
 listen('halo://update',e=>paint(e.payload)).catch(console.error);
 invoke('update_status').then(s=>{if(!installing)paint(s);}).catch(()=>{if(!installing)paint({phase:'unavailable',message:'Automatic updates will retry in the background.'});});
}
