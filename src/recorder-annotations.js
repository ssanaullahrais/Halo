import {invoke} from '@tauri-apps/api/core';import {listen} from '@tauri-apps/api/event';import {createElement,MoveUpRight,Square,Type,Droplets,Eraser,Undo2,Trash2,Check,X} from 'lucide';import './recorder-annotations.css';
const $=id=>document.getElementById(id),canvas=$('annotationCanvas'),ctx=canvas.getContext('2d');const data=await invoke('recorder_annotation_data');canvas.width=data.width;canvas.height=data.height;let annotations=data.annotations.filter(a=>a.end===null),tool='arrow',stroke=3,draft=null,start=null,editing=true,snapshot=null,hovered=null,selected=null,editorShape=null,persistence=Promise.resolve(),lastDraftUpdate=0,transform=null;
const icons={arrow:MoveUpRight,rectangle:Square,text:Type,blur:Droplets,eraser:Eraser};for(const b of document.querySelectorAll('[data-tool]')){b.append(createElement(icons[b.dataset.tool],{'aria-hidden':'true'}),document.createTextNode({arrow:'Arrow',rectangle:'Rectangle',text:'Text',blur:'Blur',eraser:'Eraser'}[b.dataset.tool]));b.onclick=()=>{finishText();selected=null;paint();tool=b.dataset.tool;if(tool==='blur')loadBlurPreview();canvas.style.cursor=tool==='eraser'?'cell':'crosshair';document.querySelectorAll('[data-tool]').forEach(x=>x.classList.toggle('active',x===b));};}document.querySelector('[data-tool=arrow]').classList.add('active');for(const [id,node]of [['undoAnnotation',Undo2],['clearAnnotations',Trash2],['applyAnnotations',Check],['removeAnnotation',Trash2]])$(id).append(createElement(node,{'aria-hidden':'true'}));
function error(value){if(value)console.error(value);$('annotationError').hidden=!value;$('annotationError').textContent=value?'Could not apply this change. Please try again.':'';}
const point=e=>({x:Math.max(0,Math.min(canvas.width,e.clientX/innerWidth*canvas.width)),y:Math.max(0,Math.min(canvas.height,e.clientY/innerHeight*canvas.height))});
function bounds(a){return {x:Math.min(a.x,a.x+a.w),y:Math.min(a.y,a.y+a.h),w:Math.abs(a.w),h:Math.abs(a.h)};}
function handles(a){if(a.kind==='text')return [];if(a.kind==='arrow')return [{name:'start',x:a.x,y:a.y},{name:'end',x:a.x+a.w,y:a.y+a.h}];const b=bounds(a),l=b.x-8,r=b.x+b.w+8,t=b.y-8,bottom=b.y+b.h+8,cx=(l+r)/2,cy=(t+bottom)/2;return [{name:'nw',x:l,y:t},{name:'n',x:cx,y:t},{name:'ne',x:r,y:t},{name:'e',x:r,y:cy},{name:'se',x:r,y:bottom},{name:'s',x:cx,y:bottom},{name:'sw',x:l,y:bottom},{name:'w',x:l,y:cy}];}
function handleAt(p){return selected&&handles(selected).find(h=>Math.hypot((p.x-h.x)*innerWidth/canvas.width,(p.y-h.y)*innerHeight/canvas.height)<=9);}
function beginTransform(a,mode,p,e){selected=a;transform={mode,original:{...a},point:p};start=p;draft={...a,keyframes:[]};canvas.setPointerCapture(e.pointerId);paint();}
function transformTo(p){const a=transform.original,b=bounds(a),dx=p.x-transform.point.x,dy=p.y-transform.point.y,mode=transform.mode;
 if(mode==='move'){draft.x=a.x+Math.max(-b.x,Math.min(canvas.width-b.x-b.w,dx));draft.y=a.y+Math.max(-b.y,Math.min(canvas.height-b.y-b.h,dy));}
 else if(a.kind==='arrow'){if(mode==='start'){draft.x=p.x;draft.y=p.y;draft.w=a.x+a.w-p.x;draft.h=a.y+a.h-p.y;}else{draft.w=p.x-a.x;draft.h=p.y-a.y;}}
 else{let l=b.x,r=b.x+b.w,t=b.y,bottom=b.y+b.h;if(mode.includes('w'))l=Math.max(0,Math.min(r-8,b.x+dx));if(mode.includes('e'))r=Math.min(canvas.width,Math.max(l+8,b.x+b.w+dx));if(mode.includes('n'))t=Math.max(0,Math.min(bottom-8,b.y+dy));if(mode.includes('s'))bottom=Math.min(canvas.height,Math.max(t+8,b.y+b.h+dy));draft.x=l;draft.y=t;draft.w=r-l;draft.h=bottom-t;}
 queueDraft();paint();
}
function hit(p){return [...annotations].reverse().find(a=>{if(a===editorShape)return false;const b=bounds(a);if(p.x<b.x-8||p.y<b.y-8||p.x>b.x+b.w+8||p.y>b.y+b.h+8)return false;if(a.kind==='arrow'){const length=a.w*a.w+a.h*a.h;const t=length?Math.max(0,Math.min(1,((p.x-a.x)*a.w+(p.y-a.y)*a.h)/length)):0;return Math.hypot(p.x-a.x-t*a.w,p.y-a.y-t*a.h)<Math.max(11,a.stroke+7);}if(a.kind==='rectangle'){const edge=Math.max(9,a.stroke+5);return Math.min(Math.abs(p.x-b.x),Math.abs(p.x-b.x-b.w),Math.abs(p.y-b.y),Math.abs(p.y-b.y-b.h))<=edge;}return true;});}
function draw(a){
 const weight=a.kind==='arrow'?Math.min(a.stroke*2.3,a.stroke+Math.hypot(a.w,a.h)/100):a.stroke;
 ctx.save();ctx.strokeStyle=a.color;ctx.fillStyle=a.color;ctx.lineWidth=weight;ctx.lineCap='round';ctx.lineJoin='round';
 ctx.shadowColor='#102b4d52';ctx.shadowBlur=5;ctx.shadowOffsetY=2;
 const border=Math.max(2,weight*.65);
 if(a.kind==='arrow'){
  const x=a.x+a.w,y=a.y+a.h,angle=Math.atan2(a.h,a.w),length=Math.min(72,12+weight*2+Math.hypot(a.w,a.h)*.14,Math.hypot(a.w,a.h)*.7);
  const head=new Path2D();head.moveTo(x,y);head.lineTo(x-length*Math.cos(angle-.5),y-length*Math.sin(angle-.5));head.lineTo(x-length*.68*Math.cos(angle),y-length*.68*Math.sin(angle));head.lineTo(x-length*Math.cos(angle+.5),y-length*Math.sin(angle+.5));head.closePath();
  const shaft=new Path2D();shaft.moveTo(a.x,a.y);shaft.lineTo(x-length*.5*Math.cos(angle),y-length*.5*Math.sin(angle));
  ctx.strokeStyle='#ffffff';ctx.lineWidth=weight+border*2;ctx.stroke(shaft);ctx.lineWidth=border*2;ctx.fillStyle='#ffffff';ctx.fill(head);ctx.stroke(head);
  ctx.shadowColor='transparent';ctx.strokeStyle=a.color;ctx.fillStyle=a.color;ctx.lineWidth=weight;ctx.stroke(shaft);ctx.fill(head);
 }
 if(a.kind==='rectangle'){
  const b=bounds(a),path=new Path2D();path.roundRect(b.x,b.y,b.w,b.h,Math.min(8,b.w/2,b.h/2));
  ctx.strokeStyle='#ffffff';ctx.lineWidth=weight+border*2;ctx.stroke(path);ctx.shadowColor='transparent';ctx.strokeStyle=a.color;ctx.lineWidth=weight;ctx.stroke(path);
 }
 if(a.kind==='text'){ctx.font=`600 ${a.font_size}px "Inter Variable",Segoe UI,sans-serif`;a.text.split('\n').forEach((line,i)=>{ctx.strokeStyle='#ffffff';ctx.lineWidth=3;const y=a.y+a.font_size*(1+i*1.3);ctx.strokeText(line,a.x,y);ctx.fillText(line,a.x,y);});}
 if(a.kind==='blur'){const b=bounds(a);ctx.shadowColor='transparent';ctx.beginPath();ctx.rect(b.x,b.y,b.w,b.h);ctx.clip();if(snapshot){ctx.filter='blur(9px)';ctx.drawImage(snapshot,0,0,canvas.width,canvas.height);}else{ctx.fillStyle='#b2c4dfcc';ctx.fillRect(b.x,b.y,b.w,b.h);}ctx.filter='none';}
 ctx.restore();
}
let paintFrame=0;function paint(){if(!paintFrame)paintFrame=requestAnimationFrame(()=>{paintFrame=0;paintNow();});}
function paintNow(){
 ctx.clearRect(0,0,canvas.width,canvas.height);annotations.filter(a=>a!==editorShape&&a.id!==draft?.id).forEach(draw);if(draft)draw(draft);
 const remove=$('removeAnnotation');remove.hidden=!editing||!selected||!!draft||!!editorShape;
 if(selected&&editing&&!editorShape){const selection=transform?draft:selected,b=bounds(selection),pad=8,sx=innerWidth/canvas.width,sy=innerHeight/canvas.height;
  ctx.save();ctx.strokeStyle='#2379ff';ctx.lineWidth=1/sx;ctx.strokeRect(b.x-pad,b.y-pad,b.w+pad*2,b.h+pad*2);
  for(const {x,y}of handles(selection)){ctx.fillStyle='white';ctx.fillRect(x-3/sx,y-3/sy,6/sx,6/sy);ctx.strokeRect(x-3/sx,y-3/sy,6/sx,6/sy);}ctx.restore();
  remove.style.left=`${Math.max(8,Math.min(innerWidth-90,(b.x+b.w/2)*sx-40))}px`;
  remove.style.top=`${Math.max(8,b.y*sy-44)}px`;
 }
}
$('removeAnnotation').append(document.createTextNode('Delete'));$('removeAnnotation').ariaLabel='Delete selected drawing';

function queueDraft(force=false){
 if(!draft||(!force&&performance.now()-lastDraftUpdate<1000/(data.fps||60)))return;
 if(draft.kind==='blur'&&(Math.abs(draft.w)<4||Math.abs(draft.h)<4))return;
 if(Math.abs(draft.w)<2&&Math.abs(draft.h)<2)return;
 lastDraftUpdate=performance.now();const shape=draft,snapshot={...shape,keyframes:[]};
 persistence=persistence.then(async()=>{if(!shape.id){const saved=await invoke('recorder_annotation_add',{annotation:snapshot});shape.id=saved.id;shape.start=saved.start;}else{snapshot.id=shape.id;await invoke('recorder_annotation_update',{annotation:snapshot});}}).catch(error);
}
async function commit(annotation){queueDraft(true);const saved=persistence;draft=null;transform=null;await saved;if(annotation.id){const index=annotations.findIndex(a=>a.id===annotation.id);if(index<0)annotations.push(annotation);else annotations[index]=annotation;if(!draft)selected=annotation;}paint();}
async function loadBlurPreview(){try{const image=new Image();image.src=await invoke('recorder_annotation_snapshot');await image.decode();snapshot=image;paint();}catch(e){error(e);}}

function queueText(){if(!editorShape)return;const shape=editorShape;shape.text=$('annotationText').value;ctx.font=`600 ${shape.font_size}px "Inter Variable",Segoe UI,sans-serif`;const lines=shape.text.split('\n');shape.w=Math.min(canvas.width-shape.x,Math.max(110,...lines.map(l=>ctx.measureText(l).width+12)));shape.h=Math.min(canvas.height-shape.y,Math.max(32,lines.length*shape.font_size*1.3+6));const sx=innerWidth/canvas.width,sy=innerHeight/canvas.height;$('annotationText').style.width=`${Math.max(80,shape.w*sx)}px`;$('annotationText').style.height=`${Math.max(34,shape.h*sy)}px`;const snapshot={...shape,keyframes:[]};persistence=persistence.then(async()=>{if(!snapshot.text.trim()){if(shape.id){await invoke('recorder_annotation_end',{id:shape.id});annotations=annotations.filter(a=>a!==shape);shape.id=0;}return;}if(!shape.id){const saved=await invoke('recorder_annotation_add',{annotation:snapshot});shape.id=saved.id;shape.start=saved.start;annotations.push(shape);}else{snapshot.id=shape.id;await invoke('recorder_annotation_update',{annotation:snapshot});}}).catch(error).then(()=>paint());}
function finishText(){if(!editorShape)return persistence;queueText();editorShape=null;$('annotationText').hidden=true;paint();return persistence;}
async function erase(annotation){if(!annotation)return;await finishText();annotations=annotations.filter(a=>a.id!==annotation.id);if(hovered?.id===annotation.id)hovered=null;if(selected?.id===annotation.id)selected=null;paint();try{await invoke('recorder_annotation_end',{id:annotation.id});}catch(e){error(e);}}
canvas.onpointerdown=e=>{if(!editing||e.button!==0)return;e.preventDefault();finishText();const p=point(e);if(tool==='eraser'){selected=null;start=p;erase(hit(p));canvas.setPointerCapture(e.pointerId);return;}const handle=handleAt(p);if(handle){beginTransform(selected,handle.name,p,e);return;}const existing=hit(p);if(existing){beginTransform(existing,'move',p,e);return;}selected=null;start=p;hovered=null;const base={id:0,kind:tool,x:p.x,y:p.y,w:0,h:0,text:'',color:$('annotationColour').value,stroke,font_size:24,start:0,end:null};if(tool==='text'){editorShape=base;const input=$('annotationText');input.hidden=false;input.style.left=`${e.clientX}px`;input.style.top=`${e.clientY}px`;input.style.fontSize=`${24*innerWidth/canvas.width}px`;input.style.color=base.color;input.value='';input.style.width='180px';input.style.height='36px';input.focus();start=null;paint();return;}draft=base;canvas.setPointerCapture(e.pointerId);};
canvas.onpointermove=e=>{const p=point(e);if(transform){transformTo(p);return;}if(tool==='eraser'){hovered=hit(p);if(start)erase(hovered);paint();return;}if(!start||!draft){const handle=handleAt(p);hovered=hit(p);canvas.style.cursor=handle?({n:'ns-resize',s:'ns-resize',e:'ew-resize',w:'ew-resize',nw:'nwse-resize',se:'nwse-resize',ne:'nesw-resize',sw:'nesw-resize'}[handle.name]||'crosshair'):hovered?'move':'crosshair';return;}draft.w=p.x-start.x;draft.h=p.y-start.y;queueDraft();paint();};
canvas.onpointerup=async()=>{start=null;if(!draft)return;if((draft.kind==='blur'&&(Math.abs(draft.w)<4||Math.abs(draft.h)<4))||(Math.abs(draft.w)<2&&Math.abs(draft.h)<2)){await persistence;if(draft.id)await invoke('recorder_annotation_end',{id:draft.id});draft=null;transform=null;paint();return;}if(draft.kind==='blur'&&!snapshot)loadBlurPreview();await commit(draft);};$('annotationText').oninput=queueText;$('annotationText').onblur=finishText;$('annotationText').onkeydown=e=>{if((e.ctrlKey&&e.key==='Enter')||e.key==='Escape'){e.preventDefault();e.stopPropagation();finishText();}};$('removeAnnotation').onpointerdown=e=>{e.preventDefault();e.stopPropagation();};$('removeAnnotation').onclick=()=>erase(selected);
$('annotationStroke').onclick=e=>{stroke=stroke===3?6:stroke===6?2:3;e.currentTarget.textContent=`${stroke} px`;};$('undoAnnotation').onclick=async()=>{await finishText();const a=annotations.at(-1);if(a)await erase(a);};$('clearAnnotations').onclick=async()=>{await finishText();await invoke('recorder_annotation_end',{id:null});annotations=[];hovered=null;selected=null;paint();};$('applyAnnotations').onclick=async()=>{await finishText();invoke('recorder_annotations_edit',{editing:false});};
await listen('halo://annotation-editing',async e=>{if(!e.payload)await finishText();editing=e.payload;$('annotationTools').hidden=!editing;$('removeAnnotation').hidden=true;canvas.style.cursor=editing?'crosshair':'default';});await listen('halo://annotation-flush',async e=>{await finishText();if(draft)await commit(draft);await persistence;await invoke('recorder_annotations_flushed',{token:e.payload});});window.onkeydown=async e=>{if(e.target!==$('annotationText')&&['Delete','Backspace'].includes(e.key)&&selected){e.preventDefault();await erase(selected);return;}if(e.key==='Escape'){if(selected){selected=null;paint();return;}await finishText();invoke('recorder_annotations_edit',{editing:false});}if(e.ctrlKey&&e.key.toLowerCase()==='z'&&e.target!==$('annotationText'))$('undoAnnotation').click();};paint();
