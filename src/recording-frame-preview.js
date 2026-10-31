// Keep source padding, the 40px title bar, and even dimensions in step with
// recorder.rs (start_segment, finish, frame_pad, and mockup_svg).
export function recordingFrameGeometry(config,target,options){
 const source=target.mode==='area'&&target.rect?target.rect:
  (target.mode==='window'?options.windows.find(w=>w.id===target.window):options.monitors.find(m=>m.id===target.monitor))||options.monitors[0];
 let width=Math.ceil((source?.w||1920)/2)*2,height=Math.ceil((source?.h||1080)/2)*2;
 if(config.resolution!=='original'){
  const [maxWidth,maxHeight]=config.resolution==='720p'?[1280,720]:[1920,1080];
  const scale=Math.min(1,maxWidth/width,maxHeight/height);
  width=Math.max(2,Math.floor(width*scale/2)*2);height=Math.max(2,Math.floor(height*scale/2)*2);
 }
 const padding=Math.max(8,Math.round(width*(Number(config.frame_padding)||5)/100))&~1;
 return {width,height,padding,canvasWidth:width+2*padding,canvasHeight:height+2*padding+40};
}

// Illustrations use a compact canvas so title bars and controls stay legible in a card.
export function recordingFrameCardGeometry(config){return recordingFrameGeometry({...config,resolution:'original'},{mode:'screen',monitor:0},{monitors:[{id:0,w:320,h:200}],windows:[]});}

export function recordingBackgroundArtwork(background,w,h,id='background'){
 if(!Array.isArray(background))return `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}"><rect width="${w}" height="${h}" fill="${/^#[0-9a-f]{6}$/i.test(background)?background:'#15213a'}"/></svg>`;
 const darken=hex=>'#'+hex.slice(1).match(/../g).map(v=>Math.round(parseInt(v,16)*.84).toString(16).padStart(2,'0')).join(''),[a,b,c]=background.map(darken),x=(w-h)/4,y=-x,d=(w+h)/2,blur=Math.min(w,h)*.11;
 return `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}"><defs><linearGradient id="${id}-base" gradientUnits="userSpaceOnUse" x1="${x}" y1="${y}" x2="${x+d}" y2="${y+d}"><stop stop-color="${a}"/><stop offset=".55" stop-color="${b}"/><stop offset="1" stop-color="${c}"/></linearGradient><filter id="${id}-soft" x="-30%" y="-30%" width="160%" height="160%"><feGaussianBlur stdDeviation="${blur}"/></filter><radialGradient id="${id}-edge" cx=".5" cy=".45" r=".8"><stop offset=".3" stop-color="#080d22" stop-opacity="0"/><stop offset="1" stop-color="#080d22" stop-opacity=".24"/></radialGradient></defs><rect width="${w}" height="${h}" fill="url(#${id}-base)"/><g filter="url(#${id}-soft)"><ellipse cx="${w*.18}" cy="${h*.24}" rx="${w*.38}" ry="${h*.27}" fill="${background[0]}" opacity=".35"/><ellipse cx="${w*.82}" cy="${h*.74}" rx="${w*.43}" ry="${h*.3}" fill="${background[2]}" opacity=".3"/></g><rect width="${w}" height="${h}" fill="url(#${id}-edge)"/></svg>`;
}

export function recordingFrameArtwork(frame,geometry,background){
 const {width:w,height:h,padding:p,canvasWidth:W,canvasHeight:H}=geometry;
 const plain=frame==='none',x=plain?0:p,y=plain?0:p+40;
 // Apple mark from Simple Icons (CC0); Windows uses the four-pane mark.
 const size=Math.min(w,h)*.28,cx=x+w/2,cyContent=y+h/2;
 const mark=frame==='mac'?`<g data-card-logo="mac" transform="translate(${cx-size/2} ${cyContent-size/2}) scale(${size/24})" fill="#344054"><path d="M12.152 6.896c-.948 0-2.415-1.078-3.96-1.04-2.04.027-3.91 1.183-4.961 3.014-2.117 3.675-.546 9.103 1.519 12.09 1.013 1.454 2.208 3.09 3.792 3.039 1.52-.065 2.09-.987 3.935-.987 1.831 0 2.35.987 3.96.948 1.637-.026 2.676-1.48 3.676-2.948 1.156-1.688 1.636-3.325 1.662-3.415-.039-.013-3.182-1.221-3.22-4.857-.026-3.04 2.48-4.494 2.597-4.559-1.429-2.09-3.623-2.324-4.39-2.376-2-.156-3.675 1.09-4.61 1.09zM15.53 3.83c.843-1.012 1.4-2.427 1.245-3.83-1.207.052-2.662.805-3.532 1.818-.78.896-1.454 2.338-1.273 3.714 1.338.104 2.715-.688 3.559-1.701"/></g>`:frame==='windows'?`<g data-card-logo="windows" transform="translate(${cx-size/2} ${cyContent-size/2})" fill="#2878dd"><rect width="${size*.45}" height="${size*.45}" rx="${size*.025}"/><rect x="${size*.55}" width="${size*.45}" height="${size*.45}" rx="${size*.025}"/><rect y="${size*.55}" width="${size*.45}" height="${size*.45}" rx="${size*.025}"/><rect x="${size*.55}" y="${size*.55}" width="${size*.45}" height="${size*.45}" rx="${size*.025}"/></g>`:`<g data-card-logo="none"><circle cx="${cx}" cy="${cyContent}" r="${size*.65}" fill="#dce9ff"/><path d="M${cx-size*.2} ${cyContent-size*.32}L${cx+size*.32} ${cyContent}L${cx-size*.2} ${cyContent+size*.32}Z" fill="#3679df"/></g>`;
 const content=`<rect data-preview-content="" x="${x}" y="${y}" width="${w}" height="${h}" fill="${plain?'#edf3fc':'#f8fbff'}"/>${mark}`;
 if(plain)return `<svg viewBox="0 0 ${w} ${h}" aria-hidden="true"><rect x="1.5" y="1.5" width="${w-3}" height="${h-3}" rx="12" fill="#edf3fc" stroke="#cbd9ee" stroke-width="3"/>${mark}</svg>`;

 const right=w+p;
 const radius=frame==='mac'?10:12,title=frame==='mac'?'#e8ecf4':'#f7f9fc',control=Math.min(w/6,46),cy=p+20,minimize=right-control*2.5,maximize=right-control*1.5,close=right-control*.5;
 const chrome=frame==='mac'?`<circle cx="${p+20}" cy="${p+20}" r="6" fill="#ff5f57"/><circle cx="${p+40}" cy="${p+20}" r="6" fill="#febc2e"/><circle cx="${p+60}" cy="${p+20}" r="6" fill="#28c840"/>`:
  `${w>=160?`<g fill="#3274e9"><rect x="${p+15}" y="${p+14}" width="5" height="5" rx=".6"/><rect x="${p+22}" y="${p+14}" width="5" height="5" rx=".6"/><rect x="${p+15}" y="${p+21}" width="5" height="5" rx=".6"/><rect x="${p+22}" y="${p+21}" width="5" height="5" rx=".6"/></g>`:''}<path d="M${minimize-5} ${cy}h10 M${maximize-4.5} ${cy-4.5}h9v9h-9z M${close-4.5} ${cy-4.5}l9 9m0-9l-9 9" stroke="#344054" fill="none" stroke-width="1.35" stroke-linecap="round" stroke-linejoin="round"/><path d="M${p} ${p+39.5}H${right}" stroke="#dde3ec" stroke-width="1"/><rect x="${p+.5}" y="${p+.5}" width="${w-1}" height="${h+39}" rx="11.5" fill="none" stroke="#cbd4e1" stroke-width="1"/>`;

 const bg=recordingBackgroundArtwork(background,W,H,`frame-${frame}`);
 return `<svg viewBox="0 0 ${W} ${H}" aria-hidden="true">${bg}<defs><clipPath id="window-${frame}"><rect x="${p}" y="${p}" width="${w}" height="${h+40}" rx="${radius}"/></clipPath></defs><g clip-path="url(#window-${frame})">${content}<rect x="${p}" y="${p}" width="${w}" height="40" rx="${radius}" fill="${title}"/><rect x="${p}" y="${p+20}" width="${w}" height="20" fill="${title}"/>${chrome}</g></svg>`;
}
