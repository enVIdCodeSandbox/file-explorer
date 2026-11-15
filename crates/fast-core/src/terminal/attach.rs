//! Shows one terminal session in a real terminal window: this program, run inside Terminal,
//! Windows Terminal, GNOME Terminal or whatever the system provides, connects to the host and
//! passes keys in and output out. Closing the window only detaches; the shell keeps running.

use super::client::Client;
use super::proto::*;
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;
use std::sync::Arc;
use std::time::Duration;

/// What the window gives us: bytes in and out, and its size.
trait Console: Send {
    fn write_out(&self, bytes: &[u8]);
    fn size(&self) -> (u16, u16);
}

#[cfg(unix)]
mod sys {
    use super::*;

    pub struct Raw {
        saved: Option<libc::termios>,
    }

    impl Raw {
        pub fn enter() -> Raw {
            // SAFETY: termios calls on the standard input with a zeroed struct we then fill.
            unsafe {
                if libc::isatty(0) == 0 {
                    return Raw { saved: None };
                }
                let mut t: libc::termios = std::mem::zeroed();
                if libc::tcgetattr(0, &mut t) != 0 {
                    return Raw { saved: None };
                }
                let saved = t;
                libc::cfmakeraw(&mut t);
                libc::tcsetattr(0, libc::TCSANOW, &t);
                Raw { saved: Some(saved) }
            }
        }
    }

    impl Drop for Raw {
        fn drop(&mut self) {
            if let Some(t) = self.saved {
                // SAFETY: restoring what tcgetattr gave us.
                unsafe { libc::tcsetattr(0, libc::TCSANOW, &t) };
            }
        }
    }

    pub struct Tty;
    impl Console for Tty {
        fn write_out(&self, bytes: &[u8]) {
            let mut out = std::io::stdout().lock();
            let _ = out.write_all(bytes);
            let _ = out.flush();
        }
        fn size(&self) -> (u16, u16) {
            // SAFETY: TIOCGWINSZ fills a winsize.
            unsafe {
                let mut ws: libc::winsize = std::mem::zeroed();
                if libc::ioctl(1, libc::TIOCGWINSZ, &mut ws) == 0 && ws.ws_col > 0 && ws.ws_row > 0 {
                    return (ws.ws_col, ws.ws_row);
                }
            }
            (80, 24)
        }
    }

    pub fn open() -> (Raw, Tty, Box<dyn Read + Send>) {
        (Raw::enter(), Tty, Box::new(std::io::stdin()))
    }
}

#[cfg(windows)]
mod sys {
    use super::*;
    use std::fs::OpenOptions;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::Console::{
        AllocConsole, GetConsoleMode, GetConsoleScreenBufferInfo, GetConsoleWindow, SetConsoleMode, CONSOLE_SCREEN_BUFFER_INFO, ENABLE_ECHO_INPUT, ENABLE_LINE_INPUT, ENABLE_PROCESSED_INPUT,
        ENABLE_PROCESSED_OUTPUT, ENABLE_VIRTUAL_TERMINAL_INPUT, ENABLE_VIRTUAL_TERMINAL_PROCESSING,
    };

    pub struct Raw {
        input: isize,
        output: isize,
        saved_in: u32,
        saved_out: u32,
    }

    impl Drop for Raw {
        fn drop(&mut self) {
            // SAFETY: the handles belong to files kept alive by `Tty` and the reader.
            unsafe {
                SetConsoleMode(self.input as _, self.saved_in);
                SetConsoleMode(self.output as _, self.saved_out);
            }
        }
    }

    pub struct Tty {
        out: std::fs::File,
    }

    impl Console for Tty {
        fn write_out(&self, bytes: &[u8]) {
            let mut f = &self.out;
            let _ = f.write_all(bytes);
            let _ = f.flush();
        }
        fn size(&self) -> (u16, u16) {
            // SAFETY: fills a plain struct from a console handle we own.
            unsafe {
                let mut info: CONSOLE_SCREEN_BUFFER_INFO = std::mem::zeroed();
                if GetConsoleScreenBufferInfo(self.out.as_raw_handle() as _, &mut info) != 0 {
                    let cols = (info.srWindow.Right - info.srWindow.Left + 1).max(2) as u16;
                    let rows = (info.srWindow.Bottom - info.srWindow.Top + 1).max(2) as u16;
                    return (cols, rows);
                }
            }
            (80, 24)
        }
    }

    pub fn open() -> (Raw, Tty, Box<dyn Read + Send>) {
        // This program is a windowed one, so it has no console of its own: make one. Windows opens
        // it in whichever terminal app the person has chosen (Windows Terminal by default on 11).
        // SAFETY: plain Win32 calls.
        unsafe {
            if GetConsoleWindow().is_null() {
                AllocConsole();
            }
        }
        let input = OpenOptions::new().read(true).write(true).open("CONIN$").expect("console input");
        let output = OpenOptions::new().read(true).write(true).open("CONOUT$").expect("console output");
        let (mut mi, mut mo) = (0u32, 0u32);
        // SAFETY: the handles come from the files just opened.
        let raw = unsafe {
            GetConsoleMode(input.as_raw_handle() as _, &mut mi);
            GetConsoleMode(output.as_raw_handle() as _, &mut mo);
            SetConsoleMode(input.as_raw_handle() as _, (mi & !(ENABLE_ECHO_INPUT | ENABLE_LINE_INPUT | ENABLE_PROCESSED_INPUT)) | ENABLE_VIRTUAL_TERMINAL_INPUT);
            SetConsoleMode(output.as_raw_handle() as _, mo | ENABLE_PROCESSED_OUTPUT | ENABLE_VIRTUAL_TERMINAL_PROCESSING);
            Raw { input: input.as_raw_handle() as isize, output: output.as_raw_handle() as isize, saved_in: mi, saved_out: mo }
        };
        (raw, Tty { out: output }, Box::new(input))
    }
}

#[cfg(not(any(unix, windows)))]
mod sys {
    use super::*;
    pub struct Raw;
    pub struct Tty;
    impl Console for Tty {
        fn write_out(&self, _: &[u8]) {}
        fn size(&self) -> (u16, u16) {
            (80, 24)
        }
    }
    pub fn open() -> (Raw, Tty, Box<dyn Read + Send>) {
        (Raw, Tty, Box::new(std::io::empty()))
    }
}

/// Attach to session `sid` of the host in `dir`. Returns the process exit code.
pub fn run(dir: &Path, sid: &str) -> i32 {
    let (raw, tty, mut input) = sys::open();
    let tty = Arc::new(tty);
    let ended = Arc::new(AtomicBool::new(false));
    let (done_tx, done_rx) = channel::<i32>();
    let out = tty.clone();
    let watch_sid = sid.to_string();
    let end_flag = ended.clone();
    let tx_exit = done_tx.clone();
    let tx_close = done_tx.clone();
    let client = match Client::connect(
        dir,
        move |e| match e.ev.as_str() {
            "out" if e.sid == watch_sid => {
                if let Some(Ok(bytes)) = e.data.as_deref().map(|d| B64.decode(d)) {
                    out.write_out(&bytes);
                }
            }
            "exit" if e.sid == watch_sid => {
                end_flag.store(true, Ordering::SeqCst);
                let _ = tx_exit.send(e.code.unwrap_or(0));
            }
            _ => {}
        },
        move || {
            let _ = tx_close.send(-2);
        },
    ) {
        Ok(c) => Arc::new(c),
        Err(e) => {
            tty.write_out(format!("Could not reach the terminal host: {e}\r\n").as_bytes());
            std::thread::sleep(Duration::from_secs(4));
            return 1;
        }
    };
    let (cols, rows) = tty.size();
    let _ = client.call(Request::Resize { sid: sid.into(), cols, rows });
    match client.call(Request::Snapshot { sid: sid.into() }) {
        Ok(v) => {
            if let Some(Ok(bytes)) = v.get("data").and_then(|d| d.as_str()).map(|d| B64.decode(d)) {
                // Start from a clean screen so the old output lines up with this window's size.
                tty.write_out(b"\x1b[2J\x1b[H");
                tty.write_out(&bytes);
            }
            if v.pointer("/info/alive").and_then(|a| a.as_bool()) == Some(false) {
                let code = v.pointer("/info/exit").and_then(|c| c.as_i64()).unwrap_or(0);
                tty.write_out(format!("\r\n[The process ended with code {code}. Press Enter to close.]\r\n").as_bytes());
                ended.store(true, Ordering::SeqCst);
            }
        }
        Err(e) => {
            tty.write_out(format!("{e}\r\n").as_bytes());
            std::thread::sleep(Duration::from_secs(4));
            return 1;
        }
    }

    // Keys to the shell.
    {
        let client = client.clone();
        let sid = sid.to_string();
        let ended = ended.clone();
        let done = done_tx.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match input.read(&mut buf) {
                    Ok(0) | Err(_) => {
                        let _ = done.send(0);
                        break;
                    }
                    Ok(n) => {
                        if ended.load(Ordering::SeqCst) {
                            // The shell is gone: any key closes the window.
                            let _ = done.send(0);
                            break;
                        }
                        let _ = client.send(Request::Input { sid: sid.clone(), data: B64.encode(&buf[..n]) });
                    }
                }
            }
        });
    }
    // The window being resized.
    {
        let client = client.clone();
        let tty = tty.clone();
        let sid = sid.to_string();
        std::thread::spawn(move || {
            let mut last = tty.size();
            loop {
                std::thread::sleep(Duration::from_millis(250));
                let now = tty.size();
                if now != last {
                    last = now;
                    if client.send(Request::Resize { sid: sid.clone(), cols: now.0, rows: now.1 }).is_err() {
                        break;
                    }
                }
            }
        });
    }
    let code = done_rx.recv().unwrap_or(0);
    if code == -2 {
        tty.write_out(b"\r\n[The terminal host stopped.]\r\n");
        std::thread::sleep(Duration::from_secs(2));
        drop(raw);
        return 1;
    }
    if !ended.load(Ordering::SeqCst) {
        drop(raw);
        return 0;
    }
    // The shell ended while we were attached: show that, and wait for a key.
    tty.write_out(format!("\r\n[The process ended with code {code}. Press any key to close.]\r\n").as_bytes());
    let _ = done_rx.recv_timeout(Duration::from_secs(600));
    drop(raw);
    0
}
