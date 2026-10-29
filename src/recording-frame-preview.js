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

export function recordingFrameArtwork(frame,geometry,background){
 const {width:w,height:h,padding:p,canvasWidth:W,canvasHeight:H}=geometry;
 const plain=frame==='none',x=plain?0:p,y=plain?0:p+40;
 const content=`<rect data-preview-content x="${x}" y="${y}" width="${w}" height="${h}" fill="#fdfdff"/><g transform="translate(${x} ${y})"><rect x="${w*.1}" y="${h*.18}" width="${w*.44}" height="${h*.08}" rx="${h*.04}" fill="${frame==='windows'?'#2a6fe8':'#c7d1ea'}"/><rect x="${w*.1}" y="${h*.4}" width="${w*.7}" height="${h*.06}" rx="${h*.03}" fill="#e1e7f4"/><rect x="${w*.1}" y="${h*.6}" width="${w*.55}" height="${h*.06}" rx="${h*.03}" fill="#e1e7f4"/></g>`;
 if(plain)return `<svg viewBox="0 0 ${w} ${h}" aria-hidden="true">${content}</svg>`;
 const right=w+p;
 const chrome=frame==='mac'?`<circle cx="${p+20}" cy="${p+20}" r="6" fill="#ff5f57"/><circle cx="${p+40}" cy="${p+20}" r="6" fill="#febc2e"/><circle cx="${p+60}" cy="${p+20}" r="6" fill="#28c840"/>`:
  `<path d="M${Math.max(0,right-78)} ${p+19}h12 M${Math.max(0,right-48)} ${p+14}h10v10h-10z M${Math.max(0,right-20)} ${p+14}l10 10m0-10l-10 10" stroke="#253252" fill="none" stroke-width="1.3"/>`;
 // Background stops and the radial highlight use the same positions as background_svg.
 const bg=Array.isArray(background)?`<defs><linearGradient id="frame-${frame}" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="${background[0]}"/><stop offset=".55" stop-color="${background[1]}"/><stop offset="1" stop-color="${background[2]}"/></linearGradient><radialGradient id="light-${frame}" cx=".2" cy=".1" r=".8"><stop stop-color="#fff" stop-opacity=".28"/><stop offset="1" stop-color="#fff" stop-opacity="0"/></radialGradient></defs><rect width="${W}" height="${H}" fill="url(#frame-${frame})"/><rect width="${W}" height="${H}" fill="url(#light-${frame})"/>`:
  `<rect width="${W}" height="${H}" fill="${/^#[0-9a-f]{6}$/i.test(background)?background:'#15213a'}"/>`;
 return `<svg viewBox="0 0 ${W} ${H}" aria-hidden="true">${bg}${content}<rect x="${p}" y="${p}" width="${w}" height="40" rx="10" fill="#e8ecf4"/><rect x="${p}" y="${p+20}" width="${w}" height="20" fill="#e8ecf4"/>${chrome}</svg>`;
}
