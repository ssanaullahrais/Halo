use serde::{Deserialize,Serialize};
#[derive(Clone,Copy,Serialize,Deserialize,PartialEq)]pub struct CurvePoint{pub x:f64,pub y:f64}
#[derive(Clone,Serialize,Deserialize)]pub struct Annotation {#[serde(default)]pub id:u64,pub kind:String,pub x:f64,pub y:f64,pub w:f64,pub h:f64,#[serde(default)]pub text:String,pub color:String,pub stroke:f64,#[serde(default="font_size")]pub font_size:f64,#[serde(default)]pub start:f64,#[serde(default)]pub end:Option<f64>,#[serde(default)]pub keyframes:Vec<AnnotationFrame>,/// Bend points along an arrow, in order from start to end: with none it's a straight line; each
/// one pulls the line into a smooth curve through it (same technique as a freehand smoothing spline).
#[serde(default)]pub points:Vec<CurvePoint>}
#[derive(Clone,Serialize,Deserialize)]pub struct AnnotationFrame {pub time:f64,pub x:f64,pub y:f64,pub w:f64,pub h:f64,pub text:String,pub color:String,pub stroke:f64,pub font_size:f64,pub points:Vec<CurvePoint>}
impl Annotation {pub fn frame(&self,time:f64)->AnnotationFrame{AnnotationFrame{time,x:self.x,y:self.y,w:self.w,h:self.h,text:self.text.clone(),color:self.color.clone(),stroke:self.stroke,font_size:self.font_size,points:self.points.clone()}} pub fn at(&self,frame:&AnnotationFrame)->Self{let mut a=self.clone();a.x=frame.x;a.y=frame.y;a.w=frame.w;a.h=frame.h;a.text=frame.text.clone();a.color=frame.color.clone();a.stroke=frame.stroke;a.font_size=frame.font_size;a.points=frame.points.clone();a}}
/// Preserve every visual edit, including bends that leave the endpoints unchanged.
pub fn record_frame(a:&mut Annotation,time:f64){
 let frame=a.frame(time);
 if let Some(last)=a.keyframes.last(){
  if last.x==frame.x&&last.y==frame.y&&last.w==frame.w&&last.h==frame.h&&last.text==frame.text&&last.color==frame.color&&last.stroke==frame.stroke&&last.font_size==frame.font_size&&last.points==frame.points{return;}
  if last.time==time{a.keyframes.pop();}
 }
 a.keyframes.push(frame);
}
pub fn timeline(a:&Annotation,offset:f64,duration:f64)->Vec<(f64,Annotation)>{
 if a.keyframes.is_empty(){return vec![(0.,a.clone())];}
 let initial=a.keyframes.iter().rfind(|f|f.time<=offset).unwrap_or(&a.keyframes[0]);
 let mut frames=vec![(0.,a.at(initial))];
 for frame in &a.keyframes {let time=frame.time-offset;if time>0.&&time<duration&&a.end.map(|end|frame.time<end).unwrap_or(true){frames.push((time,a.at(frame)));}}
 frames
}
pub fn canvas_bounds(a:&Annotation)->(i32,i32,u32,u32){let stroke=a.keyframes.iter().map(|f|f.stroke).fold(a.stroke,f64::max);let pad=(stroke*5.+12.).max(if a.kind=="arrow"{48.}else{0.});let mut left=a.x.min(a.x+a.w);let mut top=a.y.min(a.y+a.h);let mut right=a.x.max(a.x+a.w);let mut bottom=a.y.max(a.y+a.h);for f in &a.keyframes{left=left.min(f.x.min(f.x+f.w));top=top.min(f.y.min(f.y+f.h));right=right.max(f.x.max(f.x+f.w));bottom=bottom.max(f.y.max(f.y+f.h));for c in &f.points{left=left.min(c.x);top=top.min(c.y);right=right.max(c.x);bottom=bottom.max(c.y);}}for c in &a.points{left=left.min(c.x);top=top.min(c.y);right=right.max(c.x);bottom=bottom.max(c.y);}let x=(left-pad).floor().max(0.)as i32;let y=(top-pad).floor().max(0.)as i32;(x,y,(right+pad-x as f64).ceil().max(2.)as u32,(bottom+pad-y as f64).ceil().max(2.)as u32)}
fn font_size()->f64{24.}
pub fn validate(a:&Annotation,w:u32,h:u32)->Result<(),String>{if !["arrow","rectangle","ellipse","pencil","text","blur"].contains(&a.kind.as_str())||![a.x,a.y,a.w,a.h,a.stroke,a.font_size].iter().all(|v|v.is_finite())||!(1.0..=12.0).contains(&a.stroke)||!(12.0..=96.0).contains(&a.font_size)||a.text.chars().count()>500||a.color.len()!=7||!a.color.starts_with('#')||!a.color[1..].chars().all(|v|v.is_ascii_hexdigit())||a.points.len()>if a.kind=="pencil"{4096}else{20}||a.points.iter().any(|p|!p.x.is_finite()||!p.y.is_finite()||p.x<0.||p.y<0.||p.x>w as f64||p.y>h as f64){return Err("Invalid annotation.".into());}for (x,y) in [(a.x,a.y),(a.x+a.w,a.y+a.h)]{if x<0.||y<0.||x>w as f64||y>h as f64{return Err("Draw inside the recording area.".into());}}if a.kind=="blur"&&(a.w.abs()<4.||a.h.abs()<4.){return Err("Make the blur area at least 4 pixels wide and high.".into());}Ok(())}
fn curve_samples(a:&Annotation)->Vec<CurvePoint>{
 let mut controls=vec![CurvePoint{x:a.x,y:a.y}];controls.extend(a.points.iter().copied());controls.push(CurvePoint{x:a.x+a.w,y:a.y+a.h});
 let mut samples=vec![controls[0]];
 if controls.len()==2{for i in 1..=16{let t=i as f64/16.;samples.push(CurvePoint{x:a.x+a.w*t,y:a.y+a.h*t});}}
 else{
  let mut quadratic=|control:CurvePoint,end:CurvePoint|{let start=*samples.last().unwrap();for i in 1..=8{let t=i as f64/8.;let m=1.-t;samples.push(CurvePoint{x:m*m*start.x+2.*m*t*control.x+t*t*end.x,y:m*m*start.y+2.*m*t*control.y+t*t*end.y});}};
  for i in 1..controls.len()-2{quadratic(controls[i],CurvePoint{x:(controls[i].x+controls[i+1].x)/2.,y:(controls[i].y+controls[i+1].y)/2.});}
  quadratic(controls[controls.len()-2],controls[controls.len()-1]);
 }
 samples
}
fn brush_path(samples:&[CurvePoint],stroke:f64,lane:f64,spread:f64)->String{
 let expanded;let samples=if samples.len()==2{let (a,b)=(samples[0],samples[1]);expanded=(0..=16).map(|i|{let t=i as f64/16.;CurvePoint{x:a.x+(b.x-a.x)*t,y:a.y+(b.y-a.y)*t}}).collect::<Vec<_>>();expanded.as_slice()}else{samples};
 let last=samples.len().saturating_sub(1);let mut reduced:Vec<CurvePoint>=Vec::new();for (i,p)in samples.iter().copied().enumerate(){if i==last||reduced.last().map(|q|(p.x-q.x).hypot(p.y-q.y)>=1.5).unwrap_or(true){reduced.push(p);}}let samples=reduced;
 let mut distances=vec![0.];for i in 1..samples.len(){distances.push(distances[i-1]+(samples[i].x-samples[i-1].x).hypot(samples[i].y-samples[i-1].y));}
 let total=distances.last().copied().unwrap_or(0.).max(0.000001);let mut left=Vec::new();let mut right=Vec::new();
 for (i,p)in samples.iter().enumerate(){let before=samples[i.saturating_sub(1)];let after=samples[(i+1).min(samples.len()-1)];let dx=after.x-before.x;let dy=after.y-before.y;let length=dx.hypot(dy).max(0.000001);let t=distances[i]/total;let radius=stroke*1.6*(0.025+0.975*(std::f64::consts::PI*t).sin().max(0.).powf(0.65))*(1.+0.1*(t*61.).sin()+0.04*(t*127.).sin());let center=lane+if lane!=0.{0.035*(t*91.+lane*7.).sin()}else{0.};let width=if spread<0.2{spread*(0.3+0.7*(t*43.+lane*5.).sin().max(0.))}else{spread};left.push((p.x-dy/length*radius*(center+width),p.y+dx/length*radius*(center+width)));right.push((p.x-dy/length*radius*(center-width),p.y+dx/length*radius*(center-width)));}
 let mut d=String::new();for (i,(x,y))in left.into_iter().chain(right.into_iter().rev()).enumerate(){d+=&format!("{}{x:.3} {y:.3}",if i==0{"M"}else{"L"});}d.push('Z');d
}
fn shape_strokes(a:&Annotation)->Vec<(Vec<CurvePoint>,f64)>{
 if a.kind=="pencil"{return vec![(curve_samples(a),a.stroke)];}
 if a.kind=="arrow"{
  let ex=a.x+a.w;let ey=a.y+a.h;let angle=match a.points.last(){Some(p)=>(ey-p.y).atan2(ex-p.x),None=>a.h.atan2(a.w)};let weight=(a.stroke*2.3).min(a.stroke+a.w.hypot(a.h)/100.);let length=(10.+weight*1.6+a.w.hypot(a.h)*0.09).min(52.).min(a.w.hypot(a.h)*0.6);let mut strokes=vec![(curve_samples(a),a.stroke)];
  for offset in [-0.5,0.5]{strokes.push((vec![CurvePoint{x:ex-length*(angle+offset).cos(),y:ey-length*(angle+offset).sin()},CurvePoint{x:ex,y:ey}],a.stroke*0.8));}return strokes;
 }
 let (x,y,w,h)=(a.x.min(a.x+a.w),a.y.min(a.y+a.h),a.w.abs(),a.h.abs());let mut samples=Vec::new();
 if a.kind=="ellipse"{let n=((std::f64::consts::PI*(w+h)/6.).ceil()as usize).clamp(32,512);for i in 0..=n{let t=i as f64/n as f64*std::f64::consts::TAU-std::f64::consts::FRAC_PI_2;samples.push(CurvePoint{x:x+w/2.+t.cos()*w/2.,y:y+h/2.+t.sin()*h/2.});}}
 else if a.kind=="rectangle"{
  fn line(samples:&mut Vec<CurvePoint>,x:f64,y:f64){let p=*samples.last().unwrap();let n=(((x-p.x).hypot(y-p.y)/4.).ceil()as usize).clamp(1,256);for i in 1..=n{let t=i as f64/n as f64;samples.push(CurvePoint{x:p.x+(x-p.x)*t,y:p.y+(y-p.y)*t});}}
  fn arc(samples:&mut Vec<CurvePoint>,x:f64,y:f64,start:f64,r:f64){for i in 1..=8{let t=start+i as f64/8.*std::f64::consts::FRAC_PI_2;samples.push(CurvePoint{x:x+t.cos()*r,y:y+t.sin()*r});}}
  let r=8f64.min(w/2.).min(h/2.);samples.push(CurvePoint{x:x+r,y});line(&mut samples,x+w-r,y);arc(&mut samples,x+w-r,y+r,-std::f64::consts::FRAC_PI_2,r);line(&mut samples,x+w,y+h-r);arc(&mut samples,x+w-r,y+h-r,0.,r);line(&mut samples,x+r,y+h);arc(&mut samples,x+r,y+h-r,std::f64::consts::FRAC_PI_2,r);line(&mut samples,x,y+r);arc(&mut samples,x+r,y+r,std::f64::consts::PI,r);
 }
 vec![(samples,a.stroke)]
}
fn brush_svg(a:&Annotation)->String{
 let mut svg=String::new();for (stroke_index,(samples,stroke))in shape_strokes(a).into_iter().enumerate(){for (i,(lane,spread,opacity,blur))in [(0.,1.,0.45,(stroke*0.2).max(0.5)),(0.,0.72,0.65,(stroke*0.06).max(0.18)),(-0.84,0.065,0.28,0.),(-0.56,0.065,0.28,0.),(-0.27,0.065,0.28,0.),(0.13,0.065,0.28,0.),(0.43,0.065,0.28,0.),(0.76,0.065,0.28,0.)].into_iter().enumerate(){
  let d=brush_path(&samples,stroke,lane,spread);let id=format!("brush{stroke_index}_{i}");let filter=if blur>0.{format!(r#" filter="url(#{id})""#)}else{String::new()};if blur>0.{svg+=&format!(r#"<defs><filter id="{id}" x="-50%" y="-50%" width="200%" height="200%"><feGaussianBlur stdDeviation="{blur}"/></filter></defs>"#);}svg+=&format!(r#"<path d="{d}" fill="{}" opacity="{opacity}"{filter}/>"#,a.color);
 }}svg
}

fn xml(s:&str)->String{s.replace('&',"&amp;").replace('<',"&lt;").replace('>',"&gt;").replace('"',"&quot;").replace('\'',"&apos;")}
pub fn asset(a:&Annotation)->(String,i32,i32){asset_on_canvas(a,canvas_bounds(a))}
pub fn asset_on_canvas(a:&Annotation,(x,y,width,height):(i32,i32,u32,u32))->(String,i32,i32){
 let shape=match a.kind.as_str(){
 "arrow"|"rectangle"|"ellipse"|"pencil"=>brush_svg(a),
 "text"=>{let lines=a.text.split('\n').enumerate().map(|(i,line)|format!(r#"<tspan x="{}" y="{}">{}</tspan>"#,a.x,a.y+a.font_size*(1.+i as f64*1.5),xml(line))).collect::<String>();format!(r##"<text font-family="Segoe Print, Kalam, Comic Sans MS, cursive" font-size="{}" font-weight="400" fill="{}" fill-opacity=".92">{lines}</text>"##,a.font_size,a.color)},
 _=>String::new()};
 (format!(r##"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="{x} {y} {width} {height}"><defs><filter id="stickerShadow" filterUnits="userSpaceOnUse" x="{x}" y="{y}" width="{width}" height="{height}"><feDropShadow dx="0" dy="2" stdDeviation="2.5" flood-color="#102b4d" flood-opacity=".32"/></filter></defs>{shape}</svg>"##),x,y)
}

#[cfg(test)]mod tests{use super::*;fn annotation()->Annotation{Annotation{id:1,kind:"text".into(),x:2.,y:3.,w:100.,h:40.,text:"<safe> & text".into(),color:"#FF375F".into(),stroke:3.,font_size:24.,start:1.,end:None,keyframes:vec![],points:vec![]}}#[test]fn text_is_xml_escaped(){let(svg,_,_)=asset(&annotation());assert!(svg.contains("&lt;safe&gt; &amp; text"));assert!(!svg.contains("<safe>"));}#[test]fn rejects_outside_source_and_bad_colour(){let mut a=annotation();a.w=1000.;assert!(validate(&a,320,240).is_err());a.w=100.;a.color="red".into();assert!(validate(&a,320,240).is_err());}#[test]fn reverse_rectangle_and_minimum_blur_are_validated(){let mut a=annotation();a.kind="rectangle".into();a.x=180.;a.y=160.;a.w=-100.;a.h=-80.;assert!(validate(&a,320,240).is_ok());a.kind="blur".into();a.w=2.;assert!(validate(&a,320,240).is_err());}}

#[cfg(test)]mod sticker_tests{use super::*;#[test]fn flat_arrows_have_soft_ink_and_nonzero_filter_bounds(){for (w,h) in [(120.,0.),(0.,120.),(-120.,0.)]{let a=Annotation{id:1,kind:"arrow".into(),x:150.,y:150.,w,h,text:String::new(),color:"#FF375F".into(),stroke:3.,font_size:24.,start:0.,end:None,keyframes:vec![],points:vec![]};let(svg,_,_)=asset(&a);assert!(svg.contains("filterUnits=\"userSpaceOnUse\""));assert!(svg.contains("<feGaussianBlur"));assert!(!svg.contains("marker-end"));assert!(!svg.contains("NaN"));}}}

#[cfg(test)]mod timeline_tests{use super::*;#[test]fn keeps_drawing_stages_across_pause_segments(){let mut a=Annotation{id:1,kind:"rectangle".into(),x:10.,y:10.,w:100.,h:80.,text:String::new(),color:"#FF375F".into(),stroke:3.,font_size:24.,start:1.,end:None,keyframes:vec![],points:vec![]};a.keyframes=vec![AnnotationFrame{w:10.,h:8.,..a.frame(1.)},a.frame(2.)];let before=timeline(&a,0.,3.);assert_eq!(before.len(),3);assert_eq!(before[1].1.w,10.);assert_eq!(before[2].1.w,100.);let after=timeline(&a,2.5,1.);assert_eq!(after.len(),1);assert_eq!(after[0].1.w,100.);}}

#[cfg(test)]mod bend_timeline_tests{
 use super::*;
 #[test]fn replay_preserves_straight_bent_and_rebent_stages(){
  let mut a=Annotation{id:1,kind:"arrow".into(),x:30.,y:80.,w:260.,h:0.,text:String::new(),color:"#FF375F".into(),stroke:3.,font_size:24.,start:0.,end:None,keyframes:vec![],points:vec![]};
  record_frame(&mut a,0.);
  a.points=vec![CurvePoint{x:160.,y:180.}];record_frame(&mut a,0.4);
  a.points[0].y=220.;record_frame(&mut a,0.8);
  a.points.clear();record_frame(&mut a,1.);
  let frames=timeline(&a,0.,1.2);
  assert_eq!(frames.len(),4);
  assert!(frames[0].1.points.is_empty());
  assert_eq!(frames[1].1.points.len(),1,"the first bend must be recorded");
  assert_eq!(frames[1].1.points[0].y,180.);
  assert_eq!(frames[2].1.points[0].y,220.);
  assert!(frames[3].1.points.is_empty());
  let continued=timeline(&a,0.6,0.6);assert_eq!(continued[0].1.points[0].y,180.);
 }
}

#[cfg(test)]mod brush_tests{
 use super::*;
 #[test]fn brush_raster_has_a_thick_middle_and_fine_ends(){
  let a=Annotation{id:1,kind:"pencil".into(),x:10.,y:40.,w:260.,h:0.,text:String::new(),color:"#2563eb".into(),stroke:8.,font_size:24.,start:0.,end:None,keyframes:vec![],points:vec![]};
  let (svg,x,_)=asset(&a);let tree=resvg::usvg::Tree::from_str(&svg,&resvg::usvg::Options::default()).unwrap();let size=tree.size().to_int_size();let mut pix=resvg::tiny_skia::Pixmap::new(size.width(),size.height()).unwrap();resvg::render(&tree,resvg::tiny_skia::Transform::identity(),&mut pix.as_mut());
  let width=|column:i32|pix.data().chunks_exact(4).enumerate().filter(|(i,p)|i%size.width()as usize==(column-x)as usize&&p[0]<80&&p[1]<150&&p[2]>180&&p[3]>200).count();
  let middle=width(140);assert!(middle>=12,"middle must have brush weight: {middle}");assert!(width(12)<middle/2&&width(268)<middle/2,"both ends must taper");assert!(!svg.contains("NaN"));
 }
}

#[cfg(test)]mod font_fallback_tests{
 use super::*;
 #[test]fn bundled_handwriting_renders_without_any_system_fonts(){
  let mut database=resvg::usvg::fontdb::Database::new();database.load_font_data(include_bytes!("../../src/assets/fonts/Kalam-Regular.ttf").to_vec());
  assert!(database.faces().any(|face|face.families.iter().any(|(name,_)|name=="Kalam")));
  let a=Annotation{id:1,kind:"text".into(),x:20.,y:20.,w:300.,h:70.,text:"Handwritten, offline".into(),color:"#2563eb".into(),stroke:3.,font_size:24.,start:0.,end:None,keyframes:vec![],points:vec![]};let(svg,_,_)=asset(&a);
  let mut options=resvg::usvg::Options::default();options.fontdb=std::sync::Arc::new(database);let tree=resvg::usvg::Tree::from_str(&svg,&options).unwrap();let size=tree.size().to_int_size();let mut pix=resvg::tiny_skia::Pixmap::new(size.width(),size.height()).unwrap();resvg::render(&tree,resvg::tiny_skia::Transform::identity(),&mut pix.as_mut());assert!(pix.data().chunks_exact(4).filter(|p|p[3]>30).count()>300,"bundled fallback must produce real glyphs");
 }
}
