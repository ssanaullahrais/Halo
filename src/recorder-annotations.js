import {mountColourPicker} from './recorder-colour-picker.js';
import {brushLayers,shapeStrokes} from './recorder-brush.js';
import {invoke} from '@tauri-apps/api/core';import {listen} from '@tauri-apps/api/event';import {createElement,MousePointer2,MoveUpRight,Square,Pencil,Circle,Type,Droplets,Eraser,Undo2,Trash2,Check,X} from 'lucide';import './recorder-annotations.css';
const $=id=>document.getElementById(id),canvas=$('annotationCanvas'),ctx=canvas.getContext('2d');const data=await invoke('recorder_annotation_data');canvas.width=data.width;canvas.height=data.height;let inputRegionsKey='';let annotations=data.annotations.filter(a=>a.end===null),tool='cursor',stroke=3,draft=null,start=null,editing=true,snapshot=null,hovered=null,selected=null,editorShape=null,persistence=Promise.resolve(),lastDraftUpdate=0,transform=null,lastPoint=null;
const icons={cursor:MousePointer2,arrow:MoveUpRight,rectangle:Square,pencil:Pencil,ellipse:Circle,text:Type,blur:Droplets,eraser:Eraser};for(const b of document.querySelectorAll('[data-tool]')){b.append(createElement(icons[b.dataset.tool],{'aria-hidden':'true'}),document.createTextNode({cursor:'Cursor',arrow:'Arrow',rectangle:'Rectangle',pencil:'Pencil',ellipse:'Circle',text:'Text',blur:'Blur',eraser:'Eraser'}[b.dataset.tool]));b.onclick=async()=>{try{if(b.dataset.tool==='cursor')await clearAll();else finishText();selected=null;paint();selectTool(b.dataset.tool);if(tool==='blur')loadBlurPreview();}catch(e){error(e);}};}document.querySelector('[data-tool=cursor]').classList.add('active');canvas.style.cursor='default';for(const [id,node]of [['undoAnnotation',Undo2],['clearAnnotations',Trash2],['applyAnnotations',Check],['removeAnnotation',Trash2]])$(id).append(createElement(node,{'aria-hidden':'true'}));
function error(value){if(value)console.error(value);$('annotationError').hidden=!value;$('annotationError').textContent=value?'Could not apply this change. Please try again.':'';}
let colourPicker;
let viewport=data.viewport||{x:0,y:0,w:canvas.width,h:canvas.height};
function fitCanvas(){canvas.style.width=`${innerWidth*canvas.width/viewport.w}px`;canvas.style.height=`${innerHeight*canvas.height/viewport.h}px`;canvas.style.left=`${-viewport.x*innerWidth/viewport.w}px`;canvas.style.top=`${-viewport.y*innerHeight/viewport.h}px`;}
const point=e=>{const r=canvas.getBoundingClientRect();return {x:Math.max(0,Math.min(canvas.width,(e.clientX-r.left)/r.width*canvas.width)),y:Math.max(0,Math.min(canvas.height,(e.clientY-r.top)/r.height*canvas.height))};};
fitCanvas();await listen('halo://annotation-viewport',e=>{viewport=e.payload;fitCanvas();paint();});
function bounds(a){if(a.kind==='pencil'&&a.points?.length){const pts=arrowPolyline(a),xs=pts.map(p=>p.x),ys=pts.map(p=>p.y);return {x:Math.min(...xs),y:Math.min(...ys),w:Math.max(...xs)-Math.min(...xs),h:Math.max(...ys)-Math.min(...ys)};}return {x:Math.min(a.x,a.x+a.w),y:Math.min(a.y,a.y+a.h),w:Math.abs(a.w),h:Math.abs(a.h)};}
// An arrow's bend points (a.points) sit between its start and end; sampling and drawing both treat
// [start, ...points, end] as one polyline, then smooth it the standard "curve through points" way:
// each interior point becomes a quadratic control, the curve actually threading through the
// midpoint of each consecutive pair, except the final segment which runs exactly to the real end.
function arrowPolyline(a){return [{x:a.x,y:a.y},...(a.points||[]),{x:a.x+a.w,y:a.y+a.h}];}
function sampleQuadratic(p0,p1,p2,steps,out){for(let i=1;i<=steps;i++){const t=i/steps,mt=1-t;out.push({x:mt*mt*p0.x+2*mt*t*p1.x+t*t*p2.x,y:mt*mt*p0.y+2*mt*t*p1.y+t*t*p2.y});}}
function sampleArrow(a){const pts=arrowPolyline(a);const out=[pts[0]];if(pts.length===2){out.push(pts[1]);return out;}for(let i=1;i<pts.length-2;i++){const mid={x:(pts[i].x+pts[i+1].x)/2,y:(pts[i].y+pts[i+1].y)/2};sampleQuadratic(out[out.length-1],pts[i],mid,8,out);}sampleQuadratic(out[out.length-1],pts[pts.length-2],pts[pts.length-1],8,out);return out;}
function pointToSegmentDistance(p,q1,q2){const len2=(q2.x-q1.x)**2+(q2.y-q1.y)**2;const t=len2?Math.max(0,Math.min(1,((p.x-q1.x)*(q2.x-q1.x)+(p.y-q1.y)*(q2.y-q1.y))/len2)):0;return Math.hypot(p.x-q1.x-t*(q2.x-q1.x),p.y-q1.y-t*(q2.y-q1.y));}
// Finds where along the (straight) control polygon a new bend point should be inserted for a click
// at p, so "click anywhere on the arrow" lands the new dot exactly between the right neighbours.
function insertArrowPoint(a,p){const poly=arrowPolyline(a);let bestSeg=0,bestDist=Infinity;for(let i=0;i<poly.length-1;i++){const d=pointToSegmentDistance(p,poly[i],poly[i+1]);if(d<bestDist){bestDist=d;bestSeg=i;}}const points=[...(a.points||[])];points.splice(bestSeg,0,{x:p.x,y:p.y});return {index:bestSeg,points};}
function handles(a){if(a.kind==='text'||a.kind==='pencil')return [];if(a.kind==='arrow')return [{name:'start',x:a.x,y:a.y},...(a.points||[]).map((p,i)=>({name:`pt:${i}`,x:p.x,y:p.y})),{name:'end',x:a.x+a.w,y:a.y+a.h}];const b=bounds(a),l=b.x-8,r=b.x+b.w+8,t=b.y-8,bottom=b.y+b.h+8,cx=(l+r)/2,cy=(t+bottom)/2;return [{name:'nw',x:l,y:t},{name:'n',x:cx,y:t},{name:'ne',x:r,y:t},{name:'e',x:r,y:cy},{name:'se',x:r,y:bottom},{name:'s',x:cx,y:bottom},{name:'sw',x:l,y:bottom},{name:'w',x:l,y:cy}].filter(h=>a.kind!=='ellipse'||h.name.length===2);}
function handleAt(p){return selected&&handles(selected).find(h=>Math.hypot((p.x-h.x)*canvas.getBoundingClientRect().width/canvas.width,(p.y-h.y)*canvas.getBoundingClientRect().height/canvas.height)<=9);}
function clonePoints(a){return (a.points||[]).map(p=>({...p}));}
function beginTransform(a,mode,p,e,isNew=false){selected=a;transform={mode,original:{...a,points:clonePoints(a)},point:p,isNew};start=p;draft={...a,keyframes:[],points:clonePoints(a)};canvas.setPointerCapture(e.pointerId);paint();}
function transformTo(p){const a=transform.original,b=bounds(a),dx=p.x-transform.point.x,dy=p.y-transform.point.y,mode=transform.mode;
 if(mode==='move'){const nx=a.x+Math.max(-b.x,Math.min(canvas.width-b.x-b.w,dx)),ny=a.y+Math.max(-b.y,Math.min(canvas.height-b.y-b.h,dy));if(a.points?.length)draft.points=a.points.map(q=>({x:q.x+(nx-a.x),y:q.y+(ny-a.y)}));draft.x=nx;draft.y=ny;}
 else if(mode.startsWith('pt:')){const idx=Number(mode.slice(3));const points=[...draft.points];points[idx]={x:Math.max(0,Math.min(canvas.width,p.x)),y:Math.max(0,Math.min(canvas.height,p.y))};draft.points=points;}
 else if(a.kind==='ellipse'){
  const ax=mode.includes('w')?b.x+b.w:b.x,ay=mode.includes('n')?b.y+b.h:b.y;
  const sx=mode.includes('w')?-1:1,sy=mode.includes('n')?-1:1;
  const side=Math.max(1,Math.min(Math.max(Math.abs(p.x-ax),Math.abs(p.y-ay)),sx<0?ax:canvas.width-ax,sy<0?ay:canvas.height-ay));
  draft.x=sx<0?ax-side:ax;draft.y=sy<0?ay-side:ay;draft.w=side;draft.h=side;
 }
 else if(a.kind==='arrow'){if(mode==='start'){draft.x=p.x;draft.y=p.y;draft.w=a.x+a.w-p.x;draft.h=a.y+a.h-p.y;}else{draft.w=p.x-a.x;draft.h=p.y-a.y;}}
 else{let l=b.x,r=b.x+b.w,t=b.y,bottom=b.y+b.h;if(mode.includes('w'))l=Math.max(0,Math.min(r-8,b.x+dx));if(mode.includes('e'))r=Math.min(canvas.width,Math.max(l+8,b.x+b.w+dx));if(mode.includes('n'))t=Math.max(0,Math.min(bottom-8,b.y+dy));if(mode.includes('s'))bottom=Math.min(canvas.height,Math.max(t+8,b.y+b.h+dy));draft.x=l;draft.y=t;draft.w=r-l;draft.h=bottom-t;}
 queueDraft();paint();
}
function hit(p){return [...annotations].reverse().find(a=>{if(a===editorShape)return false;if(a.kind==='arrow'||a.kind==='pencil'){const pts=sampleArrow(a);let min=Infinity;for(let i=0;i<pts.length-1;i++)min=Math.min(min,pointToSegmentDistance(p,pts[i],pts[i+1]));return min<Math.max(11,a.stroke+7);}const b=bounds(a);if(p.x<b.x-8||p.y<b.y-8||p.x>b.x+b.w+8||p.y>b.y+b.h+8)return false;if(a.kind==='rectangle'){const edge=Math.max(9,a.stroke+5);return Math.min(Math.abs(p.x-b.x),Math.abs(p.x-b.x-b.w),Math.abs(p.y-b.y),Math.abs(p.y-b.y-b.h))<=edge;}return true;});}
function draw(a){
 ctx.save();ctx.fillStyle=a.color;ctx.strokeStyle=a.color;ctx.lineCap='round';ctx.lineJoin='round';
 if(['pencil','arrow','rectangle','ellipse'].includes(a.kind)){
  for(const stroke of shapeStrokes(a,sampleArrow(a)))for(const layer of brushLayers(stroke.samples,stroke.stroke)){ctx.globalAlpha=layer.opacity;ctx.filter=layer.blur?`blur(${layer.blur}px)`:'none';ctx.fill(new Path2D(layer.path));}
  ctx.globalAlpha=1;ctx.filter='none';
 }
 if(a.kind==='text'){ctx.font=`400 ${a.font_size}px "Segoe Print",Kalam,"Comic Sans MS",cursive`;a.text.split('\n').forEach((line,i)=>{const y=a.y+a.font_size*(1+i*1.5);ctx.globalAlpha=.92;ctx.fillText(line,a.x,y);});}
 if(a.kind==='blur'){const b=bounds(a);ctx.shadowColor='transparent';ctx.beginPath();ctx.rect(b.x,b.y,b.w,b.h);ctx.clip();if(snapshot){ctx.filter='blur(9px)';ctx.drawImage(snapshot,0,0,canvas.width,canvas.height);}else{ctx.fillStyle='#b2c4dfcc';ctx.fillRect(b.x,b.y,b.w,b.h);}ctx.filter='none';}
 ctx.restore();
}
let paintFrame=0;function paint(){if(!paintFrame)paintFrame=requestAnimationFrame(()=>{paintFrame=0;paintNow();});}
function paintNow(){
 ctx.clearRect(0,0,canvas.width,canvas.height);annotations.filter(a=>a!==editorShape&&a.id!==draft?.id).forEach(draw);if(draft)draw(draft);
 const remove=$('removeAnnotation');remove.hidden=!editing||!selected||!!draft||!!editorShape;
 if(selected&&editing&&!editorShape){const selection=transform?draft:selected,b=bounds(selection),pad=8,sx=canvas.getBoundingClientRect().width/canvas.width,sy=canvas.getBoundingClientRect().height/canvas.height;
  ctx.save();ctx.strokeStyle='#2379ff';ctx.lineWidth=1/sx;
  if(selection.kind==='arrow'){
   // The dashed control polygon shows exactly where each bend dot sits relative to the straight
   // line, so "drag a dot off this line to curve it, click it again to remove it" is legible.
   const poly=arrowPolyline(selection);
   ctx.save();ctx.setLineDash([5/sx,4/sx]);ctx.strokeStyle='#2379ff80';ctx.beginPath();poly.forEach((pt,i)=>i===0?ctx.moveTo(pt.x,pt.y):ctx.lineTo(pt.x,pt.y));ctx.stroke();ctx.setLineDash([]);ctx.restore();
  }else ctx.strokeRect(b.x-pad,b.y-pad,b.w+pad*2,b.h+pad*2);
  for(const h of handles(selection)){
   if(h.name.startsWith('pt:')){
    ctx.beginPath();ctx.arc(h.x,h.y,7/sx,0,Math.PI*2);ctx.fillStyle='#2379ff';ctx.fill();ctx.lineWidth=2/sx;ctx.strokeStyle='#ffffff';ctx.stroke();
    continue;
   }
   ctx.fillStyle='white';ctx.fillRect(h.x-3/sx,h.y-3/sy,6/sx,6/sy);ctx.strokeRect(h.x-3/sx,h.y-3/sy,6/sx,6/sy);
  }
  ctx.restore();
  remove.style.left=`${Math.max(8,Math.min(innerWidth-38,canvas.getBoundingClientRect().left+(b.x+b.w/2)*sx-14))}px`;
  remove.style.top=`${Math.max(8,canvas.getBoundingClientRect().top+b.y*sy-44)}px`;
 }
 syncInputRegions();
}
$('removeAnnotation').ariaLabel='Delete selected drawing';

function queueDraft(force=false){
 if(!draft||(!force&&performance.now()-lastDraftUpdate<1000/(data.fps||60)))return;
 if(draft.kind==='blur'&&(Math.abs(draft.w)<4||Math.abs(draft.h)<4))return;
 if(draft.kind!=='pencil'&&Math.abs(draft.w)<2&&Math.abs(draft.h)<2)return;
 lastDraftUpdate=performance.now();const shape=draft,snapshot={...shape,keyframes:[],points:clonePoints(shape)};
 persistence=persistence.then(async()=>{if(!shape.id){const saved=await invoke('recorder_annotation_add',{annotation:snapshot});shape.id=saved.id;shape.start=saved.start;}else{snapshot.id=shape.id;await invoke('recorder_annotation_update',{annotation:snapshot});}}).catch(error);
}
async function commit(annotation){queueDraft(true);const saved=persistence;draft=null;transform=null;await saved;if(annotation.id){const index=annotations.findIndex(a=>a.id===annotation.id);if(index<0)annotations.push(annotation);else annotations[index]=annotation;if(!draft)selected=annotation;}paint();}
async function loadBlurPreview(){try{const image=new Image();image.src=await invoke('recorder_annotation_snapshot');await image.decode();snapshot=image;paint();}catch(e){error(e);}}

function queueText(){if(!editorShape)return;const shape=editorShape;shape.text=$('annotationText').value;ctx.font=`400 ${shape.font_size}px "Segoe Print",Kalam,"Comic Sans MS",cursive`;const lines=shape.text.split('\n');shape.w=Math.min(canvas.width-shape.x,Math.max(110,...lines.map(l=>ctx.measureText(l).width+12)));shape.h=Math.min(canvas.height-shape.y,Math.max(32,lines.length*shape.font_size*1.5+6));const sx=canvas.getBoundingClientRect().width/canvas.width,sy=canvas.getBoundingClientRect().height/canvas.height;$('annotationText').style.width=`${Math.max(80,shape.w*sx)}px`;$('annotationText').style.height=`${Math.max(34,shape.h*sy)}px`;const snapshot={...shape,keyframes:[],points:clonePoints(shape)};persistence=persistence.then(async()=>{if(!snapshot.text.trim()){if(shape.id){await invoke('recorder_annotation_end',{id:shape.id});annotations=annotations.filter(a=>a!==shape);shape.id=0;}return;}if(!shape.id){const saved=await invoke('recorder_annotation_add',{annotation:snapshot});shape.id=saved.id;shape.start=saved.start;annotations.push(shape);}else{snapshot.id=shape.id;await invoke('recorder_annotation_update',{annotation:snapshot});}}).catch(error).then(()=>paint());}
function selectTool(name){tool=name;canvas.style.cursor=editing?(name==='cursor'?'default':name==='eraser'?'cell':'crosshair'):'default';document.querySelectorAll('[data-tool]').forEach(x=>x.classList.toggle('active',x.dataset.tool===name));syncInputRegions();}
function finishText(){if(!editorShape)return persistence;queueText();editorShape=null;$('annotationText').hidden=true;
 // One text box per click: finishing it drops back to Cursor instead of starting another box
 // the moment you click elsewhere, which is what plain "text tool is still selected" used to do.
 if(tool==='text')selectTool('cursor');
 paint();return persistence;}
async function erase(annotation){if(!annotation)return;await finishText();annotations=annotations.filter(a=>a.id!==annotation.id);if(hovered?.id===annotation.id)hovered=null;if(selected?.id===annotation.id)selected=null;paint();try{await invoke('recorder_annotation_end',{id:annotation.id});}catch(e){error(e);}}
function recolor(a,color){a.color=color;const snapshot={...a,keyframes:[]};persistence=persistence.then(()=>invoke('recorder_annotation_update',{annotation:snapshot})).catch(error);paint();}
canvas.onpointerdown=e=>{if(!editing||e.button!==0)return;e.preventDefault();finishText();const p=point(e);if(tool==='eraser'){selected=null;start=p;erase(hit(p));canvas.setPointerCapture(e.pointerId);return;}const handle=handleAt(p);if(handle){beginTransform(selected,handle.name,p,e);return;}const existing=hit(p);if(existing){
 beginTransform(existing,'move',p,e);
 // Dragging its body moves the whole arrow (matching the move cursor you see on hover); only a
 // plain click with no real drag instead adds a bend point at this spot — decided on pointerup.
 if(existing.kind==='arrow')transform.clickAddsPointAt=p;
 return;
}selected=null;if(tool==='cursor')return paint();start=p;hovered=null;const base={id:0,kind:tool,x:p.x,y:p.y,w:0,h:0,text:'',color:$('annotationColour').value,stroke,font_size:24,start:0,end:null};if(tool==='text'){editorShape=base;const input=$('annotationText');input.hidden=false;input.style.left=`${Math.max(4,e.clientX-6)}px`;input.style.top=`${Math.max(4,e.clientY-4)}px`;input.placeholder='Type, then press Enter';input.style.fontSize=`${24*canvas.getBoundingClientRect().width/canvas.width}px`;input.style.color=base.color;input.value='';input.style.width='190px';input.style.height='34px';input.focus();start=null;paint();return;}draft={...base,points:[]};canvas.setPointerCapture(e.pointerId);};
canvas.onpointermove=e=>{const p=point(e);lastPoint=p;if(transform){transformTo(p);return;}if(tool==='eraser'){hovered=hit(p);if(start)erase(hovered);paint();return;}if(!start||!draft){const handle=handleAt(p);hovered=hit(p);canvas.style.cursor=handle?({n:'ns-resize',s:'ns-resize',e:'ew-resize',w:'ew-resize',nw:'nwse-resize',se:'nwse-resize',ne:'nesw-resize',sw:'nesw-resize'}[handle.name]||'crosshair'):hovered?'move':tool==='cursor'?'default':'crosshair';return;}if(draft.kind==='pencil'){const previous={x:draft.x+draft.w,y:draft.y+draft.h};if(Math.hypot(p.x-previous.x,p.y-previous.y)<2)return;if(draft.points.length>=4096)draft.points=draft.points.filter((_,i)=>i%2===0);draft.points.push(previous);}draft.w=p.x-start.x;draft.h=p.y-start.y;if(draft.kind==='ellipse'){const sx=draft.w<0?-1:1,sy=draft.h<0?-1:1,side=Math.min(Math.max(Math.abs(draft.w),Math.abs(draft.h)),sx<0?start.x:canvas.width-start.x,sy<0?start.y:canvas.height-start.y);draft.w=sx*side;draft.h=sy*side;}queueDraft();paint();};
canvas.addEventListener('wheel',e=>{e.preventDefault();if(!editing||tool!=='cursor')return;const notches=Math.max(-3,Math.min(3,-e.deltaY/100));invoke('recorder_annotation_forward_wheel',{delta:Math.round(notches*120)}).catch(()=>{});},{passive:false});
canvas.onpointerup=async()=>{start=null;if(!draft)return;if(transform?.mode==='move'&&transform.clickAddsPointAt){
 const moved=Math.hypot((lastPoint?.x??transform.point.x)-transform.point.x,(lastPoint?.y??transform.point.y)-transform.point.y);
 if(moved<4){const {points}=insertArrowPoint(transform.original,transform.clickAddsPointAt);draft={...transform.original,keyframes:[],points};}
 await commit(draft);return;
}if(transform?.mode?.startsWith('pt:')){
 // A real drag moves (or places) the dot; a plain click on an *existing* dot removes it instead.
 const moved=Math.hypot((lastPoint?.x??transform.point.x)-transform.point.x,(lastPoint?.y??transform.point.y)-transform.point.y);
 if(moved<4&&!transform.isNew){const idx=Number(transform.mode.slice(3));draft.points=draft.points.filter((_,i)=>i!==idx);}
 await commit(draft);return;
}if((draft.kind==='blur'&&(Math.abs(draft.w)<4||Math.abs(draft.h)<4))||(draft.kind!=='pencil'&&Math.abs(draft.w)<2&&Math.abs(draft.h)<2)){await persistence;if(draft.id)await invoke('recorder_annotation_end',{id:draft.id});draft=null;transform=null;paint();return;}if(draft.kind==='blur'&&!snapshot)loadBlurPreview();await commit(draft);};$('annotationText').oninput=queueText;$('annotationText').onblur=finishText;$('annotationText').onkeydown=e=>{if((e.key==='Enter'&&!e.shiftKey)||(e.ctrlKey&&e.key==='Enter')||e.key==='Escape'){e.preventDefault();e.stopPropagation();finishText();}};$('removeAnnotation').onpointerdown=e=>{e.preventDefault();e.stopPropagation();};$('removeAnnotation').onclick=()=>erase(selected);
async function clearAll(){await finishText();await invoke('recorder_annotation_end',{id:null});annotations=[];draft=null;start=null;transform=null;hovered=null;selected=null;paint();}
$('annotationStroke').onclick=e=>{stroke=stroke===3?6:stroke===6?2:3;e.currentTarget.textContent=`${stroke} px`;};$('annotationColour').oninput=$('annotationColour').onchange=e=>{if(selected&&selected.kind!=='blur')recolor(selected,e.currentTarget.value);};$('undoAnnotation').onclick=async()=>{await finishText();const a=annotations.at(-1);if(a)await erase(a);};$('clearAnnotations').onclick=clearAll;$('applyAnnotations').onclick=async()=>{await finishText();invoke('recorder_annotations_edit',{editing:false});};
await listen('halo://annotation-editing',async e=>{
 editing=e.payload;
 if(!editing){
  colourPicker?.close();
  // Closing the tool clears the slate: whatever was drawn this session disappears from the live
  // canvas and stops appearing in the recording from this moment on (it isn't deleted from the
  // timeline — the parts already recorded keep showing exactly what was on screen at the time).
  await clearAll();
 } else {
  selectTool('cursor');
 }
 $('annotationTools').hidden=!editing;$('removeAnnotation').hidden=true;canvas.style.cursor=editing?(tool==='cursor'?'default':'crosshair'):'default';
});await listen('halo://annotation-flush',async e=>{await finishText();if(draft)await commit(draft);await persistence;await invoke('recorder_annotations_flushed',{token:e.payload});});window.onkeydown=async e=>{if(e.target!==$('annotationText')&&['Delete','Backspace'].includes(e.key)&&selected){e.preventDefault();await erase(selected);return;}if(e.key==='Escape'){if(selected){selected=null;paint();return;}await finishText();invoke('recorder_annotations_edit',{editing:false});}if(e.ctrlKey&&e.key.toLowerCase()==='z'&&e.target!==$('annotationText'))$('undoAnnotation').click();};paint();

// Cursor mode passes empty desktop space through to the app underneath. Report
// only actual drawing controls and annotation hit areas, in window CSS pixels.
function syncInputRegions(){
 const regions=[];
 for(const id of ['annotationTools','annotationText','removeAnnotation','annotationColourPicker']){
  const el=$(id);if(!el.hidden){const r=el.getBoundingClientRect();regions.push({x:r.x,y:r.y,w:r.width,h:r.height});}
 }
 if(editing&&tool==='cursor'){
  const sx=canvas.getBoundingClientRect().width/canvas.width,sy=canvas.getBoundingClientRect().height/canvas.height;
  for(const a of annotations){
   const b=bounds(a);const points=['arrow','pencil'].includes(a.kind)?arrowPolyline(a):[];
   // Arrow hit areas follow the shaft instead of blocking its entire rectangle.
   if(points.length){for(let i=1;i<points.length;i++){
    const from=points[i-1],to=points[i],steps=Math.max(1,Math.ceil(Math.hypot((to.x-from.x)*sx,(to.y-from.y)*sy)/10));
    for(let j=0;j<=steps;j++){const t=j/steps;regions.push({x:canvas.getBoundingClientRect().left+(from.x+(to.x-from.x)*t)*sx-12,y:canvas.getBoundingClientRect().top+(from.y+(to.y-from.y)*t)*sy-12,w:24,h:24});}
   }}else regions.push({x:canvas.getBoundingClientRect().left+b.x*sx-16,y:canvas.getBoundingClientRect().top+b.y*sy-16,w:b.w*sx+32,h:b.h*sy+32});
  }
 }
 const request={cursor:!editing||tool==='cursor',regions};const key=JSON.stringify(request);
 if(key===inputRegionsKey)return;inputRegionsKey=key;
 invoke('recorder_annotation_input',request).catch(e=>{inputRegionsKey='';error(e);});
}
colourPicker=mountColourPicker($('annotationColour'),$('annotationColourToggle'),$('annotationColourPicker'),syncInputRegions);
new ResizeObserver(syncInputRegions).observe($('annotationTools'));
window.addEventListener('resize',()=>{fitCanvas();syncInputRegions();});
syncInputRegions();

// Match the countdown: the native window is hidden until real CSS/font metrics
// and the first canvas/toolbar frame are ready, rather than flashing a fallback.
$('annotationTools').getBoundingClientRect();
await document.fonts.load('24px Kalam');
await document.fonts.ready;
await new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)));
paintNow();
await invoke('recorder_annotation_ready');
