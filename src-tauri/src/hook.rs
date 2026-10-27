use std::sync::mpsc::Sender;
use std::sync::OnceLock;

use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, SetWindowsHookExW, TranslateMessage, HC_ACTION, KBDLLHOOKSTRUCT,
    MSG, MSLLHOOKSTRUCT, WH_KEYBOARD_LL, WH_MOUSE_LL, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE,
};

pub enum Ev {
    /// x, y, and whether Ctrl was held
    Down(i32, i32, bool),
    Up,
    Move(i32, i32),
    Esc,
}

static TX: OnceLock<Sender<Ev>> = OnceLock::new();

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        if let Some(tx) = TX.get() {
            let info = &*(lparam.0 as *const MSLLHOOKSTRUCT);
            let ev = match wparam.0 as u32 {
                WM_LBUTTONDOWN => {
                    // Ctrl + click opens Halo; adding Shift (Ctrl + Shift + click) keeps Explorer's normal multi-select.
                    let ctrl = (GetAsyncKeyState(0x11) as u16 & 0x8000) != 0 && (GetAsyncKeyState(0x10) as u16 & 0x8000) == 0;
                    Some(Ev::Down(info.pt.x, info.pt.y, ctrl))
                }
                WM_LBUTTONUP => Some(Ev::Up),
                WM_MOUSEMOVE => Some(Ev::Move(info.pt.x, info.pt.y)),
                _ => None,
            };
            if let Some(ev) = ev {
                let _ = tx.send(ev);
            }
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

unsafe extern "system" fn key_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && wparam.0 as u32 == WM_KEYDOWN {
        let info = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        if info.vkCode == 0x1B {
            if let Some(tx) = TX.get() {
                let _ = tx.send(Ev::Esc);
            }
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

/// Installs global low-level mouse and keyboard hooks on a dedicated thread.
/// The hooks only observe; every event is passed on to the system untouched.
pub fn start(tx: Sender<Ev>) {
    let _ = TX.set(tx);
    std::thread::spawn(|| unsafe {
        let _mouse = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), None, 0);
        let _keys = SetWindowsHookExW(WH_KEYBOARD_LL, Some(key_proc), None, 0);
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    });
}
