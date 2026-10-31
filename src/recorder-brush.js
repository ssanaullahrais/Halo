// A tapered brush silhouette, shared in shape with recorder_annotations.rs.
export function brushPath(samples,stroke,lane=0,spread=1){
 if(!samples.length)return '';
 if(samples.length===2){const [a,b]=samples;samples=Array.from({length:17},(_,i)=>({x:a.x+(b.x-a.x)*i/16,y:a.y+(b.y-a.y)*i/16}));}
 const reduced=[];for(let i=0;i<samples.length;i++){const p=samples[i],last=reduced.at(-1);if(!last||i===samples.length-1||Math.hypot(p.x-last.x,p.y-last.y)>=1.5)reduced.push(p);}samples=reduced;
 const distances=[0];for(let i=1;i<samples.length;i++)distances.push(distances[i-1]+Math.hypot(samples[i].x-samples[i-1].x,samples[i].y-samples[i-1].y));
 const total=distances.at(-1)||1,left=[],right=[];
 for(let i=0;i<samples.length;i++){
  const p=samples[i],before=samples[Math.max(0,i-1)],after=samples[Math.min(samples.length-1,i+1)],dx=after.x-before.x,dy=after.y-before.y,length=Math.hypot(dx,dy)||1,t=distances[i]/total;
  const radius=stroke*1.6*(.025+.975*Math.pow(Math.max(0,Math.sin(Math.PI*t)),.65))*(1+.1*Math.sin(t*61)+.04*Math.sin(t*127));
  const center=lane+(lane?.035*Math.sin(t*91+lane*7):0),width=spread<.2?spread*(.3+.7*Math.max(0,Math.sin(t*43+lane*5))):spread;
  left.push({x:p.x-dy/length*radius*(center+width),y:p.y+dx/length*radius*(center+width)});right.push({x:p.x-dy/length*radius*(center-width),y:p.y+dx/length*radius*(center-width)});
 }
 return [...left,...right.reverse()].map((p,i)=>`${i?'L':'M'}${p.x.toFixed(3)} ${p.y.toFixed(3)}`).join('')+'Z';
}

export function brushLayers(samples,stroke){return [
 {path:brushPath(samples,stroke),opacity:.45,blur:Math.max(.5,stroke*.2)},
 {path:brushPath(samples,stroke,0,.72),opacity:.65,blur:Math.max(.18,stroke*.06)},
 ...[-.84,-.56,-.27,.13,.43,.76].map(lane=>({path:brushPath(samples,stroke,lane,.065),opacity:.28,blur:0}))
];}

export function shapeStrokes(a,curve){
 if(a.kind==='pencil')return [{samples:curve,stroke:a.stroke}];
 if(a.kind==='arrow'){
  const ex=a.x+a.w,ey=a.y+a.h,last=a.points?.at(-1),angle=last?Math.atan2(ey-last.y,ex-last.x):Math.atan2(a.h,a.w),weight=Math.min(a.stroke*2.3,a.stroke+Math.hypot(a.w,a.h)/100),length=Math.min(52,10+weight*1.6+Math.hypot(a.w,a.h)*.09,Math.hypot(a.w,a.h)*.6);
  return [{samples:curve,stroke:a.stroke},...[-.5,.5].map(offset=>({samples:[{x:ex-length*Math.cos(angle+offset),y:ey-length*Math.sin(angle+offset)},{x:ex,y:ey}],stroke:a.stroke*.8}))];
 }
 const x=Math.min(a.x,a.x+a.w),y=Math.min(a.y,a.y+a.h),w=Math.abs(a.w),h=Math.abs(a.h),samples=[];
 if(a.kind==='ellipse'){const n=Math.max(32,Math.min(512,Math.ceil(Math.PI*(w+h)/6)));for(let i=0;i<=n;i++){const t=i/n*Math.PI*2-Math.PI/2;samples.push({x:x+w/2+Math.cos(t)*w/2,y:y+h/2+Math.sin(t)*h/2});}}
 else if(a.kind==='rectangle'){
  const radius=Math.min(8,w/2,h/2);samples.push({x:x+radius,y});
  const line=(tx,ty)=>{const from=samples.at(-1),n=Math.max(1,Math.min(256,Math.ceil(Math.hypot(tx-from.x,ty-from.y)/4)));for(let i=1;i<=n;i++)samples.push({x:from.x+(tx-from.x)*i/n,y:from.y+(ty-from.y)*i/n});};
  const arc=(cx,cy,start)=>{for(let i=1;i<=8;i++){const t=start+i/8*Math.PI/2;samples.push({x:cx+Math.cos(t)*radius,y:cy+Math.sin(t)*radius});}};
  line(x+w-radius,y);arc(x+w-radius,y+radius,-Math.PI/2);line(x+w,y+h-radius);arc(x+w-radius,y+h-radius,0);line(x+radius,y+h);arc(x+radius,y+h-radius,Math.PI/2);line(x,y+radius);arc(x+radius,y+radius,Math.PI);
 }
 return [{samples,stroke:a.stroke}];
}
