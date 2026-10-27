use std::io::{BufRead, BufReader, Write};
use std::os::windows::process::CommandExt;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::time::Duration;

const NO_WINDOW: u32 = 0x0800_0000;

/// A long-running PowerShell helper that answers "what is selected in Explorer / on the desktop?".
/// Keeping it alive avoids paying the PowerShell start-up cost on every hold.
pub struct Selector {
    stdin: ChildStdin,
    rx: Receiver<String>,
    _child: Child,
}

impl Selector {
    pub fn start() -> Result<Self, String> {
        let script = std::env::temp_dir().join("halo-selection.ps1");
        std::fs::write(&script, include_str!("selection.ps1")).map_err(|e| e.to_string())?;

        let mut child = Command::new("powershell.exe")
            .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(&script)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .creation_flags(NO_WINDOW)
            .spawn()
            .map_err(|e| e.to_string())?;

        let stdin = child.stdin.take().ok_or("no stdin")?;
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        Ok(Self { stdin, rx, _child: child })
    }

    /// Paths selected at the given screen point. Pass `-1, -1` to skip the "is the pointer on an item" check.
    pub fn query(&mut self, x: i32, y: i32) -> Vec<String> {
        while self.rx.try_recv().is_ok() {}
        if writeln!(self.stdin, "sel {x} {y}").and_then(|_| self.stdin.flush()).is_err() {
            return vec![];
        }
        loop {
            match self.rx.recv_timeout(Duration::from_secs(6)) {
                Ok(line) if line.starts_with('[') => return serde_json::from_str(&line).unwrap_or_default(),
                Ok(_) => continue,
                Err(_) => return vec![],
            }
        }
    }
}
