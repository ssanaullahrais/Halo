//! Offline cinematic camera motion derived from the saved semantic cursor timeline.
//! Every render section uses the same global plan, keeping pause/worker joins continuous.
use super::*;


#[derive(Clone,Copy,Default)]
struct Input{x:f64,y:f64,inside:bool,drag:bool,offset:[i32;2]}
#[derive(Clone,Copy)]
struct Action{time:f64,end:f64,x:f64,y:f64}
#[derive(Clone,Copy)]struct Scroll{time:f64,delta:i32}
struct ScrollGesture{start:f64,end:f64}
#[derive(Clone)]struct ContextEvent{time:f64,data:context::Context}
#[derive(Clone)]struct Shot{start:f64,peak:f64,release:f64,end:f64,region:context::Region,zoom:f64,typing:bool}
const TRANSITION:f64=1.2;
const CLICK_SETTLE:f64=0.16;
const CLICK_HOLD:f64=0.18;
fn glide(t:f64)->f64{let t=t.clamp(0.,1.);t*t*t*(10.+t*(-15.+6.*t))}
#[derive(Clone,Copy,Debug,Serialize)]
pub(super) struct State{pub x:f64,pub y:f64,pub zoom:f64}
pub(super) struct Track{pub frames:Vec<State>,width:u32,height:u32,fps:u32}

pub(super) fn prepare_cursor(job:&Session,control:&export::Control)->Res<()>{
 // Smart Zoom's planner reads and smooths these same samples; it writes the
 // cursor commands itself so the file is never parsed and smoothed twice.
 if job.config.smart_zoom{return Ok(());}
 if !cursor_motion::processed(&job.config)||job.config.cursor=="hidden"{return Ok(());}
 for(i,segment)in job.segments.iter().enumerate(){let events=project::assets_dir(job).join(format!("cursor-{i}.jsonl"));project::restore_cursor_controlled(&events,&segment.commands,&job.config,job.bounds.w,job.bounds.h,segment.duration,control)?;}Ok(())
}

pub(super) fn plan(job:&Session,control:&export::Control)->Res<Option<Track>>{
 if !job.config.smart_zoom{return Ok(None);}
 let fps=job.config.fps;let count=job.segments.iter().map(|s|(s.duration*fps as f64).round().max(1.)as usize).sum::<usize>();
 let mut inputs=Vec::new();inputs.try_reserve_exact(count).map_err(|_|"Not enough memory to plan this recording's Smart Zoom.".to_string())?;inputs.resize(count,Input::default());
 let mut contexts=Vec::new();let mut text_point=None;let mut last_click=None;
 let mut actions=Vec::<Action>::new();let mut scroll_events=Vec::new();let mut zoom_events=Vec::<(f64,i32)>::new();let mut base=0usize;let mut offset=0.;
 for (i,segment)in job.segments.iter().enumerate(){
  let path=project::assets_dir(job).join(format!("cursor-{i}.jsonl"));
  let mut samples=project::read_cursor(&path,job.bounds.w,job.bounds.h,segment.duration,control)?;
  // Camera follows the same simplified pointer path that the viewer sees.
  if cursor_motion::processed(&job.config){project::process_samples(&mut samples,job.bounds.w,job.bounds.h,cursor_motion::cinematic(&job.config));}
  if job.config.cursor!="hidden"{project::write_cursor_commands(&samples,&segment.commands,&job.config,job.bounds.w,job.bounds.h,control)?;}
  let length=(segment.duration*fps as f64).round().max(1.)as usize;let end=base+length;let mut fill=base;let mut last=Input::default();let mut down=false;let mut active_action=None;
  for sample in samples{
   if control.cancelled(){return Err("Recording cancelled.".into());}
   let viewport=sample.window_offset.unwrap_or([0,0]);if viewport[0]<0||viewport[1]<0||viewport[0]>job.bounds.w as i32/2||viewport[1]>job.bounds.h as i32/2{return Err("This recording has invalid window alignment.".into());}
   let tick=(base+(sample.time*fps as f64-0.000001).ceil().max(0.)as usize).min(end-1);
   while fill<tick{inputs[fill]=last;fill+=1;}
   last=Input{x:sample.x as f64,y:sample.y as f64,inside:sample.inside,drag:sample.buttons&1!=0,offset:viewport};inputs[tick]=last;
   let pressed=sample.buttons&1!=0;let time=offset+sample.time.min(segment.duration);
   if sample.inside&&sample.shape==2{text_point=Some((sample.x,sample.y));}
   if let Some(mut data)=sample.context.clone(){if data.activity!=0&&data.focus.is_none(){if let Some((x,y))=text_point.or(last_click){data.focus=Some(context::Region{kind:0,x:(x-100).max(0),y:(y-20).max(0),w:200.min(job.bounds.w.saturating_sub((x-100).max(0)as u32)),h:40.min(job.bounds.h.saturating_sub((y-20).max(0)as u32)),multiline:false});}}contexts.push(ContextEvent{time,data});}
   if pressed&&!down&&sample.inside{last_click=Some((sample.x,sample.y));if sample.shape!=2{text_point=None;}actions.push(Action{time,end:time,x:last.x,y:last.y});active_action=Some(actions.len()-1);}
   if sample.wheel!=0&&sample.inside{scroll_events.push(Scroll{time,delta:sample.wheel});}
   if sample.zoom!=0{zoom_events.push((time,sample.zoom));}
   if pressed{if let Some(index)=active_action{actions[index].end=time;}}else{active_action=None;}
   down=pressed;
  }
  while fill<end{inputs[fill]=last;fill+=1;}
  base=end;offset+=segment.duration;
 }
 let mut track=if job.config.manual_zoom{build_manual(&inputs,&zoom_events,job.bounds.w,job.bounds.h,fps,job.config.smart_zoom_amount)}else{build_context(&inputs,&actions,&scroll_events,job.bounds.w,job.bounds.h,fps,job.config.smart_zoom_amount,&contexts)};
 if let Some(track)=track.as_mut(){let (w,h)=export::styled_size(&job.config,job.bounds.w,job.bounds.h);let (ow,oh)=encoder::output_size(&job.config,job.bounds.w,job.bounds.h);let (x,y)=if job.config.frame=="none"{(0.,0.)}else{let pad=frame_pad(&job.config,w)as f64;(pad,pad+40.)};track.reframe(w,h,ow,oh,x,y);}
 Ok(track)
}
// Lightweight spatial shutter only; no optical flow, frame synthesis, or time warp.
// Applied before cursor/annotations/mockup so those remain crisp.
pub(super) struct ScrollTrack{spans:Vec<(f64,f64)>}
impl ScrollTrack{
 pub(super) fn plan(job:&Session,control:&export::Control)->Res<Self>{
  let mut spans=Vec::new();let mut offset=0.;
  if job.config.smart_zoom{for(i,segment)in job.segments.iter().enumerate(){let samples=project::read_cursor(&project::assets_dir(job).join(format!("cursor-{i}.jsonl")),job.bounds.w,job.bounds.h,segment.duration,control)?;let events=samples.iter().filter(|s|s.inside&&s.wheel!=0).map(|s|Scroll{time:s.time,delta:s.wheel}).collect::<Vec<_>>();for g in gestures(&events){spans.push((offset+g.start,offset+(g.end+0.04).min(segment.duration)));}offset+=segment.duration;}}
  Ok(Self{spans})
 }
 pub(super) fn filter(&self,offset:f64,duration:f64)->Option<String>{let intervals=self.spans.iter().filter_map(|&(a,b)|{let start=(a-offset).max(0.);let end=(b-offset).min(duration);(end>start).then(||format!("gte(t,{start:.6})*lt(t,{end:.6})"))}).collect::<Vec<_>>();if intervals.is_empty(){None}else{Some(format!("avgblur=sizeX=1:sizeY=3:planes=1:enable='{}'",intervals.join("+")))}}
}
// The delayed live view uses the same framing planner with only currently known
// events. Export still has the complete future timeline and final cursor glides.
pub(super) fn preview_state(samples:&[project::CursorSample],time:f64,bounds:Rect,c:&Config)->State{
 let base=(time-6.).max(0.);let finish=samples.last().map(|s|s.time).unwrap_or(time)+2.;let count=((finish-base)*c.fps as f64).ceil().max(2.)as usize;let mut inputs=vec![Input::default();count];let mut actions=Vec::new();let mut scrolls=Vec::new();let mut contexts=Vec::new();let mut zooms=Vec::<(f64,i32)>::new();let mut last=Input::default();let mut fill=0usize;let mut down=false;let mut active=None;
 for s in samples.iter().filter(|s|s.time>=base){let t=s.time-base;let tick=((t*c.fps as f64).round()as usize).min(count-1);while fill<tick{inputs[fill]=last;fill+=1;}last=Input{x:s.x as f64,y:s.y as f64,inside:s.inside,drag:s.buttons&1!=0,offset:s.window_offset.unwrap_or([0,0])};inputs[tick]=last;let pressed=s.buttons&1!=0;if pressed&&!down&&s.inside{actions.push(Action{time:t,end:t,x:last.x,y:last.y});active=Some(actions.len()-1);}if pressed{if let Some(i)=active{actions[i].end=t;}}else{active=None;}down=pressed;if s.wheel!=0&&s.inside{scrolls.push(Scroll{time:t,delta:s.wheel});}if s.zoom!=0{zooms.push((t,s.zoom));}if let Some(data)=&s.context{contexts.push(ContextEvent{time:t,data:data.clone()});}}
 while fill<count{inputs[fill]=last;fill+=1;}let track=if c.manual_zoom{build_manual(&inputs,&zooms,bounds.w,bounds.h,c.fps,c.smart_zoom_amount)}else{build_context(&inputs,&actions,&scrolls,bounds.w,bounds.h,c.fps,c.smart_zoom_amount,&contexts)};let (w,h)=export::styled_size(c,bounds.w,bounds.h);let (ow,oh)=encoder::output_size(c,bounds.w,bounds.h);let pad=if c.frame=="none"{0.}else{frame_pad(c,w)as f64};if let Some(mut track)=track{track.reframe(w,h,ow,oh,pad,if c.frame=="none"{0.}else{pad+40.});track.frames.get(((time-base)*c.fps as f64).round()as usize).copied().unwrap_or(State{x:0.,y:0.,zoom:1.})}else{State{x:0.,y:0.,zoom:1.}}
}
// Manual Smart Zoom: the camera always follows the cursor; Numpad +/- step the
// zoom level up or down and every step eases in, so held keys ramp smoothly.
const MANUAL_EASE:f64=0.35;
fn build_manual(inputs:&[Input],events:&[(f64,i32)],width:u32,height:u32,fps:u32,amount:f64)->Option<Track>{
 if events.is_empty()||inputs.len()<2{return None;}
 // One key press is the whole gesture: + jumps straight to the configured
 // strength, - returns straight to 1x. There is no incremental stepping.
 let mut checkpoints=vec![(0.,1.)];
 for &(time,delta)in events{checkpoints.push((time,if delta>0{amount}else{1.}));}
 let mut frames=Vec::with_capacity(inputs.len());let mut idx=0usize;
 for(i,input)in inputs.iter().enumerate(){
  let time=i as f64/fps as f64;
  while idx+1<checkpoints.len()&&checkpoints[idx+1].0<=time{idx+=1;}
  let(change,target)=checkpoints[idx];let previous=if idx==0{1.}else{checkpoints[idx-1].1};
  let zoom=(previous+(target-previous)*glide((time-change)/MANUAL_EASE)).max(1.);
  let(vw,vh)=(width as f64/zoom,height as f64/zoom);
  let mut x=(input.x-vw/2.).clamp(0.,width as f64-vw);let mut y=(input.y-vh/2.).clamp(0.,height as f64-vh);
  let(left,top)=(input.offset[0]as f64,input.offset[1]as f64);let(right,bottom)=(width as f64-left,height as f64-top);
  if right-left>=vw{x=x.clamp(left,right-vw);}if bottom-top>=vh{y=y.clamp(top,bottom-vh);}
  frames.push(State{x,y,zoom});
 }
 if frames.iter().all(|s|s.zoom<1.00001){None}else{Some(Track{frames,width,height,fps})}
}
fn gestures(events:&[Scroll])->Vec<ScrollGesture>{
 let mut result=Vec::<ScrollGesture>::new();let(mut start,mut last,mut distance,mut emitted)=(0.,-1.,0u64,false);
 for event in events{if event.time-last>0.3{start=event.time;distance=0;emitted=false;}last=event.time;distance+=event.delta.unsigned_abs()as u64;if distance>=120{if !emitted{result.push(ScrollGesture{start,end:event.time});emitted=true;}else if let Some(g)=result.last_mut(){g.end=event.time;}}}result
}
fn nearby(a:&context::Region,b:&context::Region,w:u32,h:u32)->bool{let(ax,ay)=(a.x as f64+a.w as f64/2.,a.y as f64+a.h as f64/2.);let(bx,by)=(b.x as f64+b.w as f64/2.,b.y as f64+b.h as f64/2.);(ax-bx).abs()<w as f64*0.22&&(ay-by).abs()<h as f64*0.24}
fn union(a:&context::Region,b:&context::Region)->context::Region{let(x,y)=(a.x.min(b.x),a.y.min(b.y));context::Region{x,y,w:(a.x as i64+a.w as i64).max(b.x as i64+b.w as i64).saturating_sub(x as i64)as u32,h:(a.y as i64+a.h as i64).max(b.y as i64+b.h as i64).saturating_sub(y as i64)as u32,..a.clone()}}
fn strength(r:&context::Region,typing:bool,w:u32,h:u32,amount:f64)->f64{
 let cap=if typing||r.kind==1||r.kind==4{1.28}else if r.kind==0{1.32}else{1.45};let margin=if typing{1.3}else{1.2};amount.min(cap).min(w as f64/(r.w as f64*margin).max(1.)).min(h as f64/(r.h as f64*margin).max(1.)).max(1.)
}
fn contains_point(r:&context::Region,x:f64,y:f64)->bool{x>=r.x as f64-8.&&y>=r.y as f64-8.&&x<=r.x as f64+r.w as f64+8.&&y<=r.y as f64+r.h as f64+8.}
fn target_at(action:&Action,contexts:&[ContextEvent])->Option<context::Region>{let current=contexts.iter().rev().find(|e|e.time<=action.time+0.02);current.and_then(|e|e.data.target.as_ref()).filter(|r|contains_point(r,action.x,action.y)).cloned().or_else(||contexts.iter().filter(|e|e.time>action.time&&e.time<=action.time+0.25&&current.is_none_or(|c|c.data.view==0||e.data.view==c.data.view)).find_map(|e|e.data.target.as_ref().filter(|r|contains_point(r,action.x,action.y))).cloned())}
// A focused edit is a task, not an idle timer. Only bounds/categories are used.
fn stays_in_task(inputs:&[Input],a:&context::Region,b:&context::Region,from:f64,to:f64,fps:u32,width:u32,height:u32)->bool{let r=union(a,b);let px=width as f64*0.16;let py=height as f64*0.18;let first=(from*fps as f64).floor().max(0.)as usize;let end=((to*fps as f64).ceil()as usize+1).min(inputs.len());let mut outside=0;let mut run=0;let mut longest=0;let Some(samples)=inputs.get(first..end)else{return false;};for p in samples{let near=p.inside&&p.x>=r.x as f64-px&&p.x<=r.x as f64+r.w as f64+px&&p.y>=r.y as f64-py&&p.y<=r.y as f64+r.h as f64+py;if near{run=0;}else{outside+=1;run+=1;longest=longest.max(run);}}!samples.is_empty()&&outside as f64/samples.len()as f64<=0.25&&longest as f64/fps as f64<=0.45}
fn focus_release(begin:f64,region:&context::Region,initial_edit:Option<context::Region>,contexts:&[ContextEvent],actions:&[Action],scrolls:&[ScrollGesture],finish:f64,width:u32,height:u32)->f64{
 let mut release=finish.max(begin);let view=contexts.iter().rev().find(|e|e.time<=begin+0.02).map(|e|e.data.view).unwrap_or(0);
 for(i,e)in contexts.iter().enumerate().filter(|(_,e)|e.time>begin+0.03){let end_key=e.data.activity&context::ESCAPE!=0||e.data.activity&context::ENTER!=0&&!e.data.focus.as_ref().is_some_and(|r|r.multiline);let other_view=view!=0&&e.data.view!=0&&e.data.view!=view;let lost=e.time>begin+0.4&&e.data.focus.as_ref().is_none_or(|r|!nearby(r,region,width,height));let recovered=lost&&contexts.iter().skip(i+1).take_while(|next|next.time-e.time<=0.35).any(|next|next.data.focus.as_ref().is_some_and(|r|nearby(r,region,width,height)));if end_key||other_view||lost&&!recovered{release=release.min(e.time);break;}}
 if let Some(g)=scrolls.iter().find(|g|g.start>begin){release=release.min((g.start-TRANSITION).max(begin));}
 let mut edit=initial_edit;for a in actions.iter().filter(|a|a.time>begin+0.03&&a.time<=release){let target=target_at(a,contexts);if let Some(r)=target.as_ref().filter(|r|r.kind==1){if contains_point(region,a.x,a.y)||nearby(region,r,width,height){edit=Some(r.clone());continue;}}if target.as_ref().is_some_and(|r|r.kind==2||r.kind==3)||!edit.as_ref().map(|r|contains_point(r,a.x,a.y)).unwrap_or_else(||contains_point(region,a.x,a.y)){release=a.time;break;}}
 release
}
fn build_context(inputs:&[Input],actions:&[Action],scroll_events:&[Scroll],width:u32,height:u32,fps:u32,amount:f64,contexts:&[ContextEvent])->Option<Track>{
 // Providers may report a newly focused field a fraction after the first key.
 // Export can resolve that event using the subsequent bounds, before animating it.
 let mut resolved=contexts.to_vec();for i in 0..resolved.len(){if resolved[i].data.activity&context::TYPE!=0&&resolved[i].data.focus.as_ref().is_none_or(|r|r.kind==0){if let Some(r)=contexts.iter().skip(i+1).take_while(|e|e.time-resolved[i].time<=0.6).find_map(|e|e.data.focus.clone()){resolved[i].data.focus=Some(r);}}}let contexts=resolved.as_slice();
 if inputs.len()<2{return None;}let finish=(inputs.len()-1)as f64/fps as f64;let scrolls=gestures(scroll_events);let mut shots=Vec::<Shot>::new();let mut typing=None::<Shot>;let mut last_key=-10.;let mut previous_view=0;let mut boundaries=Vec::new();let mut endings=Vec::new();
 // Typing is activity in a focused editable control, not pointer hover. Nearby
 // fields share a region. Only timing/categories are stored, never characters.
 for e in contexts{let c=&e.data;if c.view!=0&&previous_view!=0&&c.view!=previous_view{boundaries.push(e.time);}if c.view!=0{previous_view=c.view;}
  let editable=c.focus.as_ref().filter(|r|r.kind==0||r.kind==1||r.kind==4);let multiline=editable.is_some_and(|r|r.multiline);
  let ended=c.activity&context::ESCAPE!=0||c.activity&context::ENTER!=0&&!multiline;
  if ended||editable.is_none()&&typing.as_ref().is_some_and(|s|s.region.kind!=0){if let Some(mut shot)=typing.take(){shot.release=shot.release.min(e.time);shot.peak=shot.peak.min(shot.release);shot.end=shot.release+TRANSITION;shots.push(shot);}if ended{endings.push(e.time);}}
  if c.activity&context::TYPE==0&&!(c.activity&context::ENTER!=0&&multiline){continue;}let Some(region)=editable else{continue;};
  if let Some(open)=typing.as_mut().filter(|shot|e.time-last_key<=0.85&&!scrolls.iter().any(|g|g.start<=e.time&&g.end>=last_key)&&nearby(&shot.region,region,width,height)){open.region=union(&open.region,region);open.zoom=strength(&open.region,true,width,height,amount);open.release=e.time+0.32;open.end=open.release+TRANSITION;}
  else{if let Some(old)=typing.take(){shots.push(old);}typing=Some(Shot{start:(e.time-0.22).max(0.),peak:e.time+0.32,release:e.time+0.32,end:e.time+0.32+TRANSITION,region:region.clone(),zoom:strength(region,true,width,height,amount),typing:true});}last_key=e.time;
 }
 if let Some(shot)=typing{shots.push(shot);}
 // Hold confirmed focused edits across typing pauses. Legacy pointer-only
 // estimates retain a bounded shot because their focus cannot be verified.
 for shot in &mut shots{if shot.typing&&shot.region.kind!=0{let began=contexts.iter().find(|e|e.time>=shot.start&&e.time<=shot.peak&&e.data.activity&context::TYPE!=0&&e.data.focus.as_ref().is_some_and(|r|nearby(r,&shot.region,width,height))).map(|e|e.time).unwrap_or(shot.start+0.22);let edit=actions.iter().rev().filter(|a|a.time<=began+0.25).find_map(|a|target_at(a,contexts).filter(|r|r.kind==1&&contains_point(&shot.region,a.x,a.y))).or_else(||(shot.region.kind==1).then(||shot.region.clone()));shot.release=focus_release(began,&shot.region,edit,contexts,actions,&scrolls,finish,width,height);shot.end=shot.release+TRANSITION;}}
 // A click into an editable control focuses it even before the first key.
 for a in actions{let target=target_at(a,contexts);let focus=contexts.iter().filter(|e|e.time>=a.time&&e.time<=a.time+0.45).find_map(|e|e.data.focus.as_ref().filter(|r|contains_point(r,a.x,a.y)).cloned()).or_else(||contexts.iter().rev().find(|e|e.time<=a.time).and_then(|e|e.data.focus.as_ref()).filter(|r|contains_point(r,a.x,a.y)).cloned());let edit=target.filter(|r|r.kind==1).or_else(||focus.as_ref().filter(|r|r.kind==1).cloned());let Some(edit)=edit else{continue;};let region=focus.filter(|r|r.kind==1||r.kind==4).unwrap_or_else(||edit.clone());let release=focus_release(a.time,&region,Some(edit),contexts,actions,&scrolls,finish,width,height);shots.push(Shot{start:(a.time-0.3).max(0.),peak:(a.time+0.3).max(0.6),release,end:release+TRANSITION,zoom:strength(&region,true,width,height,amount),region,typing:true});}
 shots.sort_by(|a,b|a.start.total_cmp(&b.start));let mut focused=Vec::<Shot>::new();for shot in shots{if let Some(previous)=focused.last_mut().filter(|p|p.typing&&shot.typing&&p.region.kind!=0&&shot.region.kind!=0&&shot.start<=p.release&&!boundaries.iter().chain(endings.iter()).any(|t|*t>p.start&&*t<=shot.peak)&&nearby(&p.region,&shot.region,width,height)){previous.region=union(&previous.region,&shot.region);previous.zoom=strength(&previous.region,true,width,height,amount);previous.release=previous.release.max(shot.release);previous.end=previous.release+TRANSITION;}else{focused.push(shot);}}let mut shots=focused;let typing_shots=shots.clone();
 // Clicks get one brief centered emphasis. Hover and central pointer jitter do
 // not produce targets. Missing accessibility metadata uses a modest point shot.
 for action in actions{
  // Omit a pulse when its complete approach/return would overlap scrolling.
  // Compressing the ramp here creates the very fast camera movement that the
  // offline plan is intended to avoid; keep the overview until there is room.
  if scrolls.iter().any(|g|action.time-TRANSITION-CLICK_SETTLE<g.end+0.12&&action.end.max(action.time+CLICK_HOLD)+TRANSITION>g.start-0.12){continue;}
  if typing_shots.iter().any(|s|s.region.kind!=0&&action.time>=s.peak&&action.time<=s.release+0.03){continue;}
  if typing_shots.iter().any(|s|action.time>=s.start-0.65&&action.time<=s.release&&nearby(&s.region,&context::Region{kind:0,x:action.x as i32,y:action.y as i32,w:1,h:1,multiline:false},width,height)){continue;}
  let contains=|r:&context::Region|action.x>=r.x as f64-8.&&action.y>=r.y as f64-8.&&action.x<=r.x as f64+r.w as f64+8.&&action.y<=r.y as f64+r.h as f64+8.;
  let target=target_at(action,contexts);
  let known=contexts.iter().any(|e|e.data.view!=0||e.data.focus.is_some()||e.data.target.is_some());
  // Editing focus and empty-space clicks aren't new camera intentions.
  if target.as_ref().is_some_and(|r|r.kind==1||r.kind==4)||known&&target.is_none(){continue;}
  if let Some(edit)=shots.iter_mut().rev().find(|s|s.typing&&action.time>=s.start&&action.time-s.release<1.5&&contains(&s.region)){edit.release=edit.release.max(action.time+CLICK_HOLD);edit.end=edit.release+TRANSITION;continue;}
  let mut r=target.unwrap_or(context::Region{kind:0,x:action.x as i32,y:action.y as i32,w:1,h:1,multiline:false});
  if action.end-action.time>0.25{for(i,input)in inputs.iter().enumerate(){let t=i as f64/fps as f64;if t>=action.time&&t<=action.end&&input.inside&&input.drag{r=union(&r,&context::Region{kind:0,x:input.x as i32,y:input.y as i32,w:1,h:1,multiline:false});}}}
  let start=(action.time-TRANSITION-CLICK_SETTLE).max(0.);let peak=(action.time-CLICK_SETTLE).max(start+0.12);let release=(action.time+CLICK_HOLD).max(action.end);let mut shot=Shot{start,peak,release,end:release+TRANSITION,zoom:strength(&r,false,width,height,amount),region:r,typing:false};
  if let Some(previous)=shots.last_mut().filter(|s|!s.typing&&action.time<=s.release+6.&&nearby(&s.region,&shot.region,width,height)&&stays_in_task(inputs,&s.region,&shot.region,s.peak,action.time,fps,width,height)&&!scrolls.iter().any(|g|g.start>s.peak&&g.start<action.time)&&!boundaries.iter().chain(endings.iter()).any(|t|*t>s.peak&&*t<=action.time)&&!typing_shots.iter().any(|edit|edit.start>s.peak&&edit.start<action.time)){previous.region=union(&previous.region,&shot.region);previous.zoom=strength(&previous.region,false,width,height,amount);previous.release=shot.release;previous.end=shot.end;continue;}
  // A newly opened page/dialog ends old framing. Never keep zooming toward a
  // previous button after the interface has changed under it.
  if let Some(t)=boundaries.iter().chain(endings.iter()).copied().filter(|t|*t>action.time&&*t<shot.end).min_by(f64::total_cmp){shot.release=shot.release.min((t-0.25).max(action.time));shot.peak=shot.peak.min(shot.release);shot.end=t+0.15;}
  shots.push(shot);
 }
 for shot in &mut shots{
  if shot.typing{let approach=shot.peak-shot.start;for g in &scrolls{if g.end>=shot.start&&g.start<shot.peak{shot.start=shot.start.max(g.end+0.12);shot.peak=shot.start+approach;}}if shot.peak>shot.release{shot.zoom=1.;}}
  if let Some(g)=scrolls.iter().find(|g|g.start>shot.start&&g.start<shot.end){shot.end=g.start;shot.release=shot.release.min((g.start-TRANSITION).max(shot.start));shot.peak=shot.peak.min(shot.release);}
  if !(shot.typing&&shot.region.kind!=0&&shot.release>=finish){shot.end=shot.end.min(finish);shot.release=shot.release.min((finish-0.55).max(shot.start));}shot.peak=shot.peak.min(shot.release);
 }
 shots.retain(|s|s.zoom>1.001&&s.end-s.start>0.4&&s.peak-s.start>0.1);shots.sort_by(|a,b|a.start.total_cmp(&b.start));if shots.is_empty(){return None;}
 // Resolve unrelated shots through a wider view instead of panning a tightly
 // cropped camera across the page. Continuous viewport blending avoids jumps.
 for i in 1..shots.len(){if shots[i-1].end>shots[i].start&&!nearby(&shots[i-1].region,&shots[i].region,width,height){let start=shots[i].start;shots[i-1].release=shots[i-1].release.min((start-0.15).max(shots[i-1].start));shots[i-1].peak=shots[i-1].peak.min(shots[i-1].release);shots[i-1].end=shots[i-1].end.min(start+0.25);}}
 let hold_at_end=shots.iter().any(|s|s.typing&&s.region.kind!=0&&s.release>=finish);
 let mut frames=Vec::with_capacity(inputs.len());
 for(i,input)in inputs.iter().enumerate(){let time=i as f64/fps as f64;let(mut sx,mut sy,mut sw,mut sh,mut weight)=(0.,0.,0.,0.,0.);
  for shot in shots.iter().filter(|s|time>=s.start&&time<=s.end){let e=glide((time-shot.start)/(shot.peak-shot.start).max(1./fps as f64)).min(1.-glide((time-shot.release)/(shot.end-shot.release).max(1./fps as f64)));if e<=0.{continue;}
   let(vw,vh)=(width as f64/shot.zoom,height as f64/shot.zoom);let(cx,cy)=(shot.region.x as f64+shot.region.w as f64/2.,shot.region.y as f64+shot.region.h as f64/2.);let x=(cx-vw/2.).clamp(0.,width as f64-vw);let y=(cy-vh/2.).clamp(0.,height as f64-vh);sx+=x*e;sy+=y*e;sw+=vw*e;sh+=vh*e;weight+=e;
  }
  let normalization=weight.max(1.);let base=(1.-weight).max(0.);let vw=(sw+width as f64*base)/normalization;let vh=(sh+height as f64*base)/normalization;let mut x=sx/normalization;let mut y=sy/normalization;
  // Native window resizing stays centered; never scale the capture to follow it.
  let (left,top)=(input.offset[0]as f64,input.offset[1]as f64);let(right,bottom)=(width as f64-left,height as f64-top);if right-left>=vw{x=x.clamp(left,right-vw);}if bottom-top>=vh{y=y.clamp(top,bottom-vh);}
  frames.push(if i==0||i+1==inputs.len()&&!hold_at_end{State{x:0.,y:0.,zoom:1.}}else{State{x:x.clamp(0.,width as f64-vw),y:y.clamp(0.,height as f64-vh),zoom:width as f64/vw}});
 }
 if frames.iter().all(|s|s.zoom<1.00001){None}else{Some(Track{frames,width,height,fps})}
}

impl Track{
 /// Only moving sections need dense shutter integration. Splitting at these
 /// frame-aligned boundaries keeps long held shots cheap and sharp.
 pub(super) fn split_parts<'a>(&self,parts:Vec<render::Part<'a>>)->Vec<render::Part<'a>>{
  let rect=|s:&State|[s.x,s.y,s.x+self.width as f64/s.zoom,s.y+self.height as f64/s.zoom];
  let moving=(0..self.frames.len()).map(|i|{let here=rect(&self.frames[i]);[i.saturating_sub(1),(i+1).min(self.frames.len()-1)].into_iter().any(|j|{let other=rect(&self.frames[j]);(0..4).any(|k|(here[k]-other[k]).abs()>0.25)})}).collect::<Vec<_>>();
  let mut result=Vec::new();for part in parts{let first=(part.offset*self.fps as f64).round()as usize;let count=(part.duration*self.fps as f64).round()as usize;let mut from=0;
   while from<count{let active=moving.get(first+from).copied().unwrap_or(false);let mut end=from+1;while end<count&&moving.get(first+end).copied().unwrap_or(false)==active{end+=1;}
    let duration=(end-from)as f64/self.fps as f64;result.push(render::Part{segment:part.segment,start:part.start+from as f64/self.fps as f64,offset:part.offset+from as f64/self.fps as f64,duration,cost:part.cost*duration/part.duration*if active{7.}else{1.}});from=end;
   }
  }result
 }

 fn reframe(&mut self,content_w:u32,content_h:u32,width:u32,height:u32,origin_x:f64,origin_y:f64){
  for state in &mut self.frames{let cx=(state.x+self.width as f64/state.zoom/2.)*content_w as f64/self.width as f64+origin_x;let cy=(state.y+self.height as f64/state.zoom/2.)*content_h as f64/self.height as f64+origin_y;let vw=width as f64/state.zoom;let vh=height as f64/state.zoom;state.x=(cx-vw/2.).clamp(0.,width as f64-vw);state.y=(cy-vh/2.).clamp(0.,height as f64-vh);}
  self.width=width;self.height=height;
 }
 pub(super) fn filter(&self,offset:f64,duration:f64)->Option<String>{
  let start=(offset*self.fps as f64).round()as usize;let end=(start+(duration*self.fps as f64).round().max(1.)as usize).min(self.frames.len());let states=self.frames.get(start..end)?;
  if states.is_empty()||states.iter().all(|s|s.zoom<1.00001){return None;}
  // Include neighbours so subframe shutter samples remain continuous at worker joins.
  let first=start.saturating_sub(1);let last=(end+1).min(self.frames.len());let origin=start-first;
  let rects=self.frames[first..last].iter().map(|s|[s.x,s.y,s.x+self.width as f64/s.zoom,s.y+self.height as f64/s.zoom]).collect::<Vec<_>>();let keys=simplify(&rects,0.08);
  let expressions=(0..4).map(|column|expression(&rects,&keys,column,0,keys.len()-1)).collect::<Vec<_>>();
  let perspective=|shutter:f64|{let e=expressions.iter().map(|s|s.replace("on",&format!("(on+{origin}+{shutter:.3})"))).collect::<Vec<_>>();format!("perspective=x0='{}':y0='{}':x1='{}':y1='{}':x2='{}':y2='{}':x3='{}':y3='{}':sense=source:interpolation=cubic:eval=frame",e[0],e[1],e[2],e[1],e[0],e[3],e[2],e[3])};
  let moving=rects.windows(2).any(|p|(0..4).any(|k|(p[1][k]-p[0][k]).abs()>0.25));
  if !moving{return Some(perspective(0.));}
  // Spatial shutter integration: blend transforms of this same source frame,
  // avoiding previous-frame ghosts and preserving source playback timing.
  let mut graph=format!("split=7{};",(0..7).map(|i|format!("[camera{i}]")).collect::<String>());
  for i in 0..7{graph+=&format!("[camera{i}]{}[shutter{i}];",perspective((i as f64-3.)*0.18/3.).replace("interpolation=cubic","interpolation=linear"));}
  graph+=&(0..7).map(|i|format!("[shutter{i}]")).collect::<String>();
  graph+="mix=inputs=7:weights='1 2 3 4 3 2 1':scale=0.0625";Some(graph)
 }
}
fn simplify(values:&[[f64;4]],tolerance:f64)->Vec<usize>{
 if values.len()<2{return vec![0];}
 let mut keep=vec![false;values.len()];keep[0]=true;keep[values.len()-1]=true;let mut stack=vec![(0,values.len()-1)];
 while let Some((a,b))=stack.pop(){let mut largest=tolerance;let mut index=None;for i in a+1..b{let t=(i-a)as f64/(b-a)as f64;let error=(0..4).map(|k|(values[i][k]-(values[a][k]+(values[b][k]-values[a][k])*t)).abs()).fold(0.,f64::max);if error>largest{largest=error;index=Some(i);}}if let Some(i)=index{keep[i]=true;stack.push((a,i));stack.push((i,b));}}
 keep.into_iter().enumerate().filter_map(|(i,v)|v.then_some(i)).collect()
}
fn expression(values:&[[f64;4]],keys:&[usize],column:usize,a:usize,b:usize)->String{
 let (first,last)=(keys[a],keys[b]);let (from,to)=(values[first][column],values[last][column]);
 if b<=a+1{if (to-from).abs()<0.00001||first==last{return format!("{from:.5}");}return format!("({from:.5}+{:.5}*clip((on-{})/{},0,1))",to-from,first+1,last-first);}
 let mid=(a+b)/2;format!("if(lt(on,{}),{},{})",keys[mid]+1,expression(values,keys,column,a,mid),expression(values,keys,column,mid,b))
}

#[cfg(test)]mod tests{
 use super::*;
 fn input(fps:usize)->Vec<Input>{vec![Input{x:960.,y:540.,inside:true,drag:false,offset:[0,0]};fps*12]}
 fn field()->context::Region{context::Region{kind:1,x:80,y:300,w:400,h:50,multiline:false}}
 fn event(time:f64,activity:u8)->ContextEvent{ContextEvent{time,data:context::Context{focus:Some(field()),activity,..Default::default()}}}
 #[test]fn button_entry_and_return_use_equal_durations(){let click=3.;let start=click-TRANSITION-CLICK_SETTLE;let peak=click-CLICK_SETTLE;let release=click+CLICK_HOLD;let end=release+TRANSITION;assert!(((peak-start)-(end-release)).abs()<0.000001);let track=build_context(&input(30),&[Action{time:click,end:click+0.05,x:800.,y:400.}],&[],1920,1080,30,2.,&[]).unwrap();assert!(track.frames[60].zoom>1.03,"entry should be visible a second before the click");assert!(track.frames[87].zoom>1.31);}
 #[test]fn hover_and_cursor_noise_never_move_the_camera(){let actions=[Action{time:1.,end:1.05,x:850.,y:500.}];let a=input(30);let mut b=a.clone();for(i,s)in b.iter_mut().enumerate(){s.x=(i*71%1900)as f64;s.y=(i*43%1000)as f64;s.inside=i%5!=0;}let first=build_context(&a,&actions,&[],1920,1080,30,2.,&[]).unwrap();let next=build_context(&b,&actions,&[],1920,1080,30,2.,&[]).unwrap();for(a,b)in first.frames.iter().zip(next.frames){assert_eq!((a.x,a.y,a.zoom),(b.x,b.y,b.zoom));}assert!(build_context(&a,&[],&[],1920,1080,30,2.,&[]).is_none());}
 #[test]fn a_click_at_the_very_start_of_the_recording_still_zooms_in(){for fps in [30usize,60]{let track=build_context(&input(fps),&[Action{time:0.05,end:0.1,x:600.,y:400.}],&[],1920,1080,fps as u32,2.,&[]).unwrap();assert!(track.frames[(fps as f64*0.3)as usize].zoom>1.2,"a click in the first frames must not be dropped by the shot filter");}}
 #[test]fn clicks_have_an_earlier_entry_and_gentler_return(){for fps in [30,60]{let track=build_context(&input(fps),&[Action{time:1.,end:1.05,x:600.,y:400.}],&[],1920,1080,fps as u32,2.,&[]).unwrap();assert!(track.frames[(fps as f64*1.05)as usize].zoom>1.3);assert!(track.frames[(fps as f64*1.5)as usize].zoom<track.frames[(fps as f64*1.05)as usize].zoom);assert_eq!(track.frames[(fps as f64*2.5)as usize].zoom,1.);assert!(track.frames.iter().all(|s|s.zoom<=1.321));}}
 #[test]fn typing_is_gentle_stable_and_enter_starts_the_return(){for fps in [30,60]{let mut events=(0..21).map(|i|event(1.+i as f64*0.15,context::TYPE)).collect::<Vec<_>>();events.push(event(4.1,context::ENTER));let track=build_context(&input(fps),&[],&[],1920,1080,fps as u32,2.,&events).unwrap();let a=&track.frames[fps*2];assert!(a.zoom>1.27&&a.zoom<=1.28);for s in &track.frames[fps*2..fps*4]{assert_eq!((a.x,a.y,a.zoom),(s.x,s.y,s.zoom));}assert!(track.frames[(fps as f64*4.4)as usize].zoom<1.27);assert_eq!(track.frames[fps*6].zoom,1.);}}
 #[test]fn multiline_enter_keeps_typing_but_escape_finishes(){let mut events=vec![event(1.,context::TYPE),event(1.5,context::ENTER),event(1.9,context::TYPE),event(2.,context::ESCAPE)];for e in &mut events{e.data.focus.as_mut().unwrap().multiline=true;}let track=build_context(&input(30),&[],&[],1920,1080,30,2.,&events).unwrap();assert!(track.frames[48].zoom>1.27);assert_eq!(track.frames[105].zoom,1.);}
 #[test]fn scroll_adjacent_clicks_do_not_pull_anticipation_into_wheel_gestures(){for fps in [30usize,60]{let scrolls=(0..6).map(|i|Scroll{time:2.+i as f64*0.2,delta:120}).collect::<Vec<_>>();for click in [1.5,3.6,4.2,4.6]{let actions=[Action{time:click,end:click+0.05,x:1200.,y:500.}];let track=build_context(&input(fps),&actions,&scrolls,1920,1080,fps as u32,2.,&[]);if click<4.5{assert!(track.is_none(),"an overlapping click pulse should be omitted rather than accelerated around scrolling: {fps}fps click={click}");}else{let t=track.unwrap();assert!(t.frames[(click*fps as f64)as usize].zoom>1.3);for s in &t.frames[2*fps..=3*fps]{assert_eq!(s.zoom,1.);}}}}}
 #[test]fn typing_after_scroll_waits_for_settled_content_without_a_fast_entry(){for fps in [30usize,60]{let scrolls=(0..6).map(|i|Scroll{time:2.+i as f64*0.2,delta:120}).collect::<Vec<_>>();let events=[event(3.1,context::TYPE),event(3.4,context::TYPE),event(3.7,context::TYPE)];let track=build_context(&input(fps),&[],&scrolls,1920,1080,fps as u32,2.,&events).unwrap();for s in &track.frames[2*fps..=3*fps]{assert_eq!(s.zoom,1.,"typing anticipation must not zoom during scrolling");}assert!(track.frames[(3.8*fps as f64)as usize].zoom>1.27);let brief=build_context(&input(fps),&[],&scrolls,1920,1080,fps as u32,2.,&events[..1]);assert!(brief.unwrap().frames[(3.8*fps as f64)as usize].zoom>1.27,"a focused field remains active during an idle pause");}}
 #[test]fn typing_resumes_after_an_intervening_scroll_even_with_short_key_gaps(){for fps in [30usize,60]{let events=[1.7,2.1,2.8,3.2,3.6].into_iter().map(|t|event(t,context::TYPE)).collect::<Vec<_>>();let scrolls=(0..5).map(|i|Scroll{time:2.2+i as f64*0.2,delta:120}).collect::<Vec<_>>();let track=build_context(&input(fps),&[],&scrolls,1920,1080,fps as u32,2.,&events).unwrap();for s in &track.frames[(2.2*fps as f64)as usize..=3*fps]{assert_eq!(s.zoom,1.);}assert!(track.frames[(3.8*fps as f64)as usize].zoom>1.27,"typing after scrolling must keep its own shot");}}
 #[test]fn sustained_scroll_is_wide_and_scroll_alone_never_zooms(){let inputs=input(30);let actions=[Action{time:1.,end:1.05,x:900.,y:500.}];let scrolls=[Scroll{time:1.6,delta:120},Scroll{time:1.8,delta:120}];let track=build_context(&inputs,&actions,&scrolls,1920,1080,30,2.,&[]);if let Some(track)=track{for s in &track.frames[48..70]{assert_eq!(s.zoom,1.);}}assert!(build_context(&inputs,&[],&scrolls,1920,1080,30,2.,&[]).is_none());}
 #[test]fn tiny_scroll_noise_does_not_change_framing(){let inputs=input(30);let actions=[Action{time:1.,end:1.05,x:900.,y:500.}];let a=build_context(&inputs,&actions,&[],1920,1080,30,2.,&[]).unwrap();let b=build_context(&inputs,&actions,&[Scroll{time:1.3,delta:12}],1920,1080,30,2.,&[]).unwrap();for(a,b)in a.frames.iter().zip(b.frames){assert_eq!((a.x,a.y,a.zoom),(b.x,b.y,b.zoom));}}
 #[test]fn edge_targets_stay_visible_and_all_motion_is_bounded(){for fps in [30,60]{let actions=[Action{time:1.,end:1.05,x:1900.,y:1060.},Action{time:3.,end:3.05,x:10.,y:10.}];let track=build_context(&input(fps),&actions,&[],1920,1080,fps as u32,2.,&[]).unwrap();for s in &track.frames{assert!(s.x>=0.&&s.y>=0.&&s.x+1920./s.zoom<=1920.0001&&s.y+1080./s.zoom<=1080.0001);}for a in &actions{let s=&track.frames[(a.time*fps as f64)as usize];assert!(a.x>=s.x&&a.x<=s.x+1920./s.zoom&&a.y>=s.y&&a.y<=s.y+1080./s.zoom);}for pair in track.frames.windows(2){assert!((pair[0].zoom-pair[1].zoom).abs()<0.06);}}}
 #[test]fn dialog_bounds_fit_and_a_navigation_ends_old_framing(){let mut r=field();r.kind=4;r.x=600;r.y=200;r.w=720;r.h=600;let mut events=vec![event(1.,context::TYPE),event(1.2,context::TYPE),event(1.4,context::TYPE)];for e in &mut events{e.data.focus=Some(r.clone());}events.push(ContextEvent{time:1.5,data:context::Context{activity:context::ENTER,view:2,..Default::default()}});let track=build_context(&input(30),&[],&[],1920,1080,30,2.,&events).unwrap();for s in &track.frames{assert!(s.zoom<=1.28);assert!(r.x as f64>=s.x&&r.y as f64>=s.y&&r.x as f64+r.w as f64<=s.x+1920./s.zoom&&r.y as f64+r.h as f64<=s.y+1080./s.zoom);}assert_eq!(track.frames[90].zoom,1.);}
 #[test]fn repeated_field_clicks_hold_steady_while_blank_clicks_stay_wide(){let inputs=input(30);let clicks=(0..8).map(|i|Action{time:1.+i as f64*0.4,end:1.05+i as f64*0.4,x:300.,y:320.}).collect::<Vec<_>>();let contexts=[ContextEvent{time:0.,data:context::Context{focus:Some(field()),view:1,..Default::default()}}];let track=build_context(&inputs,&clicks,&[],1920,1080,30,2.,&contexts).unwrap();let anchor=track.frames[60];assert_eq!(track.frames.last().unwrap().zoom,anchor.zoom,"an active field must not zoom out just because recording ends");for s in &track.frames[60..180]{assert_eq!((s.x,s.y,s.zoom),(anchor.x,anchor.y,anchor.zoom));}let blank=clicks.into_iter().map(|a|Action{x:1000.,y:800.,..a}).collect::<Vec<_>>();assert!(build_context(&inputs,&blank,&[],1920,1080,30,2.,&contexts).is_none());}
 #[test]fn a_clicked_field_holds_through_idle_until_an_outside_click(){for fps in [30usize,60]{let contexts=[ContextEvent{time:0.,data:context::Context{focus:Some(field()),target:Some(field()),view:1,..Default::default()}},ContextEvent{time:7.2,data:context::Context{view:1,..Default::default()}}];let actions=[Action{time:2.,end:2.05,x:300.,y:320.},Action{time:7.,end:7.05,x:1200.,y:800.}];let track=build_context(&input(fps),&actions,&[],1920,1080,fps as u32,2.,&contexts).unwrap();let anchor=track.frames[fps*3];assert!(anchor.zoom>1.27);for s in &track.frames[fps*3..=fps*7]{assert_eq!((s.x,s.y,s.zoom),(anchor.x,anchor.y,anchor.zoom));}assert!(track.frames[(7.5*fps as f64)as usize].zoom>1.1);assert_eq!(track.frames[(8.3*fps as f64)as usize].zoom,1.);}}

 #[test]fn a_previous_pages_control_is_never_reused_for_a_blank_click(){let old=context::Region{kind:2,x:800,y:480,w:80,h:30,multiline:false};let contexts=[ContextEvent{time:0.,data:context::Context{target:Some(old),view:1,..Default::default()}},ContextEvent{time:0.5,data:context::Context{view:2,..Default::default()}}];assert!(build_context(&input(30),&[Action{time:1.,end:1.05,x:830.,y:490.}],&[],1920,1080,30,2.,&contexts).is_none());}
 #[test]fn delayed_focus_bounds_resolve_first_typing_activity(){let e=[ContextEvent{time:1.,data:context::Context{activity:context::TYPE,..Default::default()}},event(1.25,0)];let track=build_context(&input(30),&[],&[],1920,1080,30,2.,&e).unwrap();assert!(track.frames[40].zoom>1.27);}
 #[test]fn nearby_clicks_share_one_stable_shot(){let r=context::Region{kind:2,x:800,y:480,w:80,h:30,multiline:false};let events=[ContextEvent{time:0.,data:context::Context{target:Some(r),view:1,..Default::default()}}];let actions=(0..4).map(|i|Action{time:1.+i as f64*0.8,end:1.05+i as f64*0.8,x:830.,y:490.}).collect::<Vec<_>>();let track=build_context(&input(30),&actions,&[],1920,1080,30,2.,&events).unwrap();let anchor=&track.frames[30];for s in &track.frames[30..100]{assert_eq!((s.x,s.y,s.zoom),(anchor.x,anchor.y,anchor.zoom));}}
 #[test]fn nearby_clicks_after_a_thoughtful_pause_share_a_shot_but_new_views_do_not(){for fps in [30usize,60]{let r=context::Region{kind:2,x:800,y:480,w:80,h:30,multiline:false};let events=[ContextEvent{time:0.,data:context::Context{target:Some(r.clone()),view:1,..Default::default()}}];let actions=[Action{time:2.,end:2.05,x:830.,y:490.},Action{time:5.,end:5.05,x:850.,y:500.}];let track=build_context(&input(fps),&actions,&[],1920,1080,fps as u32,2.,&events).unwrap();let anchor=track.frames[fps*2];for s in &track.frames[fps*2..=fps*5]{assert_eq!((s.x,s.y,s.zoom),(anchor.x,anchor.y,anchor.zoom));}let mut changed=events.to_vec();changed.push(ContextEvent{time:3.,data:context::Context{target:Some(r),view:2,..Default::default()}});let separated=build_context(&input(fps),&actions,&[],1920,1080,fps as u32,2.,&changed).unwrap();assert_eq!(separated.frames[(3.4*fps as f64)as usize].zoom,1.);}}
 #[test]fn a_deliberate_move_out_of_the_working_area_separates_click_shots(){let mut data=input(30);for p in &mut data[90..120]{p.x=1700.;p.y=900.;}let actions=[Action{time:2.,end:2.05,x:830.,y:490.},Action{time:5.,end:5.05,x:850.,y:500.}];let track=build_context(&data,&actions,&[],1920,1080,30,2.,&[]).unwrap();assert_eq!(track.frames[105].zoom,1.);}
 #[test]fn manual_zoom_jumps_straight_to_the_configured_strength_on_one_press(){for fps in [30usize,60]{let track=build_manual(&input(fps),&[(1.,1)],1920,1080,fps as u32,2.).unwrap();assert_eq!(track.frames[0].zoom,1.);assert!(track.frames[(fps as f64*1.5)as usize].zoom>1.95,"a single zoom-in press must reach the full configured strength, not an intermediate step");let held=track.frames[(fps as f64*3.)as usize].zoom;assert_eq!(track.frames.last().unwrap().zoom,held,"a manual zoom level must hold until the next key press");}}
 #[test]fn manual_zoom_out_returns_straight_to_1x_on_one_press(){let mut samples=input(30);for s in &mut samples[90..]{s.x=1500.;s.y=850.;}let track=build_manual(&samples,&[(1.,1),(4.,-1)],1920,1080,30,2.).unwrap();let zoomed=&track.frames[90];assert!(zoomed.zoom>1.95,"a single zoom-in press must reach the full configured strength");assert!(zoomed.x+1920./zoomed.zoom>1500.&&1500.>=zoomed.x,"the camera should be centred on the live cursor position");assert_eq!(track.frames.last().unwrap().zoom,1.,"a single zoom-out press must return fully to 1x");}
 #[test]fn no_manual_key_presses_means_no_zoom_track(){assert!(build_manual(&input(30),&[],1920,1080,30,2.).is_none());}
 #[test]fn blur_sections_preserve_frames_and_holds_stay_sharp(){let track=build_context(&input(30),&[],&[],1920,1080,30,2.,&[event(1.,context::TYPE),event(1.5,context::TYPE),event(2.,context::TYPE)]).unwrap();let segment=Segment{raw:PathBuf::new(),audio:vec![],commands:PathBuf::new(),duration:12.};let parts=track.split_parts(vec![render::Part{segment:&segment,start:0.,duration:12.,offset:0.,cost:12.}]);assert_eq!(parts.iter().map(|p|(p.duration*30.).round()as usize).sum::<usize>(),360);assert!(track.filter(0.9,0.3).unwrap().contains("split=7"));assert!(!track.filter(1.6,0.2).unwrap().contains("split="));}
 #[test]#[ignore="requires local FFmpeg"]fn kenney_cursor_does_not_disable_the_smart_zoom_camera(){
  for kenney in [false,true]{
  for fps in [30usize,60]{
   let mut job=super::super::media_tests::fixture(fps as u32,"moderate");
   job.config.smart_zoom=true;job.config.kenney_cursor=kenney;job.config.cursor="normal".into();
   job.segments[0].duration=8.;
   let mut text=String::new();
   for tick in 0..8*fps{let t=tick as f64/fps as f64;let (x,y)=if t<2.{(60,70)}else{(340,210)};let sample=project::CursorSample{context:None,wheel:0,shape:0,zoom:0,time:t,x,y,inside:true,buttons:if [fps,2*fps,4*fps].contains(&tick){1}else{0},window_offset:None};text+=&format!("{}\n",serde_json::to_string(&sample).unwrap());}
   fs::write(job.dir.join("cursor-0.jsonl"),text).unwrap();
   let control=export::Control::default();
   let track=plan(&job,&control).unwrap();
   assert!(track.is_some(),"kenney_cursor={kenney} must not disable the smart-zoom camera track, fps={fps}");
   let track=track.unwrap();
   assert!(track.frames[fps*2..fps*3].iter().any(|s|s.zoom>1.1),"camera should still zoom in after the click when kenney_cursor={kenney}, fps={fps}");
  }}
 }
}
