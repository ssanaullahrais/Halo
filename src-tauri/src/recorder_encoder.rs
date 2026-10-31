//! Hardware encoders share bounded probing, quality tiers and CPU fallback.
use super::*;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub(super) enum Encoder{Nvenc,Qsv,Amf,Cpu}
impl Encoder{
 pub(super) fn name(self)->&'static str{match self{Self::Nvenc=>"h264_nvenc",Self::Qsv=>"h264_qsv",Self::Amf=>"h264_amf",Self::Cpu=>"libx264"}}
 pub(super) fn hardware(self)->bool{self!=Self::Cpu}
 pub(super) fn settings(self,command:&mut std::process::Command,c:&Config,capture:bool){
  let q=match c.quality.as_str(){"archive"=>12,"balanced"=>23,_=>16}.to_string();
  command.args(["-c:v",self.name()]);
  match self{
   Self::Nvenc=>{
    command.args(["-preset",if capture{"p1"}else{"p5"},"-rc","vbr","-cq",&q,"-b:v","0"]);
    if capture{command.args(["-zerolatency","1","-delay","0","-bf","0"]);}
   }
   Self::Qsv=>{
    command.args(["-preset",if capture{"veryfast"}else{"medium"},"-global_quality",&q,"-look_ahead","0"]);
    if capture{command.args(["-async_depth","1","-bf","0"]);}
   }
   Self::Amf=>{
    // CQP works on older AMF drivers as well as recent ones; no bitrate cap
    // that could make detailed desktop text lose quality under load.
    command.args(["-usage",if capture{"lowlatency"}else{"transcoding"},"-quality",if capture{"speed"}else{"quality"},"-rc","cqp","-qp_i",&q,"-qp_p",&q,"-qp_b",&q,"-bf","0"]);
    if capture{command.args(["-async_depth","1"]);}
   }
   Self::Cpu=>{
    command.args(["-preset",if capture{"ultrafast"}else{"medium"},"-crf",&q]);
    if capture{command.args(["-tune","zerolatency"]);}
   }
  }
  command.args(["-pix_fmt",if self==Self::Qsv{"nv12"}else{"yuv420p"}]);
 }
}
pub(super) fn candidates(config:&Config,preferred:Encoder)->Vec<Encoder>{
 if config.encoder=="cpu"{return vec![Encoder::Cpu];}
 let mut encoders=vec![Encoder::Nvenc,Encoder::Qsv,Encoder::Amf];
 if preferred.hardware(){encoders.retain(|e|*e!=preferred);encoders.insert(0,preferred);}
 encoders.push(Encoder::Cpu);encoders
}
pub(super) fn output_size(config:&Config,width:u32,height:u32)->(u32,u32){
 let (w,h)=export::styled_size(config,width,height);
 if config.frame=="none"{(w,h)}else{let pad=frame_pad(config,w);(w+2*pad,h+2*pad+40)}
}
pub(super) fn export_with_fallback<T>(config:&Config,preferred:Encoder,control:&export::Control,mut probe:impl FnMut(Encoder)->bool,mut render:impl FnMut(Encoder)->Res<T>)->Res<T>{
 let mut last_error=None;
 for encoder in candidates(config,preferred){
  if control.cancelled(){return Err("Recording cancelled.".into());}
  control.set_stage(1);
  if !probe(encoder){continue;}
  match render(encoder){
   Ok(result)=>return Ok(result),
   Err(error)=>{
    if encoder==Encoder::Cpu||!hardware_error(&error)||control.cancelled(){return Err(error);}
    log_error(&format!("{} export failed; trying the next encoder: {error}",encoder.name()));last_error=Some(error);
   }
  }
 }
 if control.cancelled(){return Err("Recording cancelled.".into());}
 Err(last_error.unwrap_or_else(||"No video encoder could start. Your original video and project are saved.".into()))
}
pub(super) fn probe(encoder:Encoder,config:&Config,width:u32,height:u32,capture:bool,control:&export::Control)->bool{
 if control.cancelled(){return false;}
 if encoder==Encoder::Cpu{return true;}
 let mut command=ffmpeg();command.args(["-f","lavfi","-i",&format!("color=size={}x{}:rate={}",(width+1)/2*2,(height+1)/2*2,config.fps),"-frames:v","4","-an"]);
 encoder.settings(&mut command,config,capture);command.args(["-f","null","-"]);
 match export::run_probe(command,control.clone()){
  Ok(())=>true,
  Err(error)=>{if !control.cancelled(){log_error(&format!("{} {} probe unavailable: {error}",encoder.name(),if capture{"capture"}else{"export"}));}false}
 }
}
// Retry hardware initialization/driver failures, not corrupt inputs, filter
// errors, destination failures, cancellation or ordinary export watchdogs.
pub(super) fn hardware_error(error:&str)->bool{
 let e=error.to_ascii_lowercase();
 e.contains("video encoder did not start")||((e.contains("[h264_nvenc")||e.contains("[h264_qsv")||e.contains("[h264_amf")||e.contains("mfx_err_")||e.contains("amf_"))&&[
  "not supported","unsupported","failed","cannot","can't","could not","error","unavailable","no capable","not found","invalid","device","memory","resource","session"
 ].iter().any(|s|e.contains(s)))
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn selection_is_deterministic_and_cpu_mode_never_tries_hardware(){
  let mut c=Config::default();assert_eq!(candidates(&c,Encoder::Nvenc),vec![Encoder::Nvenc,Encoder::Qsv,Encoder::Amf,Encoder::Cpu]);
  assert_eq!(candidates(&c,Encoder::Qsv),vec![Encoder::Qsv,Encoder::Nvenc,Encoder::Amf,Encoder::Cpu]);
  assert_eq!(candidates(&c,Encoder::Amf),vec![Encoder::Amf,Encoder::Nvenc,Encoder::Qsv,Encoder::Cpu]);
  c.encoder="cpu".into();assert_eq!(candidates(&c,Encoder::Nvenc),vec![Encoder::Cpu]);
 }
 #[test]fn hardware_failures_are_distinct_from_media_and_delivery_errors(){
  for error in ["[h264_qsv] Error initializing an MFX session", "[h264_amf] AMF failed to initialise", "[h264_nvenc] No capable devices found", "The video encoder did not start within 30 seconds."]{assert!(hardware_error(error),"{error}");}
  for error in ["Recording cancelled.","Invalid data found when processing input","Permission denied","No such filter: overlay","Error opening input qsv-recording.mp4: Invalid data","Error opening output nvenc.mp4: Permission denied","Recording export stopped making progress."]{assert!(!hardware_error(error),"{error}");}
 }
 #[test]fn all_quality_tiers_use_supported_pixel_formats_and_low_latency_capture(){
  for encoder in [Encoder::Nvenc,Encoder::Qsv,Encoder::Amf,Encoder::Cpu]{for quality in ["archive","high","balanced"]{
   let mut c=Config::default();c.quality=quality.into();let mut cmd=ffmpeg();encoder.settings(&mut cmd,&c,true);let args=cmd.get_args().map(|x|x.to_string_lossy().into_owned()).collect::<Vec<_>>();
   assert!(args.windows(2).any(|p|p==["-c:v",encoder.name()]));assert!(args.windows(2).any(|p|p==["-pix_fmt",if encoder==Encoder::Qsv{"nv12"}else{"yuv420p"}]));
   assert!(args.iter().any(|s|s==match quality{"archive"=>"12","balanced"=>"23",_=>"16"}));
  }}
 }
}

#[cfg(test)]mod fallback_tests{
 use super::*;
 #[test]fn failed_probe_and_driver_failure_fall_through_to_the_next_encoder(){
  let c=Config::default();let control=export::Control::background(Arc::new(AtomicBool::new(false)));let mut attempted=vec![];
  let result=export_with_fallback(&c,Encoder::Nvenc,&control,|e|e!=Encoder::Nvenc,|e|{attempted.push(e);if e==Encoder::Qsv{Err("[h264_qsv] Error initializing MFX session".into())}else{Ok(e)}}).unwrap();
  assert_eq!(result,Encoder::Amf);assert_eq!(attempted,vec![Encoder::Qsv,Encoder::Amf]);
  let result=export_with_fallback(&c,Encoder::Nvenc,&control,|_|true,|e|if e.hardware(){Err(format!("[{}] device unavailable",e.name()))}else{Ok(e)}).unwrap();assert_eq!(result,Encoder::Cpu);
 }
 #[test]fn corrupt_input_and_cancellation_do_not_restart_exports(){
  let c=Config::default();let cancel=Arc::new(AtomicBool::new(false));let control=export::Control::background(cancel.clone());let mut count=0;
  assert!(export_with_fallback::<()>(&c,Encoder::Nvenc,&control,|_|true,|_|{count+=1;Err("Invalid data found when processing input".into())}).is_err());assert_eq!(count,1);
  count=0;assert!(export_with_fallback::<()>(&c,Encoder::Nvenc,&control,|_|{cancel.store(true,Ordering::Relaxed);false},|_|{count+=1;Ok(())}).unwrap_err().contains("cancelled"));assert_eq!(count,0);
 }
 #[test]fn export_probes_include_frame_padding_after_resolution_scaling(){
  let mut c=Config::default();assert_eq!(output_size(&c,1920,1080),(1920,1080));c.frame="mac".into();assert_eq!(output_size(&c,1920,1080),(2112,1312));
  c.resolution="720p".into();assert_eq!(output_size(&c,1920,1080),(1408,888));
 }
}
