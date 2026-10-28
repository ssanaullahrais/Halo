use serde::{Deserialize,Serialize};
#[derive(Clone,Serialize,Deserialize)]pub struct Annotation {#[serde(default)]pub id:u64,pub kind:String,pub x:f64,pub y:f64,pub w:f64,pub h:f64,#[serde(default)]pub text:String,pub color:String,pub stroke:f64,#[serde(default="font_size")]pub font_size:f64,#[serde(default)]pub start:f64,#[serde(default)]pub end:Option<f64>,#[serde(default)]pub keyframes:Vec<AnnotationFrame>}
#[derive(Clone,Serialize,Deserialize)]pub struct AnnotationFrame {pub time:f64,pub x:f64,pub y:f64,pub w:f64,pub h:f64,pub text:String}
impl Annotation {pub fn frame(&self,time:f64)->AnnotationFrame{AnnotationFrame{time,x:self.x,y:self.y,w:self.w,h:self.h,text:self.text.clone()}} pub fn at(&self,frame:&AnnotationFrame)->Self{let mut a=self.clone();a.x=frame.x;a.y=frame.y;a.w=frame.w;a.h=frame.h;a.text=frame.text.clone();a}}
pub fn timeline(a:&Annotation,offset:f64,duration:f64)->Vec<(f64,Annotation)>{
 if a.keyframes.is_empty(){return vec![(0.,a.clone())];}
 let initial=a.keyframes.iter().rfind(|f|f.time<=offset).unwrap_or(&a.keyframes[0]);
 let mut frames=vec![(0.,a.at(initial))];
 for frame in &a.keyframes {let time=frame.time-offset;if time>0.&&time<duration&&a.end.map(|end|frame.time<end).unwrap_or(true){frames.push((time,a.at(frame)));}}
 frames
}
pub fn canvas_bounds(a:&Annotation)->(i32,i32,u32,u32){let pad=(a.stroke*5.+12.).max(if a.kind=="arrow"{48.}else{0.});let mut left=a.x.min(a.x+a.w);let mut top=a.y.min(a.y+a.h);let mut right=a.x.max(a.x+a.w);let mut bottom=a.y.max(a.y+a.h);for f in &a.keyframes{left=left.min(f.x.min(f.x+f.w));top=top.min(f.y.min(f.y+f.h));right=right.max(f.x.max(f.x+f.w));bottom=bottom.max(f.y.max(f.y+f.h));}let x=(left-pad).floor().max(0.)as i32;let y=(top-pad).floor().max(0.)as i32;(x,y,(right+pad-x as f64).ceil().max(2.)as u32,(bottom+pad-y as f64).ceil().max(2.)as u32)}
fn font_size()->f64{24.}
pub fn validate(a:&Annotation,w:u32,h:u32)->Result<(),String>{if !["arrow","rectangle","text","blur"].contains(&a.kind.as_str())||![a.x,a.y,a.w,a.h,a.stroke,a.font_size].iter().all(|v|v.is_finite())||!(1.0..=12.0).contains(&a.stroke)||!(12.0..=96.0).contains(&a.font_size)||a.text.chars().count()>500||a.color.len()!=7||!a.color.starts_with('#')||!a.color[1..].chars().all(|v|v.is_ascii_hexdigit()){return Err("Invalid annotation.".into());}for (x,y) in [(a.x,a.y),(a.x+a.w,a.y+a.h)]{if x<0.||y<0.||x>w as f64||y>h as f64{return Err("Draw inside the recording area.".into());}}if a.kind=="blur"&&(a.w.abs()<4.||a.h.abs()<4.){return Err("Make the blur area at least 4 pixels wide and high.".into());}Ok(())}
fn xml(s:&str)->String{s.replace('&',"&amp;").replace('<',"&lt;").replace('>',"&gt;").replace('"',"&quot;").replace('\'',"&apos;")}
pub fn asset(a:&Annotation)->(String,i32,i32){asset_on_canvas(a,canvas_bounds(a))}
pub fn asset_on_canvas(a:&Annotation,(x,y,width,height):(i32,i32,u32,u32))->(String,i32,i32){
 let border=(a.stroke*0.65).max(2.);
 let shape=match a.kind.as_str(){
 "arrow"=>{
  let weight=(a.stroke*2.3).min(a.stroke+a.w.hypot(a.h)/100.);let border=(weight*0.65).max(2.);let ex=a.x+a.w;let ey=a.y+a.h;let angle=a.h.atan2(a.w);let length=(10.+weight*1.6+a.w.hypot(a.h)*0.09).min(52.).min(a.w.hypot(a.h)*0.6);
  let head=format!("M{ex} {ey}L{} {}L{} {}L{} {}Z",ex-length*(angle-0.5).cos(),ey-length*(angle-0.5).sin(),ex-length*0.68*angle.cos(),ey-length*0.68*angle.sin(),ex-length*(angle+0.5).cos(),ey-length*(angle+0.5).sin());
  let shaft=format!("M{} {}L{} {}",a.x,a.y,ex-length*0.5*angle.cos(),ey-length*0.5*angle.sin());
  format!(r##"<g stroke-linecap="round" stroke-linejoin="round"><g filter="url(#stickerShadow)" stroke="#ffffff"><path d="{shaft}" fill="none" stroke-width="{}"/><path d="{head}" fill="#ffffff" stroke-width="{}"/></g><path d="{shaft}" fill="none" stroke="{}" stroke-width="{}"/><path d="{head}" fill="{}"/></g>"##,weight+border*2.,border*2.,a.color,weight,a.color)
 },
 "rectangle"=>{
  let rect=format!(r#"x="{}" y="{}" width="{}" height="{}" rx="{}" fill="none""#,a.x.min(a.x+a.w),a.y.min(a.y+a.h),a.w.abs(),a.h.abs(),8f64.min(a.w.abs()/2.).min(a.h.abs()/2.));
  format!(r##"<rect {rect} stroke="#ffffff" stroke-width="{}" filter="url(#stickerShadow)"/><rect {rect} stroke="{}" stroke-width="{}"/>"##,a.stroke+border*2.,a.color,a.stroke)
 },
 "text"=>{let lines=a.text.split('\n').enumerate().map(|(i,line)|format!(r#"<tspan x="{}" y="{}">{}</tspan>"#,a.x,a.y+a.font_size*(1.+i as f64*1.3),xml(line))).collect::<String>();format!(r##"<text font-family="Inter, Segoe UI, sans-serif" font-size="{}" font-weight="600" fill="{}" stroke="#ffffff" stroke-width="3" paint-order="stroke fill" stroke-linejoin="round" filter="url(#stickerShadow)">{lines}</text>"##,a.font_size,a.color)},
 _=>String::new()};
 (format!(r##"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="{x} {y} {width} {height}"><defs><filter id="stickerShadow" filterUnits="userSpaceOnUse" x="{x}" y="{y}" width="{width}" height="{height}"><feDropShadow dx="0" dy="2" stdDeviation="2.5" flood-color="#102b4d" flood-opacity=".32"/></filter></defs>{shape}</svg>"##),x,y)
}

#[cfg(test)]mod tests{use super::*;fn annotation()->Annotation{Annotation{id:1,kind:"text".into(),x:2.,y:3.,w:100.,h:40.,text:"<safe> & text".into(),color:"#FF375F".into(),stroke:3.,font_size:24.,start:1.,end:None,keyframes:vec![]}}#[test]fn text_is_xml_escaped(){let(svg,_,_)=asset(&annotation());assert!(svg.contains("&lt;safe&gt; &amp; text"));assert!(!svg.contains("<safe>"));}#[test]fn rejects_outside_source_and_bad_colour(){let mut a=annotation();a.w=1000.;assert!(validate(&a,320,240).is_err());a.w=100.;a.color="red".into();assert!(validate(&a,320,240).is_err());}#[test]fn reverse_rectangle_and_minimum_blur_are_validated(){let mut a=annotation();a.kind="rectangle".into();a.x=180.;a.y=160.;a.w=-100.;a.h=-80.;assert!(validate(&a,320,240).is_ok());a.kind="blur".into();a.w=2.;assert!(validate(&a,320,240).is_err());}}

#[cfg(test)]mod sticker_tests{use super::*;#[test]fn flat_arrows_have_a_visible_outline_and_nonzero_filter_bounds(){for (w,h) in [(120.,0.),(0.,120.),(-120.,0.)]{let a=Annotation{id:1,kind:"arrow".into(),x:150.,y:150.,w,h,text:String::new(),color:"#FF375F".into(),stroke:3.,font_size:24.,start:0.,end:None,keyframes:vec![]};let(svg,_,_)=asset(&a);assert!(svg.contains("filterUnits=\"userSpaceOnUse\""));assert!(svg.contains("stroke=\"#ffffff\""));assert!(!svg.contains("marker-end"));assert!(!svg.contains("NaN"));}}}

#[cfg(test)]mod timeline_tests{use super::*;#[test]fn keeps_drawing_stages_across_pause_segments(){let mut a=Annotation{id:1,kind:"rectangle".into(),x:10.,y:10.,w:100.,h:80.,text:String::new(),color:"#FF375F".into(),stroke:3.,font_size:24.,start:1.,end:None,keyframes:vec![]};a.keyframes=vec![AnnotationFrame{time:1.,x:10.,y:10.,w:10.,h:8.,text:String::new()},a.frame(2.)];let before=timeline(&a,0.,3.);assert_eq!(before.len(),3);assert_eq!(before[1].1.w,10.);assert_eq!(before[2].1.w,100.);let after=timeline(&a,2.5,1.);assert_eq!(after.len(),1);assert_eq!(after[0].1.w,100.);}}
