//! Interaction-aware pointer glides and a directionally blurred cursor atlas.
use super::*;
#[path="recorder_cursor_assets.rs"]mod artwork;

pub(super) const PAD:u32=32;
const LENGTHS:[f64;6]=[0.,2.,5.,10.,18.,32.];
pub(super) fn processed(c:&Config)->bool{c.smart_zoom||c.smooth_cursor||c.cinematic_cursor}
pub(super) fn cinematic(c:&Config)->bool{c.smart_zoom||c.cinematic_cursor}
pub(super) fn enabled(c:&Config)->bool{(c.kenney_cursor||processed(c)||c.frame=="mac")&&c.cursor!="hidden"}
pub(super) fn cell(c:&Config)->u32{c.cursor_size+PAD*2}
fn ease(t:f64)->f64{let t=t.clamp(0.,1.);t*t*t*(10.+t*(-15.+6.*t))}

pub(super) fn sprite(vx:f64,vy:f64,scale:f64,fps:u32,shape:u8)->(u32,u32){
 if shape>=3{return (0,54+(shape.min(12)-3)as u32);}
 let length=(vx.hypot(vy)*0.5/fps as f64).min(32.);
 let level=(0..LENGTHS.len()).min_by(|&a,&b|(LENGTHS[a]-length).abs().total_cmp(&(LENGTHS[b]-length).abs())).unwrap();
 let direction=if level==0{0}else{((vy.atan2(vx).rem_euclid(std::f64::consts::TAU)/std::f64::consts::TAU*16.).round()as u32)%16};
 let press=if scale<0.91{0}else if scale<0.97{1}else{2};
 (direction,shape.min(2)as u32*18+press*6+level as u32)
}

pub(super) fn native_shape()->u8{
 static HANDLES:std::sync::OnceLock<Vec<(usize,u8)>>=std::sync::OnceLock::new();
 let handles=HANDLES.get_or_init(||unsafe{[(IDC_HAND,1),(IDC_IBEAM,2),(IDC_SIZEWE,3),(IDC_SIZENS,4),(IDC_SIZENWSE,5),(IDC_SIZENESW,6),(IDC_SIZEALL,7),(IDC_WAIT,8),(IDC_APPSTARTING,9),(IDC_NO,10),(IDC_CROSS,11),(IDC_HELP,12)].into_iter().filter_map(|(id,shape)|LoadCursorW(None,id).ok().map(|h|(h.0 as usize,shape))).collect()});
 let mut info=CURSORINFO{cbSize:std::mem::size_of::<CURSORINFO>()as u32,..Default::default()};if unsafe{GetCursorInfo(&mut info)}.is_ok(){if let Some((_,shape))=handles.iter().find(|(h,_)|*h==info.hCursor.0 as usize){return *shape;}}0
}
static WHEEL:std::sync::atomic::AtomicI32=std::sync::atomic::AtomicI32::new(0);
unsafe extern "system" fn mouse_hook(code:i32,w:windows::Win32::Foundation::WPARAM,l:windows::Win32::Foundation::LPARAM)->windows::Win32::Foundation::LRESULT{
 if code>=0{let key=match w.0 as u32{WM_LBUTTONDOWN=>Some(1),WM_RBUTTONDOWN=>Some(2),WM_MBUTTONDOWN=>Some(4),WM_XBUTTONDOWN=>{let e=unsafe{&*(l.0 as *const MSLLHOOKSTRUCT)};Some(if e.mouseData>>16==1{5}else{6})},_=>None};if let Some(key)=key{keystrokes::hook(key,0,0);}}
 if code>=0&&w.0 as u32==WM_MOUSEWHEEL{let event=unsafe{&*(l.0 as *const MSLLHOOKSTRUCT)};WHEEL.fetch_add((event.mouseData>>16)as u16 as i16 as i32,Ordering::Relaxed);}
 unsafe{CallNextHookEx(None,code,w,l)}
}
// Low-level mouse hooks synchronously participate in desktop input dispatch.
// Keep their message pump off the sampler, which performs file I/O and Win32 queries.
pub(super) struct WheelCapture{stop:Arc<AtomicBool>,thread:Option<thread::JoinHandle<()>>}
impl WheelCapture{
 pub(super) fn new(track_keys:bool)->Self{
  WHEEL.store(0,Ordering::Relaxed);let stop=Arc::new(AtomicBool::new(false));let done=stop.clone();
  let thread=thread::spawn(move||{
   let module=unsafe{windows::Win32::System::LibraryLoader::GetModuleHandleW(None)}.ok();
   let hook=match unsafe{SetWindowsHookExW(WH_MOUSE_LL,Some(mouse_hook),module.map(Into::into),0)}{Ok(h)=>h,Err(e)=>{log_error(&format!("Scroll focus tracking is unavailable: {e}"));return;}};
   let keyboard=if track_keys{unsafe{SetWindowsHookExW(WH_KEYBOARD_LL,Some(context::keyboard_hook),module.map(Into::into),0)}.map_err(|e|log_error(&format!("Keyboard prompt tracking unavailable: {e}"))).ok()}else{None};
   let mut message=MSG::default();while !done.load(Ordering::Relaxed){unsafe{while PeekMessageW(&mut message,None,0,0,PM_REMOVE).as_bool(){let _=TranslateMessage(&message);DispatchMessageW(&message);}}thread::sleep(Duration::from_millis(1));}
   let _=unsafe{UnhookWindowsHookEx(hook)};if let Some(h)=keyboard{let _=unsafe{UnhookWindowsHookEx(h)};}
  });Self{stop,thread:Some(thread)}
 }
 pub(super) fn take(&self)->i32{WHEEL.swap(0,Ordering::Relaxed)}
}
impl Drop for WheelCapture{fn drop(&mut self){self.stop.store(true,Ordering::Relaxed);if let Some(thread)=self.thread.take(){let _=thread.join();}WHEEL.store(0,Ordering::Relaxed);}}
pub(super) fn hotspot(c:&Config,shape:u8)->(f64,f64){let size=c.cursor_size as f64;if c.kenney_cursor{if c.cursor=="highlight"{return(size/2.,size/2.);}return match shape{0=>(0.,0.),1=>(size*10./28.,0.),9|12=>(size*2./32.,size/32.),_=>(size/2.,size/2.)};}if c.cursor=="highlight"{(size/2.,size/2.)}else if c.cursor=="normal"&&shape==1{(size*29./64.,size*5./64.)}else if c.cursor=="normal"&&shape==2{(size/2.,size/2.)}else{(size*5./64.,size*3./64.)}}

pub(super) fn pack_body(shape:u8)->String{if shape==2{return r##"<path d="M21 8H43M32 8V56M21 56H43" fill="none" stroke="white" stroke-width="7"/><path d="M21 8H43M32 8V56M21 56H43" fill="none" stroke="#15171c" stroke-width="3"/>"##.into();}let svg=artwork::svg(shape);let content=svg.split_once('>').unwrap().1.strip_suffix("</svg>\n").or_else(||svg.split_once('>').unwrap().1.trim().strip_suffix("</svg>")).unwrap();let view=if shape==0{"8 6 20 20"}else if shape==1{"2 2 28 28"}else{"0 0 32 32"};format!(r#"<svg width="64" height="64" viewBox="{view}">{content}</svg>"#)}
pub(super) fn pack_style(c:&Config,shape:u8)->String{let body=pack_body(shape);if c.cursor!="highlight"{return body;}let mut plain=c.clone();plain.cursor="normal".into();let(hx,hy)=hotspot(&plain,shape);let factor=64./c.cursor_size as f64;format!(r##"<circle cx="32" cy="32" r="29" fill="{}" fill-opacity=".18" stroke="{}" stroke-opacity=".65" stroke-width="1.5"/><g transform="translate({} {}) scale(.5)">{body}</g>"##,c.cursor_color,c.cursor_color,32.-hx*factor/2.,32.-hy*factor/2.)}
pub(super) fn atlas(c:&Config)->String{
 if !enabled(c){return cursor_svg(c);}
 let svg=cursor_svg(c);let body=svg.split_once('>').unwrap().1.strip_suffix("</svg>").unwrap();
 let cell=cell(c);let size=c.cursor_size as f64;
 let mut text=format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}">"#,cell*16,cell*64);
 for shape in 0..3{let (tipx,tipy)=hotspot(c,shape);let custom=pack_style(c,shape);let body=if c.kenney_cursor{custom.as_str()}else if c.cursor!="normal"||shape==0{body}else if shape==1{r##"<path d="M24 29V10Q24 5 29 5Q34 5 34 10V25Q35 20 40 22L51 28Q55 30 54 37L51 54Q50 59 44 59H30Q26 59 23 54L12 39Q9 34 13 31Q17 28 21 34L24 38Z" fill="white" stroke="#1b2029" stroke-width="2.4" stroke-linejoin="round"/>"##}else{r##"<path d="M21 8H43M32 8V56M21 56H43" fill="none" stroke="white" stroke-width="7"/><path d="M21 8H43M32 8V56M21 56H43" fill="none" stroke="#15171c" stroke-width="3"/>"##};
 for (press,scale)in [0.88,0.94,1.].into_iter().enumerate(){for(level,length)in LENGTHS.into_iter().enumerate(){for direction in 0..16{
  let angle=direction as f64*22.5;let x=direction*cell+PAD;let y=(shape as u32*18+press as u32*6+level as u32)*cell+PAD;let id=format!("blur{shape}_{press}_{level}_{direction}");
  if level>0{text+=&format!(r#"<defs><filter id="{id}" x="-200%" y="-200%" width="500%" height="500%" color-interpolation-filters="sRGB"><feGaussianBlur stdDeviation="{} 0.15"/></filter></defs>"#,length/3.);}
  let filter=if level>0{format!(" filter=\"url(#{id})\"")}else{String::new()};
  text+=&format!(r#"<g transform="translate({x} {y})"><g transform="rotate({angle} {tipx} {tipy})"><g{filter}><g transform="rotate({} {tipx} {tipy}) translate({tipx} {tipy}) scale({scale}) translate({} {}) scale({})">{body}</g></g></g></g>"#,-angle,-tipx,-tipy,size/64.);
 }}}}
 for shape in 3..=12{let content=if c.kenney_cursor{pack_style(c,shape)}else{pack_body(0)};text+=&format!(r#"<g transform="translate({PAD} {}) scale({})">{content}</g>"#,(54+shape as u32-3)*cell+PAD,size/64.);}
 text+"</svg>"
}

/// Retain deliberate hover stops and every interaction; replace the travel between
/// them with minimum-jerk glides. Dragged paths retain their original geometry.
pub(super) fn glides(samples:&mut[project::CursorSample],width:u32){
 let radius=(width as f64*0.003).clamp(4.,10.);let mut start=0;
 while start<samples.len(){if !samples[start].inside{start+=1;continue;}let mut end=start+1;while end<samples.len()&&samples[end].inside{end+=1;}let raw=samples[start..end].to_vec();let mut anchors=vec![0usize];
  // Preserve every drag sample and every press/release. Shape flicker and
  // incidental movement are not interaction points.
  for i in 1..raw.len(){if raw[i].buttons!=0||raw[i-1].buttons!=0||raw[i].buttons!=raw[i-1].buttons{anchors.push(i);}if raw[i].wheel!=0&&(raw[i-1].wheel==0||raw[i].time-raw[i-1].time>0.3){anchors.push(i);}if raw[i-1].wheel!=0&&raw[i].wheel==0{anchors.push(i-1);}}
  // Deliberate tooltip/link/text hover stops remain useful in a tutorial.
  // Brief hesitations, blank-space stops and aimless loops are discarded.
  let mut i=0;while i+1<raw.len(){let mut j=i+1;while j<raw.len()&&raw[j].buttons==0&&((raw[j].x-raw[i].x)as f64).hypot((raw[j].y-raw[i].y)as f64)<=radius{j+=1;}if raw[i].buttons==0&&raw[i].shape!=0&&raw[j-1].time-raw[i].time>=0.35{anchors.push(i);anchors.push(j-1);i=j;}else{i+=1;}}
  let last=raw.len()-1;let mut settled=last;while settled>0&&((raw[settled-1].x-raw[last].x)as f64).hypot((raw[settled-1].y-raw[last].y)as f64)<=radius{settled-=1;}
  if end<samples.len()||raw[last].time-raw[settled].time>=0.35{anchors.push(settled);anchors.push(last);}anchors.sort_unstable();anchors.dedup();
  for pair in anchors.windows(2){let(a,b)=(pair[0],pair[1]);if a==b||raw[a..b].iter().any(|s|s.buttons!=0){continue;}let distance=((raw[b].x-raw[a].x)as f64).hypot((raw[b].y-raw[a].y)as f64);let mut arrival=b;while arrival>a+1&&((raw[arrival-1].x-raw[b].x)as f64).hypot((raw[arrival-1].y-raw[b].y)as f64)<=radius{arrival-=1;}let end_time=raw[arrival].time.max(raw[a].time+0.01);let duration=(0.3+distance/width.max(1)as f64*0.7).clamp(0.3,0.85).min(end_time-raw[a].time);let begins=end_time-duration;
   for k in a..=b{let t=ease((raw[k].time-begins)/duration.max(0.001));samples[start+k].x=(raw[a].x as f64+(raw[b].x-raw[a].x)as f64*t).round()as i32;samples[start+k].y=(raw[a].y as f64+(raw[b].y-raw[a].y)as f64*t).round()as i32;samples[start+k].shape=if t<0.02{raw[a].shape}else if t<0.98{0}else{raw[b].shape};}
  }
  // An unconfirmed final detour is not an extra destination.
  if let Some(&last_anchor)=anchors.last(){for k in last_anchor+1..raw.len(){if raw[k].buttons==0{samples[start+k].x=raw[last_anchor].x;samples[start+k].y=raw[last_anchor].y;samples[start+k].shape=raw[last_anchor].shape;}}}
  start=end;
 }
}

#[cfg(test)]mod tests{
 use super::*;
 #[test]fn detours_become_eased_glides_but_clicks_and_drag_geometry_stay_exact(){
  let mut samples=(0..121).map(|i|project::CursorSample{context:None,wheel:0,shape:0,time:i as f64/60.,x:100+i*5,y:200+if i<100{(i as f64/100.*std::f64::consts::PI).sin().mul_add(70.,0.).round()as i32}else{i-100},inside:true,buttons:if i>=100{1}else{0},window_offset:None}).collect::<Vec<_>>();let original=samples.clone();glides(&mut samples,1920);
  assert_eq!((samples[100].x,samples[100].y),(original[100].x,original[100].y));for i in 100..121{assert_eq!((samples[i].x,samples[i].y),(original[i].x,original[i].y));}assert!((samples[50].y-200).abs()<2);assert!(samples[5].x<original[5].x);
 }
 #[test]fn mac_cursor_shapes_keep_their_hotspots_without_smoothing(){
  let mut c=Config::default();c.frame="mac".into();c.cursor="normal".into();c.smooth_cursor=false;assert!(enabled(&c));let mut program=project::CursorProgram::new(&c,1920,1080);let mut out=Vec::new();
  for shape in 0..3{let sample=project::CursorSample{context:None,wheel:0,shape,time:shape as f64,x:120,y:300,inside:true,buttons:0,window_offset:None};program.push(&sample,&mut out).unwrap();}
  let out=String::from_utf8(out).unwrap();for shape in 0..3{let (x,y)=hotspot(&c,shape);assert!(out.contains(&format!("{:.3} overlay@pointer x {};",shape as f64,120-x.round()as i32-PAD as i32)));assert!(out.contains(&format!("{:.3} overlay@pointer y {};",shape as f64,300-y.round()as i32-PAD as i32)));assert!(out.contains(&format!("{:.3} crop@pointerframe y {};",shape as f64,(shape as u32*18+12)*cell(&c))));}
 }
 #[test]fn cinematic_cursor_is_independent_of_smart_zoom_and_modes_are_exclusive(){let mut c=Config::default();c.cursor="normal".into();c.cinematic_cursor=true;c.smart_zoom=false;assert!(processed(&c));assert!(enabled(&c));assert!(!native_cursor(&c));assert!(validate(&c).is_ok());c.smooth_cursor=true;assert!(validate(&c).is_err());}
 #[test]fn smart_zoom_keeps_cinematic_motion_for_every_normal_cursor_mode(){for (smooth,cinematic_mode)in [(false,false),(true,false),(false,true)]{let c=Config{smart_zoom:true,smooth_cursor:smooth,cinematic_cursor:cinematic_mode,..Config::default()};assert!(processed(&c));assert!(cinematic(&c));assert!(!native_cursor(&c));}}
 #[test]fn kenney_is_independent_of_motion_and_hidden_remains_hidden(){let mut c=Config{kenney_cursor:true,..Config::default()};assert!(enabled(&c));assert!(!processed(&c));assert!(!native_cursor(&c));for shape in 0..=12{let (x,y)=hotspot(&c,shape);assert!(x>=0.&&x<c.cursor_size as f64&&y>=0.&&y<c.cursor_size as f64);assert!(!pack_style(&c,shape).is_empty());}assert_eq!(sprite(0.,0.,1.,30,12),(0,63));c.cursor="hidden".into();assert!(!enabled(&c));assert!(!cursor_svg(&c).contains("<path"));c.cursor="highlight".into();assert!(pack_style(&c,1).contains("<circle"));assert_eq!(hotspot(&c,1),(c.cursor_size as f64/2.,c.cursor_size as f64/2.));}
 #[test]fn stationary_pointer_is_sharp_and_velocity_selects_directional_blur(){assert_eq!(sprite(0.,0.,1.,30,0),(0,12));let x=sprite(1800.,0.,0.88,30,0);let y=sprite(0.,1800.,0.88,30,0);assert_eq!(x.0,0);assert_eq!(y.0,4);assert!(x.1>0&&x.1<6);assert_eq!(sprite(0.,0.,1.,30,2).1,48);}
}
