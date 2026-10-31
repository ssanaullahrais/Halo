//! Bounded accessibility metadata and key activity. Never reads names, values or text.
use super::*;
use windows::{core::Interface,Win32::{System::Com::{CoInitializeEx,CoUninitialize,CoCreateInstance,COINIT_MULTITHREADED,CLSCTX_INPROC_SERVER},UI::Accessibility::*}};
pub(super) const TYPE:u8=1;pub(super) const ENTER:u8=2;pub(super) const ESCAPE:u8=4;pub(super) const TAB:u8=8;
static KEYS:std::sync::atomic::AtomicU8=std::sync::atomic::AtomicU8::new(0);
fn key_activity(key:u32,shortcut:bool)->u8{match key{0x0d=>ENTER,0x1b=>ESCAPE,0x09=>TAB,0x08|0x20|0x2e|0x30..=0x5a|0x60..=0x6f|0xba..=0xe2|0xe7 if !shortcut||[0x56,0x58,0x59,0x5a].contains(&key)=>TYPE,_=>0}}
pub(super) unsafe extern "system" fn keyboard_hook(code:i32,w:windows::Win32::Foundation::WPARAM,l:windows::Win32::Foundation::LPARAM)->windows::Win32::Foundation::LRESULT{
 keystrokes::note_keyboard(code>=0&&[WM_KEYDOWN,WM_SYSKEYDOWN].contains(&(w.0 as u32)));
 if code>=0&&[WM_KEYDOWN,WM_SYSKEYDOWN].contains(&(w.0 as u32)){let key=unsafe{&*(l.0 as *const KBDLLHOOKSTRUCT)}.vkCode;let shortcut=[0x11,0x12,0x5b,0x5c].into_iter().any(|k|unsafe{GetAsyncKeyState(k)}as u16&0x8000!=0);KEYS.fetch_or(key_activity(key,shortcut),Ordering::Relaxed);let event=unsafe{&*(l.0 as *const KBDLLHOOKSTRUCT)};keystrokes::hook(key,event.scanCode,event.flags.0);}
 unsafe{CallNextHookEx(None,code,w,l)}
}
pub(super) fn take_activity()->Option<Context>{let activity=KEYS.swap(0,Ordering::Relaxed);(activity!=0).then_some(Context{activity,..Default::default()})}
#[derive(Clone,Debug,PartialEq,Serialize,Deserialize)]pub(super) struct Region{pub kind:u8,pub x:i32,pub y:i32,pub w:u32,pub h:u32,#[serde(default)]pub multiline:bool}
#[derive(Clone,Debug,Default,PartialEq,Serialize,Deserialize)]pub(super) struct Context{pub focus:Option<Region>,pub target:Option<Region>,#[serde(default)]pub activity:u8,#[serde(default)]pub view:u32}
#[derive(Default)]struct Snapshot{data:Context,updated:Option<Instant>}
pub(super) struct Sampler{stop:Arc<AtomicBool>,snapshot:Arc<Mutex<Snapshot>>,previous:Context,enabled:bool}
fn region(element:&IUIAutomationElement)->Option<Region>{unsafe{let kind=element.CurrentControlType().ok()?;let kind=if kind==UIA_EditControlTypeId||kind==UIA_ComboBoxControlTypeId{1}else if kind==UIA_ButtonControlTypeId||kind==UIA_MenuItemControlTypeId||kind==UIA_TabItemControlTypeId{2}else if kind==UIA_HyperlinkControlTypeId{3}else{let role=element.CurrentAriaRole().map(|s|s.to_string()).unwrap_or_default();let modal=kind==UIA_WindowControlTypeId&&element.GetCurrentPatternAs::<IUIAutomationWindowPattern>(UIA_WindowPatternId).and_then(|p|p.CurrentIsModal()).map(|b|b.as_bool()).unwrap_or(false);if role=="dialog"||role=="alertdialog"||modal{4}else{return None;}};let r=element.CurrentBoundingRectangle().ok()?;let(w,h)=(r.right-r.left,r.bottom-r.top);if w<=0||h<=0{return None;}let multiline=element.CurrentAriaProperties().map(|s|s.to_string().contains("multiline=true")).unwrap_or(false);Some(Region{kind,x:r.left,y:r.top,w:w as u32,h:h as u32,multiline})}}
fn ancestors(ui:&IUIAutomation,element:&IUIAutomationElement)->(Option<Region>,Option<IUIAutomationElement>){
 let walker=unsafe{ui.ControlViewWalker()}.ok();let mut current=Some(element.clone());let mut focus=None;let mut document=None;
 for _ in 0..12{let Some(e)=current else{break;};if let Some(mut r)=region(&e){if focus.is_none()||r.kind==4{if r.kind==4{r.multiline=focus.as_ref().is_some_and(|f:&Region|f.multiline);}focus=Some(r);}}let kind=unsafe{e.CurrentControlType()}.ok();if kind==Some(UIA_DocumentControlTypeId){document=Some(e);break;}current=walker.as_ref().and_then(|w|unsafe{w.GetParentElement(&e)}.ok());} (focus,document)
}
fn target_region(ui:&IUIAutomation,element:&IUIAutomationElement)->Option<Region>{let walker=unsafe{ui.ControlViewWalker()}.ok()?;let mut e=element.clone();for _ in 0..10{if let Some(r)=region(&e){return Some(r);}e=unsafe{walker.GetParentElement(&e)}.ok()?;}None}
impl Sampler{
 pub(super) fn new(enabled:bool,target:&Target)->Self{
  KEYS.store(0,Ordering::Relaxed);let stop=Arc::new(AtomicBool::new(false));let snapshot=Arc::new(Mutex::new(Snapshot::default()));
  if enabled{let done=stop.clone();let output=snapshot.clone();let window=if target.mode=="window"{target.window}else{0};
   // Provider calls never run on the input hook, recording thread or UI thread.
   // Dropping a recording signals this worker without waiting for a slow provider.
   thread::spawn(move||{let initialized=unsafe{CoInitializeEx(None,COINIT_MULTITHREADED)}.is_ok();if !initialized{return;}let result=(||->windows::core::Result<()>{let ui:IUIAutomation=unsafe{CoCreateInstance(&CUIAutomation8,None,CLSCTX_INPROC_SERVER)}?;if let Ok(v)=ui.cast::<IUIAutomation2>(){unsafe{let _=v.SetConnectionTimeout(150);let _=v.SetTransactionTimeout(150);}}
    let(mut document,mut view,mut last_point)=(None::<IUIAutomationElement>,0u32,POINT{x:i32::MIN,y:i32::MIN});let mut hovered=None;
    while !done.load(Ordering::Relaxed){let mut point=POINT::default();unsafe{let _=GetCursorPos(&mut point);}let owner=if window!=0{unsafe{GetAncestor(HWND(window as usize as *mut _),GA_ROOTOWNER)}}else{HWND::default()};let allowed_focus=window==0||unsafe{GetAncestor(GetForegroundWindow(),GA_ROOTOWNER)}==owner;let focused=if allowed_focus{unsafe{ui.GetFocusedElement()}.ok()}else{None};let(mut focus,mut next_document)=(None,None);
     keystrokes::set_safe(if focused.as_ref().is_some_and(|e|unsafe{e.CurrentIsPassword()}.map(|p|!p.as_bool()).unwrap_or(false)){unsafe{GetForegroundWindow()}.0 as usize}else{0});
     if let Some(e)=focused{(focus,next_document)=ancestors(&ui,&e);focus=focus.filter(|r|r.kind==1||r.kind==4);}
     if let Some(next)=next_document{if document.as_ref().map(|old|unsafe{ui.CompareElements(old,&next)}.map(|b|!b.as_bool()).unwrap_or(true)).unwrap_or(true){view=view.wrapping_add(1);last_point=POINT{x:i32::MIN,y:i32::MIN};}document=Some(next);}
     if (point.x as i64-last_point.x as i64).abs()+(point.y as i64-last_point.y as i64).abs()>6{let allowed=window==0||unsafe{GetAncestor(WindowFromPoint(point),GA_ROOTOWNER)}==owner;hovered=if allowed{unsafe{ui.ElementFromPoint(point)}.ok().and_then(|e|target_region(&ui,&e))}else{None};last_point=point;}
     if let Ok(mut s)=output.lock(){*s=Snapshot{data:Context{focus,target:hovered.clone(),activity:0,view},updated:Some(Instant::now())};}
     for _ in 0..20{if done.load(Ordering::Relaxed){break;}thread::sleep(Duration::from_millis(10));}
    }Ok(())})();if let Err(e)=result{log_error(&format!("Camera focus metadata unavailable; using pointer targets: {e}"));}unsafe{CoUninitialize();}});
  }Self{stop,snapshot,previous:Context::default(),enabled}
 }
 pub(super) fn sample(&mut self,bounds:Rect,viewport:Rect,offset:(i32,i32))->Option<Context>{
  if !self.enabled{return None;}let activity=KEYS.swap(0,Ordering::Relaxed);let mut c=self.snapshot.try_lock().ok().filter(|s|s.updated.is_some_and(|t|t.elapsed()<Duration::from_secs(2))).map(|s|s.data.clone()).unwrap_or_default();
  let map=|r:Region|{let x=r.x-viewport.x+offset.0;let y=r.y-viewport.y+offset.1;let right=(x as i64+r.w as i64).min(bounds.w as i64);let bottom=(y as i64+r.h as i64).min(bounds.h as i64);let left=x.max(0);let top=y.max(0);if right<=left as i64||bottom<=top as i64{return None;}Some(Region{x:left,y:top,w:(right-left as i64)as u32,h:(bottom-top as i64)as u32,..r})};c.focus=c.focus.and_then(map);c.target=c.target.and_then(map);c.activity=activity;
  if c.activity==0&&c==self.previous{return None;}self.previous=c.clone();self.previous.activity=0;Some(c)
 }
}
impl Drop for Sampler{fn drop(&mut self){self.stop.store(true,Ordering::Relaxed);}}

#[cfg(test)]mod tests{use super::*;
 #[test]fn activity_categories_ignore_navigation_and_never_keep_key_codes(){assert_eq!(key_activity(0x41,false),TYPE);assert_eq!(key_activity(0x0d,false),ENTER);assert_eq!(key_activity(0x1b,false),ESCAPE);assert_eq!(key_activity(0x09,false),TAB);for key in [0x10,0x11,0x21,0x22,0x25,0x28,0x70]{assert_eq!(key_activity(key,false),0);}assert_eq!(key_activity(0x41,true),0);assert_eq!(key_activity(0x56,true),TYPE);let c=Context{activity:TYPE,..Default::default()};let json=serde_json::to_value(c).unwrap();assert_eq!(json.as_object().unwrap().keys().map(String::as_str).collect::<Vec<_>>(),vec!["activity","focus","target","view"]);}
}
