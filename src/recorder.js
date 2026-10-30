import {closeRecordingDropdown} from './recording-select';
import {createElement,GripVertical,Monitor,AppWindow,Scan,ChevronDown,Circle,Pause,Play,Square,Settings,X,Mic,Volume2,Check,CircleCheck,FolderOpen,Video,LoaderCircle,MicOff,VolumeX,PenLine,Share2,Trash2,CircleAlert,RotateCcw} from 'lucide';
import {invoke} from '@tauri-apps/api/core';
import {getCurrentWindow,currentMonitor,LogicalSize,LogicalPosition} from '@tauri-apps/api/window';
import {listen,emitTo} from '@tauri-apps/api/event';
import {mountRecordingControls} from './recording-controls';
import './recorder.css';
const $=id=>document.getElementById(id);const win=getCurrentWindow();let state={phase:'idle'},busy=false,selectingArea=false,output=null,originalOutput=null,lastOutput=null,countdownToken=0,lastError='',lastBackendError='',previousHeight=0,previewKey='',previewTimer,countdownRemaining=null,pendingLabel='',revealAnimation=null,savedTimer=null,lastReveal=0,widgetClosed=false,widgetGeneration=0,lastElapsed=0,annotationEditing=false,annotationPending=false;
let countdownOpening=null,countdownId=null;
const settings=$('recorderSettings'),menu=$('sourceMenu');
const icon=node=>createElement(node,{'aria-hidden':'true','stroke-width':1.8});
$('recorderDrag').replaceChildren(icon(GripVertical));for(const [id,node]of [['recorderGear',Settings],['hideRecorder',X],['micToggle',Mic],['systemToggle',Volume2]])$(id).replaceChildren(icon(node));$('pauseRecorder').replaceChildren(icon(Pause),document.createTextNode('Pause'));$('sourceToggle').querySelector('svg').replaceWith(icon(Monitor));$('sourceToggle').querySelector('.chevron').replaceChildren(icon(ChevronDown));$('finishRecorder').querySelector('svg').replaceWith(icon(Square));$('recordToggle').querySelector('.record-dot').replaceChildren(icon(Circle));
function revealToolbar(){const bar=$('recorderBar');revealAnimation?.cancel();if(matchMedia('(prefers-reduced-motion: reduce)').matches||bar.matches(':hover')||performance.now()-lastReveal<300)return;lastReveal=performance.now();revealAnimation=bar.animate([{opacity:0,transform:'translateY(-14px)'},{opacity:1,transform:'translateY(0)'}],{duration:240,easing:'cubic-bezier(.2,.8,.2,1)'});}
$('recorderBar').addEventListener('pointerenter',()=>{if(revealAnimation?.playState==='running')revealAnimation.finish();});
function setAnnotationEditing(editing){annotationEditing=!!editing;$('annotationToggle').classList.toggle('active',annotationEditing);$('annotationToggle').setAttribute('aria-pressed',String(annotationEditing));}
setAnnotationEditing(false);
$('savedRecordingIcon').replaceChildren(icon(CircleCheck));$('annotationToggle').replaceChildren(icon(PenLine));$('removeRecording').replaceChildren(icon(Trash2));$('dismissResult').replaceChildren(icon(X));$('openRecording').replaceChildren(icon(Play),document.createTextNode('Play video'));$('revealRecording').replaceChildren(icon(FolderOpen),document.createTextNode('Open folder'));
await listen('halo://recorder-closed',()=>{setAnnotationEditing(false);widgetClosed=true;widgetGeneration++;countdownToken++;selectingArea=false;busy=false;countdownRemaining=null;pendingLabel='';lastError='';lastBackendError='';clearTimeout(previewTimer);clearTimeout(savedTimer);previewKey='off';settings.hidden=true;menu.hidden=true;$('recorderResult').hidden=true;});
await listen('halo://recorder-opened',()=>{widgetClosed=false;revealToolbar();});
function setText(element,value){if(element.textContent!==value)element.textContent=value;}
function status(text,error=false){const element=$('recorderStatus');if(error){lastError='The recording could not be completed. Please try again.';console.error(text);invoke('recorder_log_error',{message:String(text)}).catch(()=>{});element.textContent=lastError;element.hidden=false;element.classList.remove('sr-only');}else{if(!text)lastError='';element.classList.remove('sr-only');setText(element,text);element.hidden=!text||/^(Starting|Saving|Pausing|Finishing)/.test(text);}element.classList.toggle('error',error);}
function paintAudioButton(id,node){
 const wave=document.createElement('span');wave.className='audio-wave';wave.setAttribute('aria-hidden','true');
 for(let n=0;n<4;n++){const bar=document.createElement('i');bar.style.setProperty('--wave-scale',[.55,.85,1,.65][n]);wave.append(bar);}
 $(id).replaceChildren(icon(node),wave);
}
const audioLevels={mic:0,system:0};
function paintAudioLevels(){
 const {config}=controls.get(),live=state.phase==='recording'||(state.phase==='idle'&&state.preview_active);
 for(const [kind,id,key,gain]of [['mic','micToggle','microphone','mic_gain'],['system','systemToggle','system_audio','system_gain']]){
  const enabled=!!config[key],peak=live&&enabled?Math.max(0,Number(state[`${kind}_peak`])||0)*config[gain]:0;
  const level=peak>0.0001?Math.max(0,Math.min(1,(Math.log10(peak)+4)/4)):0;
  audioLevels[kind]=live&&enabled?Math.max(level,audioLevels[kind]*.6):0;
  const button=$(id);button.style.setProperty('--audio-level',audioLevels[kind].toFixed(3));button.classList.toggle('audio-live',enabled&&peak>0.001);
  button.setAttribute('aria-label',`${kind==='mic'?'Microphone':'System audio'}: ${!enabled?'muted':peak>0.001?'receiving audio':'on, no sound detected'}`);
 }
}
function paint({config,target}){paintAudioButton('micToggle',config.microphone?Mic:MicOff);paintAudioButton('systemToggle',config.system_audio?Volume2:VolumeX);$('sourceToggle').querySelector('svg').replaceWith(icon({screen:Monitor,window:AppWindow,area:Scan}[target.mode]));$('sourceLabel').textContent={screen:'Screen',window:'Window',area:'Area'}[target.mode];$('micToggle').classList.toggle('muted',!config.microphone);$('micToggle').ariaPressed=String(config.microphone);$('systemToggle').classList.toggle('muted',!config.system_audio);$('systemToggle').ariaPressed=String(config.system_audio);document.querySelectorAll('[data-mode]').forEach(b=>(b.classList.toggle('active',b.dataset.mode===target.mode),b.setAttribute('aria-pressed',String(b.dataset.mode===target.mode))));$('sourceDetails').textContent=target.rect?`${target.rect.w} × ${target.rect.h} pixels`:'';}
const controls=await mountRecordingControls(settings,{onChange:paint});
let sourceOptionsGeneration=0;
function sourceOptions(){
 const generation=++sourceOptionsGeneration;
 const {target}=controls.get(),isWindow=target.mode==='window',list=$('sourcePicker');
 $('sourcePickerLabel').textContent=isWindow?'Open windows':'Available displays';list.setAttribute('aria-label',isWindow?'Available windows':'Available displays');list.replaceChildren();
 list.classList.toggle('window-thumbnails',isWindow);const items=isWindow?controls.options.windows:controls.options.monitors;
 const pending=[];
 for(const [index,item] of items.entries()){
  const selected=item.id===(isWindow?target.window:target.monitor),row=document.createElement('button');row.type='button';row.className='source-option';row.dataset.sourceId=item.id;row.setAttribute('role','option');row.setAttribute('aria-selected',String(selected));
  const copy=document.createElement('span');copy.className='source-option-copy';const name=document.createElement('strong');name.textContent=isWindow?item.name:`Display ${index+1}`;const detail=document.createElement('small');detail.textContent=item.minimized?'Restore window to preview':`${item.w} × ${item.h}${isWindow?'':' pixels'}`;row.disabled=!!item.minimized;copy.append(name,detail);if(isWindow){row.classList.add('source-window-card');const preview=document.createElement('img');preview.className='source-window-thumbnail';preview.alt='';preview.hidden=true;row.append(preview);if(item.minimized){preview.hidden=true;row.classList.add('preview-unavailable');}else{row.classList.add('preview-loading');detail.textContent='Preview loading…';pending.push({row,preview,detail,id:item.id});}}row.append(icon(isWindow?AppWindow:Monitor),copy);if(selected)row.append(icon(Check));list.append(row);
 }
 if(!items.length){const empty=document.createElement('p');empty.className='source-empty';empty.textContent=isWindow?'No available windows. Open a window and try again.':'No connected displays found.';list.append(empty);}
 (async()=>{for(const {row,preview,detail,id} of pending){if(generation!==sourceOptionsGeneration)return;try{const data=await invoke('recorder_window_thumbnail',{window:id});if(generation!==sourceOptionsGeneration)return;if(row.isConnected){preview.src=data;preview.hidden=false;row.classList.remove('preview-loading');}}catch(error){if(generation!==sourceOptionsGeneration)return;row.classList.remove('preview-loading');row.classList.add('preview-unavailable');detail.textContent=String(error).includes('Restore')?'Restore window to preview':'Preview unavailable';}}})();
}
for(const b of menu.querySelectorAll('[data-mode]')){const label=b.textContent;b.replaceChildren(icon({screen:Monitor,window:AppWindow,area:Scan}[b.dataset.mode]),document.createTextNode(label));}

async function choose(mode){if(busy||selectingArea||!['idle','exporting'].includes(state.phase))return;controls.setTarget({mode,rect:null});sourceOptions();if(mode==='area'){menu.hidden=true;$('sourceToggle').ariaExpanded='false';status('');}else menu.hidden=false;}
$('sourceToggle').onclick=async()=>{settings.hidden=true;$('recorderGear').classList.remove('active');menu.hidden=!menu.hidden;$('sourceToggle').ariaExpanded=String(!menu.hidden);if(!menu.hidden){sourceOptions();try{const fresh=await invoke('recorder_options');controls.options.windows=fresh.windows;controls.options.monitors=fresh.monitors;}catch{}if(!menu.hidden)sourceOptions();}};
menu.querySelectorAll('[data-mode]').forEach(b=>b.onclick=()=>choose(b.dataset.mode));
$('sourcePicker').onclick=e=>{const row=e.target.closest('[data-source-id]');if(!row)return;controls.setTarget({[controls.get().target.mode==='window'?'window':'monitor']:Number(row.dataset.sourceId)});menu.hidden=true;$('sourceToggle').ariaExpanded='false';$('sourceToggle').focus();};
$('sourcePicker').onkeydown=e=>{const rows=[...$('sourcePicker').querySelectorAll('[data-source-id]')];const index=rows.indexOf(document.activeElement);let next;if(e.key==='ArrowDown')next=(index+1)%rows.length;if(e.key==='ArrowUp')next=(index+rows.length-1)%rows.length;if(e.key==='Home')next=0;if(e.key==='End')next=rows.length-1;if(next!==undefined&&rows.length){e.preventDefault();rows[next].focus();}};

function closeSettings(){closeRecordingDropdown();settings.hidden=true;$('recorderGear').classList.remove('active');$('recorderGear').ariaExpanded='false';syncPreview();}
win.onFocusChanged(({payload:focused})=>{if(!focused){closeSettings();menu.hidden=true;$('sourceToggle').ariaExpanded='false';}});
document.addEventListener('pointerdown',e=>{if(!settings.hidden&&!settings.contains(e.target)&&!$('recorderGear').contains(e.target)&&!e.target.closest('.rec-dropdown-menu'))closeSettings();if(!menu.hidden&&!menu.contains(e.target)&&!$('sourceToggle').contains(e.target)){menu.hidden=true;$('sourceToggle').ariaExpanded='false';}});
document.addEventListener('keydown',e=>{if(e.key==='Escape'&&!menu.hidden){menu.hidden=true;$('sourceToggle').ariaExpanded='false';$('sourceToggle').focus();}});

await listen('halo://recording-area',async e=>{menu.hidden=true;status('');if(widgetClosed||!selectingArea)return;selectingArea=false;await win.show();revealToolbar();const request=controls.get();await start({target:{...request.target,mode:'area',monitor:e.payload.monitor,rect:e.payload.rect},config:request.config},true);});
async function cancelStartingCountdown(){
 if(!busy||pendingLabel!=='Starting'||countdownRemaining===null)return;
 const generation=widgetGeneration,token=++countdownToken,opening=countdownOpening;
 countdownRemaining=null;pendingLabel='Cancelling';$('recordToggle').disabled=true;
 try{await invoke('recorder_close_countdown',{id:countdownId});const id=await opening?.catch(()=>null);if(id!=null)await invoke('recorder_close_countdown',{id});}
 catch(e){if(generation===widgetGeneration)status(String(e),true);}
 finally{if(generation===widgetGeneration&&token===countdownToken){countdownId=null;countdownOpening=null;busy=false;pendingLabel='';status('');$('recordLabel').textContent='Record';$('recordToggle').disabled=false;}}
}
await listen('halo://countdown-cancelled',async e=>{if(widgetClosed||!busy||pendingLabel!=='Starting'||(countdownId!==null&&e.payload.id!==countdownId))return;await cancelStartingCountdown();});
await listen('halo://annotation-editing',e=>setAnnotationEditing(e.payload));
$('annotationToggle').onclick=async()=>{if(annotationPending)return;annotationPending=true;$('annotationToggle').disabled=true;const editing=!annotationEditing;try{await invoke('recorder_annotations_edit',{editing});setAnnotationEditing(editing);}catch(e){status(String(e),true);}finally{annotationPending=false;$('annotationToggle').disabled=!['recording','paused'].includes(state.phase);}};
await listen('halo://recording-area-cancelled',async()=>{if(widgetClosed||!selectingArea)return;selectingArea=false;status('');await win.show();revealToolbar();});
async function start(request=controls.get(),areaConfirmed=false){if(widgetClosed||busy||selectingArea||!['idle','exporting'].includes(state.phase))return;const generation=widgetGeneration;lastError='';lastBackendError='';status('');if(request.target.mode==='area'&&!areaConfirmed){selectingArea=true;menu.hidden=true;settings.hidden=true;status('');try{await win.hide();await controls.selectArea();}catch(e){selectingArea=false;if(generation!==widgetGeneration)return;await win.show();revealToolbar();status(String(e),true);}return;}setAnnotationEditing(false);lastElapsed=0;$('timer').textContent='00:00';busy=true;pendingLabel='Starting';menu.hidden=true;settings.hidden=true;$('recorderResult').hidden=true;const token=++countdownToken;let id=null;try{if(request.config.countdown>0){countdownRemaining=request.config.countdown;countdownOpening=invoke('recorder_countdown_open',{target:request.target,seconds:request.config.countdown});id=await countdownOpening;if(token!==countdownToken||generation!==widgetGeneration)return;countdownId=id;}for(let n=request.config.countdown;n>0;n--){if(token!==countdownToken)return;status(`Starting in ${n} · click Cancel to stop`);countdownRemaining=n;await emitTo('recorder-countdown','halo://countdown-tick',{id,value:n});$('recordLabel').textContent='Starting…';$('recordToggle').title='Cancel countdown';await new Promise(r=>setTimeout(r,1000));}if(token!==countdownToken)return;countdownRemaining=null;$('recordToggle').disabled=true;if(request.config.countdown>0)await emitTo('recorder-countdown','halo://countdown-tick',{id,value:0});if(token!==countdownToken||generation!==widgetGeneration)return;status('Starting…');$('recordToggle').disabled=true;await invoke('recorder_start',request);if(generation!==widgetGeneration)return;if(request.config.countdown>0){await emitTo('recorder-countdown','halo://countdown-complete',{id});await new Promise(r=>setTimeout(r,170));await invoke('recorder_close_countdown',{id});}status('');}catch(e){if(generation===widgetGeneration&&token===countdownToken){lastError=String(e);status(lastError,true);}}finally{if(id!==null)await invoke('recorder_close_countdown',{id}).catch(()=>{});if(generation===widgetGeneration&&token===countdownToken){busy=false;countdownRemaining=null;countdownOpening=null;countdownId=null;pendingLabel='';}}}
async function finishRecording(){revealAnimation?.finish();if(busy||!['recording','paused'].includes(state.phase))return;busy=true;pendingLabel='Saving';$('recordToggle').disabled=true;$('finishRecorder').disabled=true;try{status('Saving…');await invoke('recorder_stop');}catch(e){if(!widgetClosed)status(String(e),true);}finally{busy=false;countdownRemaining=null;pendingLabel='';/* Only close the countdown that belongs to this take: an id-less close could hit the next take's countdown. */if(countdownId!==null)invoke('recorder_close_countdown',{id:countdownId}).catch(()=>{});}}
$('finishRecorder').onclick=finishRecording;
let restartTimer=null;
function resetRestartConfirm(){clearTimeout(restartTimer);const b=$('restartRecorder');if(!b.classList.contains('confirm'))return;b.classList.remove('confirm');b.title='Discard this take and start over';b.querySelector('span').textContent='Restart';}
$('restartRecorder').prepend(icon(RotateCcw));
// Two clicks, so a stray tap never throws a recording away: the first arms it, the second discards and starts over.
$('restartRecorder').onclick=async()=>{const b=$('restartRecorder');if(!b.classList.contains('confirm')){b.classList.add('confirm');b.title='Click again to discard this take';b.querySelector('span').textContent='Discard?';status('Click Restart again to discard this take and start over.');restartTimer=setTimeout(()=>{resetRestartConfirm();status('');},4000);return;}resetRestartConfirm();status('');try{await invoke('recorder_cancel');await invoke('open_recorder');}catch(e){status(String(e),true);}};

$('recordToggle').onclick=async()=>{if(busy){await cancelStartingCountdown();return;}try{state=await invoke('recorder_state');}catch{}if(busy){await cancelStartingCountdown();return;}if(['recording','paused'].includes(state.phase))await togglePause();else if(['pausing','resuming','saving','preparing'].includes(state.phase))return;else await start();};
async function togglePause(){try{await invoke(state.phase==='paused'?'recorder_resume':'recorder_pause');}catch(e){status(String(e),true);}}
$('pauseRecorder').onclick=togglePause;
$('recorderGear').onclick=()=>{menu.hidden=true;settings.hidden=!settings.hidden;$('recorderGear').classList.toggle('active',!settings.hidden);$('recorderGear').ariaExpanded=String(!settings.hidden);};$('hideRecorder').onclick=async()=>{setAnnotationEditing(false);widgetClosed=true;widgetGeneration++;countdownToken++;selectingArea=false;busy=false;countdownRemaining=null;pendingLabel='';clearTimeout(previewTimer);previewKey='off';settings.hidden=true;menu.hidden=true;$('recorderResult').hidden=true;status('');await win.hide();try{await invoke('recorder_close');}catch(e){console.error(e);}};$('recorderDrag').onpointerdown=()=>win.startDragging();
for(const [id,key,kind]of [['micToggle','microphone','microphone'],['systemToggle','system_audio','system']])$(id).onclick=async()=>{const {config}=controls.get();const enabled=!config[key];$(id).disabled=true;try{if(['recording','paused'].includes(state.phase))await invoke('recorder_toggle_audio',{kind,enabled});controls.setConfig({[key]:enabled});}catch(e){status(String(e),true);}finally{$(id).disabled=false;}};
$('openProject').replaceChildren(icon(FolderOpen),document.createTextNode('Open project…'));
$('exportAgain').replaceChildren(icon(RotateCcw));
async function openProject(path=null){
 if(busy||selectingArea||!['idle','exporting'].includes(state.phase))return;
 busy=true;pendingLabel='Opening';menu.hidden=true;settings.hidden=true;status('');
 try{
  const opened=await invoke('recorder_open_project',{path});
  if(opened){lastOutput=null;lastError='';lastBackendError='';state=await invoke('recorder_state');}
 }catch(error){lastBackendError=String(error);status(String(error));}
 finally{busy=false;pendingLabel='';}
}
$('openProject').onclick=()=>openProject();
$('exportAgain').onclick=()=>state.project_path&&openProject(state.project_path);
$('openRecording').onclick=()=>output&&invoke('open_path',{path:output});$('cancelExport').onclick=()=>invoke('recorder_cancel_export').catch(e=>status(String(e),true));$('openOriginal').replaceChildren(icon(Play),document.createTextNode('Play original'));$('openOriginal').onclick=()=>originalOutput&&invoke('open_path',{path:originalOutput});$('revealRecording').onclick=()=>output&&invoke('reveal_path',{path:output});function dismissSaved(){clearTimeout(savedTimer);$('recorderResult').hidden=true;}function scheduleSavedHide(delay=6000){clearTimeout(savedTimer);if(state.phase==='exporting')return;savedTimer=setTimeout(dismissSaved,delay);}$('dismissResult').onclick=dismissSaved;$('recorderResult').onpointerenter=()=>clearTimeout(savedTimer);$('recorderResult').onpointerleave=()=>scheduleSavedHide(3000);$('recorderResult').onfocusin=()=>clearTimeout(savedTimer);$('recorderResult').onfocusout=e=>{if(!$('recorderResult').contains(e.relatedTarget))scheduleSavedHide(3000);};
function syncPreview(){const {config}=controls.get();const enabled=!widgetClosed&&['idle','exporting'].includes(state.phase)&&!selectingArea&&(config.microphone||config.system_audio);const key=enabled?JSON.stringify([config.microphone,config.system_audio,config.microphone_id]):'off';if(key===previewKey)return;previewKey=key;clearTimeout(previewTimer);previewTimer=setTimeout(async()=>{try{await invoke(enabled?'recorder_preview_start':'recorder_preview_stop',enabled?{config}:{});}catch(e){if(previewKey===key){console.error(e);invoke('recorder_log_error',{message:`Audio preview: ${e}`}).catch(()=>{});}}},200);}
window.addEventListener('beforeunload',()=>invoke('recorder_preview_stop').catch(()=>{}));
let micAutoDisabled=false,systemAutoDisabled=false;
async function poll(){try{state=await invoke('recorder_state');const active=['recording','paused','pausing','resuming'].includes(state.phase),locked=!['idle','exporting'].includes(state.phase);controls.meters(state);paintAudioLevels();
 if(!locked){micAutoDisabled=false;systemAutoDisabled=false;}
 const liveConfig=controls.get().config;
 if(state.microphone===false&&liveConfig.microphone&&!micAutoDisabled){micAutoDisabled=true;controls.setConfigLocal({microphone:false});status(active?'Microphone unavailable — recording without it.':'Microphone unavailable.');}
 if(state.system_audio===false&&liveConfig.system_audio&&!systemAutoDisabled){systemAutoDisabled=true;controls.setConfigLocal({system_audio:false});status(active?'System audio unavailable — recording without it.':'System audio unavailable.');}
 $('cancelExport').hidden=state.phase!=='exporting';syncPreview();controls.setBusy(locked);$('sourceToggle').disabled=locked||busy||selectingArea;for(const id of ['micToggle','systemToggle'])$(id).disabled=busy||selectingArea||['preparing','resuming','pausing','saving'].includes(state.phase);const hideSettings=locked||busy||selectingArea;$('recorderGear').style.visibility='visible';$('recorderGear').disabled=hideSettings;$('recorderGear').ariaHidden='false';$('sourceToggle').hidden=false;$('micToggle').hidden=false;$('systemToggle').hidden=false;$('annotationToggle').hidden=!active;$('annotationToggle').disabled=annotationPending||!['recording','paused'].includes(state.phase);setAnnotationEditing(active&&annotationEditing);$('pauseRecorder').hidden=true;$('finishRecorder').hidden=!active&&!['saving','exporting'].includes(state.phase);{const f=$('finishRecorder'),saving=['saving','exporting'].includes(state.phase),pct=Math.max(0,Math.min(100,Math.floor(state.save_percent||0)));f.classList.toggle('saving',saving);f.style.setProperty('--save',`${pct}%`);const label=f.querySelector('span');const stage=state.phase==='exporting'?(state.save_stage||'preparing'):'saving';f.dataset.saveStage=stage;const text=saving?(stage==='queued'?'Queued':stage==='cancelling'?'Cancelling…':['finalizing','publishing'].includes(stage)?'Finalizing…':`${({preparing:'Preparing',encoding:'Exporting',finalizing:'Finalizing',saving:'Saving'})[stage]||'Preparing'} ${stage==='encoding'&&pct===0?Number((state.save_percent||0).toFixed(2)):pct}%`):'Finish';if(label&&label.textContent!==text)label.textContent=text;}$('restartRecorder').hidden=state.phase!=='paused';if(state.phase!=='paused')resetRestartConfirm();$('pauseRecorder').disabled=!['recording','paused'].includes(state.phase);$('finishRecorder').disabled=!['recording','paused'].includes(state.phase)||busy;$('pauseRecorder').title=state.phase==='paused'?'Resume':'Pause';const pauseIcon=state.phase==='paused'?'resume':'pause';if($('pauseRecorder').dataset.iconState!==pauseIcon){$('pauseRecorder').dataset.iconState=pauseIcon;$('pauseRecorder').replaceChildren(icon(pauseIcon==='resume'?Play:Pause),document.createTextNode(pauseIcon==='resume'?'Resume':'Pause'));}$('timer').hidden=true;lastElapsed=active?Math.max(lastElapsed,state.elapsed||0):0;const t=Math.floor(lastElapsed);setText($('timer'),`${Math.floor(t/60).toString().padStart(2,'0')}:${(t%60).toString().padStart(2,'0')}`);$('recorderBar').dataset.phase=state.phase;$('recordToggle').hidden=state.phase==='saving';$('recordToggle').disabled=selectingArea||pendingLabel==='Cancelling'||(busy&&pendingLabel==='Opening')||(busy&&pendingLabel==='Starting'&&countdownRemaining===null)||['preparing','pausing','resuming','saving'].includes(state.phase);const ctaText=countdownRemaining!==null?'Starting…':selectingArea?'Select…':({preparing:pendingLabel==='Opening'?'Opening':'Starting',saving:'Saving',exporting:'Record',pausing:'Pausing'}[state.phase]||(active?$('timer').textContent:pendingLabel||'Record'));setText($('recordLabel'),ctaText);const dot=$('recordToggle').querySelector('.record-dot');const dotState=['preparing','saving'].includes(state.phase)?state.phase:active?state.phase:'idle';if(dot.dataset.state!==dotState){dot.dataset.state=dotState;dot.replaceChildren(icon(['preparing','saving'].includes(state.phase)?LoaderCircle:active?(state.phase==='paused'?Play:Pause):lastError?CircleAlert:Video));}$('recordToggle').ariaLabel=active?(state.phase==='paused'?'Resume recording':'Pause recording'):'Start recording';$('recordToggle').title=countdownRemaining!==null?'Cancel countdown':active?(state.phase==='paused'?'Resume recording':'Pause recording'):'Start recording';if(widgetClosed){$('recorderResult').hidden=true;}if(locked){if(!state.output)$('recorderResult').hidden=true;menu.hidden=true;}if(!busy){if(state.error){if(state.output){lastBackendError=state.error;status('');}else if(state.error!==lastBackendError){lastBackendError=state.error;status(state.error,true);}}else{lastBackendError='';if(state.phase==='exporting')status('');else if(state.phase==='saving')status('Saving original video…');else if(!lastError)status('');}}if(!widgetClosed&&state.output&&state.output!==lastOutput){output=lastOutput=state.output;$('recordingFileName').textContent=output.split(/[\\/]/).pop();$('recordingFileName').title=output;showSavedResult(output);}if(state.output)renderResultState();}catch(e){status(String(e),true);}if(!polled){polled=true;requestAnimationFrame(()=>requestAnimationFrame(async()=>{try{await resizeToContent();}catch{}maybeReveal(true);}));}setTimeout(poll,250);}invoke('recorder_state').then(initial=>{lastOutput=initial.output;poll();});
let resizeChain=Promise.resolve();let resizeKey='';
// Matches the 615-logical-px width Rust's `open()` assumes when it first centers this window
// (src-tauri/src/recorder.rs). The real first-render width is almost never exactly 615 (it
// depends on which buttons are visible), so treating this as the starting point lets the very
// first resize self-correct the center instead of only correcting drift from then on.
let lastResizeWidth=615,lastBarWidth=0,firstPlaced=false,firstSized=false,polled=false,revealed=false,renderReady=false;
// The window is created hidden; it appears once the first layout, position and state are settled, so nothing flashes.
function maybeReveal(){if(revealed||!renderReady||!firstSized||!polled)return;revealed=true;invoke('recorder_widget_ready').then(revealToolbar).catch(()=>{revealed=false;});}
function resizeToContent(){if(!renderReady)return Promise.resolve();const bounds=$('recorderApp').getBoundingClientRect();const barWidth=Math.ceil($('recorderBar').getBoundingClientRect().width);const width=Math.ceil(bounds.width),height=Math.min(900,Math.ceil(bounds.height+3));const key=`${width}:${height}`;if(key!==resizeKey){resizeKey=key;resizeChain=resizeChain.then(async()=>{
 // The window width now tracks visible content (pause/finish, settings), so re-center it
 // horizontally around its previous center instead of letting it grow/shrink from the left edge.
 if(!firstPlaced){
  // First layout after the window opens: Rust only guessed a width, so centre the bar for its real width.
  firstPlaced=true;
  try{const scale=await win.scaleFactor(),monitor=await currentMonitor(),pos=await win.outerPosition();if(monitor)await win.setPosition(new LogicalPosition((monitor.position.x+monitor.size.width/2)/scale-width/2,pos.y/scale));}catch{}
 }else if(lastBarWidth!==barWidth){
  // Only the bar itself grows or shrinks around its centre. Opening settings or the saved card widens the window to
  // the right, so the bar stays exactly where it was and the card opens below it.
  try{const scale=await win.scaleFactor();const pos=await win.outerPosition();await win.setPosition(new LogicalPosition(pos.x/scale+(lastBarWidth-barWidth)/2,pos.y/scale));}catch{}
 }
 lastBarWidth=barWidth;
 lastResizeWidth=width;
 await win.setSize(new LogicalSize(width,height));
 firstSized=true;
}).catch(e=>console.warn('Recorder resize failed',e));}return resizeChain;}
new ResizeObserver(()=>{requestAnimationFrame(resizeToContent);}).observe($('recorderApp'));
async function showSavedResult(path){const card=$('recorderResult');card.style.visibility='hidden';card.hidden=false;loadSavedDetails(path);await resizeToContent();await new Promise(requestAnimationFrame);if(widgetClosed||output!==path)return;card.style.visibility='visible';if(!matchMedia('(prefers-reduced-motion: reduce)').matches)card.animate([{opacity:0,transform:'translateY(-4px)'},{opacity:1,transform:'translateY(0)'}],{duration:160,easing:'ease-out'});scheduleSavedHide();if(state.phase!=='exporting')invoke('reveal_path',{path}).catch(e=>status(String(e),true));}

function removeHoverTitles(){document.querySelectorAll('[title]').forEach(el=>{if(!el.hasAttribute('aria-label'))el.setAttribute('aria-label',el.title);el.removeAttribute('title');});}removeHoverTitles();new MutationObserver(removeHoverTitles).observe(document.getElementById('recorderApp'),{subtree:true,attributes:true,attributeFilter:['title'],childList:true});

function renderResultState(){
 const card=$('recorderResult'),pending=state.phase==='exporting',fallback=!!state.original_output&&state.original_output===output&&!!state.error;
 const kind=pending?'exporting':fallback?'original':'complete';
 originalOutput=state.original_output||null;
 card.setAttribute('aria-busy',String(pending));
 setText(document.querySelector('.saved-recording-copy strong'),pending||fallback?'Original video available':'Recording saved');
 setText($('recordingFileName'),pending?(state.save_stage==='queued'?'Final video queued. You can record again.':'The final version is still exporting.'):fallback?(state.error.includes('cancelled')?'Final export cancelled.':'The final version could not be completed.'):output?.split(/[\\/]/).pop()||'');
 $('openRecording').hidden=pending;
 $('openOriginal').hidden=!originalOutput||(!pending&&originalOutput===output);
 $('removeRecording').hidden=pending;
 $('exportAgain').hidden=state.phase!=='idle'||!state.project_path;$('exportAgain').disabled=busy;
 if(card.dataset.resultState!==kind){
  card.dataset.resultState=kind;
  $('savedRecordingIcon').replaceChildren(icon(pending?Video:fallback?CircleAlert:CircleCheck));
  $('openRecording').replaceChildren(icon(Play),document.createTextNode(fallback?'Play original':'Play video'));
 }
 $('savedRecordingThumbnail').hidden=!$('savedRecordingThumbnail').getAttribute('src');
}
async function loadSavedDetails(path){
 $('savedRecordingThumbnail').hidden=true;$('savedRecordingThumbnail').removeAttribute('src');renderResultState();
 try{
  const [info,thumbnail]=await Promise.all([invoke('recorder_output_info'),invoke('thumbnail',{path})]);
  if(output!==path)return;
  const seconds=Math.floor(info.duration);
  $('savedRecordingDetails').textContent=`${Math.floor(seconds/60).toString().padStart(2,'0')}:${(seconds%60).toString().padStart(2,'0')} · ${(info.bytes/1048576).toFixed(1)} MB${info.original_path&&info.original_path!==path?' · + original':''}${info.project_path?' · Project saved':''}`;
  if(thumbnail?.data)$('savedRecordingThumbnail').src=thumbnail.data;
  renderResultState();
 }catch{}
}
$('removeRecording').onclick=async()=>{if(!confirm('Move this recording to the Recycle Bin?'))return;try{await invoke('recorder_recycle_output');dismissSaved();}catch(e){status(String(e),true);}};

// Use the countdown's font/layout readiness gate before showing the native window.
$('recorderApp').getBoundingClientRect();
await document.fonts.ready;
await new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)));
renderReady=true;
await resizeToContent();
maybeReveal();
