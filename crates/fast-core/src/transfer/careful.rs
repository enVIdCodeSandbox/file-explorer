//! Reading from a drive that stops answering (an SD card or USB stick on Windows, most of the time).
//!
//! A read or a folder listing that the drive never answers holds its thread for ever, and with it the whole copy.
//! What is done about it here:
//!
//! * **Listing a folder** is done on a helper thread that is watched. When it goes quiet for too long, the
//!   call is cancelled (Windows: `CancelSynchronousIo`, which makes the drive driver give the request up) and
//!   tried once more; if the drive still does not answer, or the person says *Skip this folder*, the folder
//!   is given up and reported, and the rest of the job goes on.
//! * **Reading a file** is done through [`SourceReader`]: a read that gets no answer for a while is cancelled,
//!   the file is opened again in a different way (small sequential reads, then unbuffered sector-sized ones)
//!   and the copy goes on from the same byte.

use crate::activity::{self, Level};
use crossbeam_channel as chan;
use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// No answer to one read for this long counts as a stall.
pub const READ_STALL: Duration = Duration::from_secs(8);
/// Opening a file, or copying a small one in a single call, on a drive that may not answer: no answer for this long, plus
/// time for the size at a very slow 256 KB/s, and the request is given up.
pub fn call_limit(size: u64) -> Duration {
    Duration::from_secs(45 + size / (256 * 1024))
}
/// One read from a network share may get no answer for this long (a card is given `READ_STALL`).
pub const NETWORK_READ_STALL: Duration = Duration::from_secs(30);
/// A write (or the push-out of written data) with no answer for this long means the destination has stopped answering.
pub const WRITE_STALL: Duration = Duration::from_secs(60);
/// No answer from a folder listing for this long counts as a stall.
pub const LIST_STALL: Duration = Duration::from_secs(20);
/// Listing is slow enough to be worth telling the person about after this long.
const LIST_SLOW: Duration = Duration::from_secs(5);

// ---------------------------------------------------------------------------
// Copying by way of this computer's own disk
// ---------------------------------------------------------------------------

/// Whether a file goes to a temporary place on this computer's disk first, and from there to its destination.
/// Reading from a card or stick and writing to a network drive at the same moment lets either one stall the other;
/// two separate steps cannot.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    /// A card or stick to a network drive goes by way of the local disk; anything else is tried directly and,
    /// if that fails on such a drive, once more by way of the local disk.
    Auto,
    /// Every copy that involves a card, stick or network drive goes by way of the local disk.
    Always,
    /// Never.
    Never,
}

static STAGE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

pub fn set_stage(mode: Stage) {
    STAGE.store(mode as u8, Ordering::Relaxed);
}

pub fn stage_mode() -> Stage {
    match STAGE.load(Ordering::Relaxed) {
        1 => Stage::Always,
        2 => Stage::Never,
        _ => Stage::Auto,
    }
}

pub fn stage_from_setting(s: &str) -> Stage {
    match s {
        "always" => Stage::Always,
        "never" => Stage::Never,
        _ => Stage::Auto,
    }
}

/// A place for one staged file, on this computer's own disk.
pub fn stage_path(name: &std::ffi::OsStr) -> PathBuf {
    static N: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!("fast-stage-{}", std::process::id()));
    let _ = fs::create_dir_all(&dir);
    dir.join(format!("{}-{}", N.fetch_add(1, Ordering::Relaxed), name.to_string_lossy()))
}

/// Removes what a crashed run left behind (nothing is staged by another running copy of the app when this is called at start).
pub fn sweep_stage_dirs() {
    if let Ok(rd) = fs::read_dir(std::env::temp_dir()) {
        for e in rd.flatten() {
            if e.file_name().to_string_lossy().starts_with("fast-stage-") {
                let _ = fs::remove_dir_all(e.path());
            }
        }
    }
}

/// Failures that another route would not change.
pub fn worth_staging(e: &io::Error) -> bool {
    !matches!(e.kind(), io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied | io::ErrorKind::AlreadyExists | io::ErrorKind::Interrupted | io::ErrorKind::StorageFull | io::ErrorKind::InvalidInput)
}

// ---------------------------------------------------------------------------
// Cancelling a call that is stuck in the drive
// ---------------------------------------------------------------------------

#[cfg(windows)]
pub(crate) mod os {
    #[link(name = "kernel32")]
    extern "system" {
        fn CancelSynchronousIo(thread: isize) -> i32;
        fn GetCurrentThread() -> isize;
        fn GetCurrentProcess() -> isize;
        fn DuplicateHandle(from_process: isize, from: isize, to_process: isize, to: *mut isize, access: u32, inherit: i32, options: u32) -> i32;
        fn CloseHandle(handle: isize) -> i32;
    }
    const DUPLICATE_SAME_ACCESS: u32 = 2;

    /// A handle to a thread that another thread can use to cancel the input or output that thread is waiting for.
    pub struct ThreadHandle(isize);
    // SAFETY: a thread handle may be used and closed from any thread.
    unsafe impl Send for ThreadHandle {}
    unsafe impl Sync for ThreadHandle {}

    impl ThreadHandle {
        /// A handle to the calling thread (the thread's own pseudo-handle only means "me", so it is copied).
        pub fn current() -> Option<ThreadHandle> {
            let mut out = 0isize;
            // SAFETY: the pseudo-handles are valid for the calling thread and process; `out` is written on success.
            let ok = unsafe { DuplicateHandle(GetCurrentProcess(), GetCurrentThread(), GetCurrentProcess(), &mut out, 0, 0, DUPLICATE_SAME_ACCESS) };
            (ok != 0).then_some(ThreadHandle(out))
        }
        /// Ask the drive's driver to give up what the thread is waiting for. True when there was something to cancel.
        pub fn cancel_io(&self) -> bool {
            // SAFETY: the handle is a live thread handle owned by this value.
            unsafe { CancelSynchronousIo(self.0) != 0 }
        }
    }

    impl Drop for ThreadHandle {
        fn drop(&mut self) {
            // SAFETY: the handle was made by `DuplicateHandle` and is closed once.
            unsafe { CloseHandle(self.0) };
        }
    }
}

#[cfg(not(windows))]
pub(crate) mod os {
    /// Other systems cannot take a stuck request back: a stalled call is left behind on its own thread instead.
    pub struct ThreadHandle;
    impl ThreadHandle {
        pub fn current() -> Option<ThreadHandle> {
            None
        }
        pub fn cancel_io(&self) -> bool {
            false
        }
    }
}

use std::sync::{Arc, Mutex, Once};

struct Armed {
    id: u64,
    handle: Arc<os::ThreadHandle>,
    since: Instant,
    limit: Duration,
    fired: Arc<AtomicBool>,
    what: String,
}

fn armed() -> &'static Mutex<Vec<Armed>> {
    static A: Mutex<Vec<Armed>> = Mutex::new(Vec::new());
    &A
}

fn start_watcher() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let _ = std::thread::Builder::new().name("read-watch".into()).spawn(|| loop {
            std::thread::sleep(Duration::from_millis(250));
            let Ok(list) = armed().lock() else { continue };
            for a in list.iter() {
                if !a.fired.load(Ordering::Relaxed) && a.since.elapsed() >= a.limit {
                    a.fired.store(true, Ordering::Relaxed);
                    let cancelled = a.handle.cancel_io();
                    activity::note(Level::Warn, "read", "no answer from the drive", &format!("{} for {} s: {}", a.what, a.since.elapsed().as_secs(), if cancelled { "the request was cancelled" } else { "nothing to cancel" }));
                }
            }
        });
    });
}

thread_local! {
    static MY_HANDLE: std::cell::RefCell<Option<Arc<os::ThreadHandle>>> = const { std::cell::RefCell::new(None) };
}

/// Watches one call on this thread: if the drive has not answered within `limit`, the call is cancelled.
/// Where calls cannot be cancelled (not Windows) there is nothing to watch with and this is `None`.
pub struct ReadWatch {
    id: u64,
    fired: Arc<AtomicBool>,
}

pub fn watch_read(limit: Duration, what: &str) -> Option<ReadWatch> {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let handle = MY_HANDLE.with(|h| {
        let mut h = h.borrow_mut();
        if h.is_none() {
            *h = os::ThreadHandle::current().map(Arc::new);
        }
        h.clone()
    })?;
    start_watcher();
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let fired = Arc::new(AtomicBool::new(false));
    armed().lock().ok()?.push(Armed { id, handle, since: Instant::now(), limit, fired: fired.clone(), what: what.to_string() });
    Some(ReadWatch { id, fired })
}

impl ReadWatch {
    /// The watcher gave up on the call (and cancelled it).
    pub fn fired(&self) -> bool {
        self.fired.load(Ordering::Relaxed)
    }
}

impl Drop for ReadWatch {
    fn drop(&mut self) {
        if let Ok(mut list) = armed().lock() {
            list.retain(|a| a.id != self.id);
        }
    }
}

/// Runs one write to a drive that may stop answering. If it gets no answer for `WRITE_STALL`, the request is cancelled
/// where the system allows (Windows) and the failure says what happened, instead of the copy waiting for ever.
pub fn guarded_write<T>(on: bool, what: &str, call: impl FnOnce() -> io::Result<T>) -> io::Result<T> {
    let watch = if on { watch_read(WRITE_STALL, what) } else { None };
    let result = call();
    let fired = watch.as_ref().is_some_and(|w| w.fired());
    drop(watch);
    match result {
        Err(_) if fired => Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "The destination stopped answering (nothing for 60 seconds), so this file was not finished. Check that the drive or share is reachable, then use Retry failed items.",
        )),
        r => r,
    }
}

// ---------------------------------------------------------------------------
// Reading a file in the way that works
// ---------------------------------------------------------------------------

/// How a file is read, from the usual way to the most careful.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tier {
    /// Big reads through the system's cache.
    Normal,
    /// Small reads, told to the system as one front-to-back pass.
    Small,
    /// Small reads straight from the drive, in whole sectors, without the cache.
    Unbuffered,
}

const SECTOR: usize = 4096;
const SMALL: usize = 64 * 1024;
const UNBUFFERED: usize = 32 * 1024;

impl Tier {
    pub fn next(self) -> Option<Tier> {
        match self {
            Tier::Normal => Some(Tier::Small),
            Tier::Small => Some(Tier::Unbuffered),
            Tier::Unbuffered => None,
        }
    }
    /// How much to ask for at a time when the caller's buffer holds `room` bytes.
    pub fn chunk(self, room: usize) -> usize {
        match self {
            Tier::Normal => room,
            Tier::Small => room.min(SMALL),
            Tier::Unbuffered => (room.min(UNBUFFERED) / SECTOR * SECTOR).max(SECTOR),
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Tier::Normal => "normal",
            Tier::Small => "small sequential",
            Tier::Unbuffered => "unbuffered sector-sized",
        }
    }
}

/// A buffer whose start is on a sector boundary, which unbuffered reads require.
struct Aligned {
    raw: Vec<u8>,
    off: usize,
    len: usize,
}

impl Aligned {
    fn new(len: usize) -> Aligned {
        let raw = vec![0u8; len + SECTOR];
        let off = (SECTOR - raw.as_ptr() as usize % SECTOR) % SECTOR;
        Aligned { raw, off, len }
    }
    fn slice(&mut self) -> &mut [u8] {
        &mut self.raw[self.off..self.off + self.len]
    }
}

fn open_as(path: &Path, tier: Tier) -> io::Result<File> {
    let mut o = fs::OpenOptions::new();
    o.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        match tier {
            Tier::Normal => {}
            Tier::Small => {
                o.custom_flags(0x0800_0000); // FILE_FLAG_SEQUENTIAL_SCAN
            }
            Tier::Unbuffered => {
                o.custom_flags(0x2000_0000); // FILE_FLAG_NO_BUFFERING
            }
        }
    }
    let _ = tier;
    o.open(path)
}

/// A source file that is read through, and that changes how it reads when the drive stops answering.
pub struct SourceReader {
    path: PathBuf,
    file: File,
    /// Bytes handed out so far.
    pos: u64,
    tier: Tier,
    /// How long one read may get no answer.
    stall: Duration,
    scratch: Aligned,
    /// Bytes read ahead of `pos` by an unbuffered read that began before it, handed out first.
    carry: Vec<u8>,
}

impl SourceReader {
    /// `file` is the source already opened (read from its start).
    pub fn new(path: &Path, file: File) -> SourceReader {
        SourceReader::with_stall(path, file, READ_STALL)
    }

    /// As `new`, with how long one read may get no answer (a network share is given longer than a card).
    pub fn with_stall(path: &Path, file: File, stall: Duration) -> SourceReader {
        SourceReader { path: path.to_path_buf(), file, pos: 0, tier: Tier::Normal, stall, scratch: Aligned::new(UNBUFFERED), carry: Vec::new() }
    }

    pub fn tier(&self) -> Tier {
        self.tier
    }

    /// The drive did not answer: open the file again the next way and carry on from the byte after the last one given out.
    pub(crate) fn recover(&mut self) -> io::Result<()> {
        let Some(next) = self.tier.next() else {
            activity::note(Level::Error, "read", "gave up", &format!("{}: the drive did not answer in any way tried, at {} bytes", self.path.display(), self.pos));
            return Err(io::Error::new(io::ErrorKind::TimedOut, "The drive stopped answering and did not start again. Take it out and put it back in, then copy this file again."));
        };
        activity::note(
            Level::Warn,
            "read",
            "reading differently",
            &format!("{}: no answer at {} MB; changing from {} to {} reads", self.path.display(), self.pos / (1024 * 1024), self.tier.label(), next.label()),
        );
        self.tier = next;
        self.carry.clear();
        self.file = open_as(&self.path, next)?;
        self.seek_to_pos()
    }

    /// Put the file at `pos`. Unbuffered reads may only start on a sector, so the part of that sector before `pos`
    /// is read and thrown away and the part after it is kept to be handed out next.
    fn seek_to_pos(&mut self) -> io::Result<()> {
        if self.tier != Tier::Unbuffered {
            self.file.seek(SeekFrom::Start(self.pos))?;
            return Ok(());
        }
        let aligned = self.pos - self.pos % SECTOR as u64;
        self.file.seek(SeekFrom::Start(aligned))?;
        let skip = (self.pos - aligned) as usize;
        if skip > 0 {
            let want = SECTOR;
            let n = self.file.read(&mut self.scratch.slice()[..want])?;
            if n > skip {
                let rest = self.scratch.slice()[skip..n].to_vec();
                self.carry = rest;
            }
        }
        Ok(())
    }

    fn attempt(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self.tier {
            Tier::Unbuffered if buf.len() >= SECTOR => {
                let want = self.tier.chunk(buf.len());
                let n = self.file.read(&mut self.scratch.slice()[..want])?;
                buf[..n].copy_from_slice(&self.scratch.slice()[..n]);
                Ok(n)
            }
            // A buffer smaller than a sector cannot take an unbuffered read: it is read a sector at a time and handed out in parts.
            Tier::Unbuffered => {
                let n = self.file.read(&mut self.scratch.slice()[..SECTOR])?;
                let give = n.min(buf.len());
                buf[..give].copy_from_slice(&self.scratch.slice()[..give]);
                self.carry = self.scratch.slice()[give..n].to_vec();
                Ok(give)
            }
            tier => {
                let cap = tier.chunk(buf.len());
                self.file.read(&mut buf[..cap])
            }
        }
    }
}

impl Read for SourceReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        if !self.carry.is_empty() {
            let n = buf.len().min(self.carry.len());
            buf[..n].copy_from_slice(&self.carry[..n]);
            self.carry.drain(..n);
            self.pos += n as u64;
            return Ok(n);
        }
        loop {
            let began = Instant::now();
            let what = format!("reading {} at {} MB", self.path.display(), self.pos / (1024 * 1024));
            let watch = watch_read(self.stall, &what);
            let result = self.attempt(buf);
            let stalled = watch.as_ref().is_some_and(|w| w.fired());
            drop(watch);
            match result {
                Ok(n) => {
                    self.pos += n as u64;
                    if began.elapsed() >= Duration::from_secs(3) {
                        activity::note(Level::Warn, "read", "slow read", &format!("{what} took {} s", began.elapsed().as_secs()));
                    }
                    return Ok(n);
                }
                Err(e) if stalled || e.raw_os_error() == Some(995) => self.recover()?,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Listing a folder that may not answer
// ---------------------------------------------------------------------------

/// One thing found in a folder.
pub struct Listed {
    pub path: PathBuf,
    pub name: std::ffi::OsString,
    pub is_dir: bool,
    pub size: u64,
    /// The item could not be read.
    pub error: Option<io::Error>,
}

enum Msg {
    Started(Option<Arc<os::ThreadHandle>>),
    Opened(io::Result<()>),
    Chunk(Vec<Listed>),
    End,
}

fn listed(e: io::Result<fs::DirEntry>, parent: &Path) -> Listed {
    match e {
        Err(error) => Listed { path: parent.to_path_buf(), name: Default::default(), is_dir: false, size: 0, error: Some(error) },
        Ok(e) => {
            let path = e.path();
            let name = e.file_name();
            match e.file_type() {
                Err(error) => Listed { path, name, is_dir: false, size: 0, error: Some(error) },
                Ok(ft) if ft.is_dir() => Listed { path, name, is_dir: true, size: 0, error: None },
                Ok(_) => {
                    let size = e.metadata().map(|m| m.len()).unwrap_or(0);
                    Listed { path, name, is_dir: false, size, error: None }
                }
            }
        }
    }
}

/// Lists `path`, handing what is found to `sink` in batches as it arrives.
///
/// Gives up with `TimedOut` when the drive has not answered for `stall` (after one cancel and one more try where the
/// system allows that), and with `Interrupted` when `skip` says the person does not want to wait any longer or
/// `cancelled` says the job is over. `stalled` goes up by one for as long as the drive has been quiet for a while,
/// so that the job can say so.
pub fn list_dir(path: &Path, stall: Duration, skip: &dyn Fn() -> bool, cancelled: &dyn Fn() -> bool, stalled: &AtomicU64, sink: &mut dyn FnMut(Vec<Listed>)) -> io::Result<()> {
    let mut delivered = 0usize;
    for attempt in 0..2 {
        let (tx, rx) = chan::unbounded::<Msg>();
        let p = path.to_path_buf();
        std::thread::Builder::new()
            .name("list-folder".into())
            .spawn(move || {
                let handle = os::ThreadHandle::current().map(Arc::new);
                let _ = tx.send(Msg::Started(handle));
                let rd = match fs::read_dir(&p) {
                    Ok(rd) => rd,
                    Err(e) => {
                        let _ = tx.send(Msg::Opened(Err(e)));
                        return;
                    }
                };
                let _ = tx.send(Msg::Opened(Ok(())));
                let mut chunk: Vec<Listed> = Vec::new();
                for e in rd {
                    chunk.push(listed(e, &p));
                    if chunk.len() >= 256 && tx.send(Msg::Chunk(std::mem::take(&mut chunk))).is_err() {
                        return;
                    }
                }
                if !chunk.is_empty() {
                    let _ = tx.send(Msg::Chunk(chunk));
                }
                let _ = tx.send(Msg::End);
            })?;
        let mut handle: Option<Arc<os::ThreadHandle>> = None;
        let mut last = Instant::now();
        let mut counted = false;
        let leave = |counted: bool| {
            if counted {
                stalled.fetch_sub(1, Ordering::Relaxed);
            }
        };
        loop {
            if cancelled() {
                leave(counted);
                return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
            }
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(msg) => {
                    last = Instant::now();
                    if counted {
                        counted = false;
                        stalled.fetch_sub(1, Ordering::Relaxed);
                    }
                    match msg {
                        Msg::Started(h) => handle = h,
                        Msg::Opened(Err(e)) => return Err(e),
                        Msg::Opened(Ok(())) => {}
                        Msg::Chunk(v) => {
                            delivered += v.len();
                            sink(v);
                        }
                        Msg::End => return Ok(()),
                    }
                }
                Err(chan::RecvTimeoutError::Timeout) => {
                    let quiet = last.elapsed();
                    if quiet >= LIST_SLOW && !counted {
                        counted = true;
                        stalled.fetch_add(1, Ordering::Relaxed);
                        activity::note(Level::Warn, "folder", "slow to answer", &format!("{} has not answered for {} s", path.display(), quiet.as_secs()));
                    }
                    if skip() && quiet >= Duration::from_secs(3) {
                        leave(counted);
                        activity::note(Level::Warn, "folder", "skipped", &format!("{} was skipped at the person's request", path.display()));
                        return Err(io::Error::new(io::ErrorKind::Interrupted, "This folder was skipped because the drive did not answer. Use Retry failed items to try it again."));
                    }
                    if quiet >= stall {
                        let cancelled_io = handle.as_ref().is_some_and(|h| h.cancel_io());
                        activity::note(Level::Warn, "folder", "no answer", &format!("{} for {} s{}", path.display(), quiet.as_secs(), if cancelled_io { ": the request was cancelled" } else { "" }));
                        // Nothing has been handed over yet and a cancel went through: give the drive one more try.
                        if cancelled_io && delivered == 0 && attempt == 0 && rx.recv_timeout(Duration::from_secs(5)).is_ok() {
                            leave(counted);
                            break;
                        }
                        leave(counted);
                        return Err(io::Error::new(io::ErrorKind::TimedOut, "The drive did not answer when this folder was read, so it was skipped. Use Retry failed items to try it again."));
                    }
                }
                Err(chan::RecvTimeoutError::Disconnected) => {
                    leave(counted);
                    return Err(io::Error::other("the folder could not be read"));
                }
            }
        }
    }
    Err(io::Error::new(io::ErrorKind::TimedOut, "The drive did not answer when this folder was read."))
}

/// Runs one call to a drive (the first look at an item, whether its copy exists) on its own thread, and gives up on it
/// when the drive does not answer for `limit`: the request is cancelled where the system allows (Windows) and the
/// thread left behind where it does not. `skip` (the person's wish) ends the wait after 3 seconds. While waiting for a
/// while, `stalled` is up by one so that the job can say so and offer to skip.
pub fn bounded_io<T: Send + 'static>(
    limit: Duration,
    what: &str,
    skip: &dyn Fn() -> bool,
    cancelled: &dyn Fn() -> bool,
    stalled: &AtomicU64,
    f: impl FnOnce() -> io::Result<T> + Send + 'static,
) -> io::Result<T> {
    enum Got<T> {
        Handle(Option<Arc<os::ThreadHandle>>),
        Done(io::Result<T>),
    }
    let (tx, rx) = chan::unbounded::<Got<T>>();
    std::thread::Builder::new().name("drive-call".into()).spawn(move || {
        let _ = tx.send(Got::Handle(os::ThreadHandle::current().map(Arc::new)));
        let _ = tx.send(Got::Done(f()));
    })?;
    let began = Instant::now();
    let mut handle = None;
    let mut counted = false;
    let leave = |counted: bool| {
        if counted {
            stalled.fetch_sub(1, Ordering::Relaxed);
        }
    };
    loop {
        if cancelled() {
            leave(counted);
            return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
        }
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(Got::Handle(h)) => handle = h,
            Ok(Got::Done(r)) => {
                leave(counted);
                return r;
            }
            Err(chan::RecvTimeoutError::Timeout) => {
                let waited = began.elapsed();
                if waited >= LIST_SLOW && !counted {
                    counted = true;
                    stalled.fetch_add(1, Ordering::Relaxed);
                    activity::note(Level::Warn, "drive", "slow to answer", &format!("{what}: no answer for {} s", waited.as_secs()));
                }
                let skipped = skip() && waited >= Duration::from_secs(3);
                if skipped || waited >= limit {
                    let cancelled_io = handle.as_ref().is_some_and(|h| h.cancel_io());
                    activity::note(Level::Warn, "drive", if skipped { "skipped" } else { "no answer" }, &format!("{what}: {} s{}", waited.as_secs(), if cancelled_io { ", the request was cancelled" } else { ", left behind" }));
                    leave(counted);
                    return Err(if skipped {
                        io::Error::new(io::ErrorKind::Interrupted, "Skipped because the drive did not answer. Use Retry failed items to try it again.")
                    } else {
                        io::Error::new(io::ErrorKind::TimedOut, "The drive did not answer, so this was skipped. Use Retry failed items to try it again.")
                    });
                }
            }
            Err(chan::RecvTimeoutError::Disconnected) => {
                leave(counted);
                return Err(io::Error::other("the drive call ended without an answer"));
            }
        }
    }
}

/// `bounded_io` without a person to ask: for a single call that a stalled drive could hold.
pub fn bounded<T: Send + 'static>(limit: Duration, what: &str, f: impl FnOnce() -> io::Result<T> + Send + 'static) -> io::Result<T> {
    bounded_io(limit, what, &|| false, &|| false, &AtomicU64::new(0), f)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn call_limit_grows_with_size_and_network_reads_wait_longer() {
        assert_eq!(call_limit(0), Duration::from_secs(45));
        assert!(call_limit(1 << 30) > call_limit(1 << 20));
        assert!(NETWORK_READ_STALL > READ_STALL);
    }

    #[test]
    fn reader_with_longer_stall_reads_the_same_bytes() {
        use std::io::Read;
        let dir = std::env::temp_dir().join(format!("fast-stall-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.bin");
        let data: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
        std::fs::write(&path, &data).unwrap();
        let mut r = SourceReader::with_stall(&path, std::fs::File::open(&path).unwrap(), NETWORK_READ_STALL);
        let mut got = Vec::new();
        r.read_to_end(&mut got).unwrap();
        assert_eq!(got, data);
        let _ = std::fs::remove_dir_all(&dir);
    }

    use std::io::Write;

    fn pattern(len: usize) -> Vec<u8> {
        (0..len).map(|i| (i * 31 % 251) as u8).collect()
    }

    #[test]
    fn reads_get_smaller_and_more_careful_step_by_step_and_then_stop() {
        assert_eq!(Tier::Normal.next(), Some(Tier::Small));
        assert_eq!(Tier::Small.next(), Some(Tier::Unbuffered));
        assert_eq!(Tier::Unbuffered.next(), None);
        assert_eq!(Tier::Normal.chunk(8 << 20), 8 << 20);
        assert_eq!(Tier::Small.chunk(8 << 20), 64 * 1024);
        assert_eq!(Tier::Small.chunk(10_000), 10_000);
        assert_eq!(Tier::Unbuffered.chunk(8 << 20), 32 * 1024);
        assert_eq!(Tier::Unbuffered.chunk(10_000), 8192, "whole sectors only");
        assert_eq!(Tier::Unbuffered.chunk(100), SECTOR);
    }

    fn read_all(r: &mut SourceReader, buf_len: usize) -> Vec<u8> {
        let mut out = Vec::new();
        let mut buf = vec![0u8; buf_len];
        loop {
            match r.read(&mut buf).unwrap() {
                0 => return out,
                n => out.extend_from_slice(&buf[..n]),
            }
        }
    }

    #[test]
    fn the_whole_file_comes_through_however_often_the_way_of_reading_changes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("clip.bin");
        let data = pattern(3 * 1024 * 1024 + 777);
        std::fs::File::create(&path).unwrap().write_all(&data).unwrap();
        for stop_at in [0usize, 1, 4095, 4096, 10_001, 1_000_000, data.len() - 5] {
            let mut r = SourceReader::new(&path, File::open(&path).unwrap());
            let mut got = Vec::new();
            let mut buf = vec![0u8; 200_000];
            // Read up to the place where the drive "stalls", change the way of reading, and carry on.
            while got.len() < stop_at {
                let want = (stop_at - got.len()).min(buf.len());
                let n = r.read(&mut buf[..want]).unwrap();
                got.extend_from_slice(&buf[..n]);
            }
            r.recover().unwrap();
            assert_eq!(r.tier(), Tier::Small);
            let more = read_all(&mut r, 300_000);
            got.extend_from_slice(&more);
            assert_eq!(got.len(), data.len(), "stalled at {stop_at}");
            assert!(got == data, "bytes differ after a change at {stop_at}");
        }
    }

    #[test]
    fn the_most_careful_way_starts_in_the_middle_of_a_sector_and_loses_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.bin");
        let data = pattern(200_000);
        std::fs::write(&path, &data).unwrap();
        for stop_at in [1usize, 4097, 12_345, 99_999] {
            let mut r = SourceReader::new(&path, File::open(&path).unwrap());
            let mut got = vec![0u8; stop_at];
            r.read_exact(&mut got).unwrap();
            r.recover().unwrap();
            r.recover().unwrap();
            assert_eq!(r.tier(), Tier::Unbuffered);
            // Small buffers too: they are served from what the sector read gave.
            let more = read_all(&mut r, 1000);
            got.extend_from_slice(&more);
            assert!(got == data, "bytes differ with a stall at {stop_at}");
        }
    }

    #[test]
    fn when_every_way_has_been_tried_the_error_says_what_to_do() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.bin");
        std::fs::write(&path, pattern(5000)).unwrap();
        let mut r = SourceReader::new(&path, File::open(&path).unwrap());
        r.recover().unwrap();
        r.recover().unwrap();
        let e = r.recover().unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::TimedOut);
        assert!(e.to_string().contains("copy this file again"));
    }

    #[test]
    fn a_folder_is_listed_in_batches_with_what_could_not_be_read_marked() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..700 {
            std::fs::write(dir.path().join(format!("f{i}.txt")), "x").unwrap();
        }
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        let stalled = AtomicU64::new(0);
        let (mut files, mut dirs, mut batches) = (0, 0, 0);
        list_dir(dir.path(), Duration::from_secs(5), &|| false, &|| false, &stalled, &mut |v| {
            batches += 1;
            for l in v {
                assert!(l.error.is_none());
                if l.is_dir {
                    dirs += 1;
                } else {
                    files += 1;
                    assert_eq!(l.size, 1);
                }
            }
        })
        .unwrap();
        assert_eq!((files, dirs), (700, 1));
        assert!(batches >= 3, "big folders arrive in batches");
        assert_eq!(stalled.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn a_folder_that_is_not_there_gives_the_system_s_own_error() {
        let stalled = AtomicU64::new(0);
        let e = list_dir(Path::new("/definitely/not/here"), Duration::from_secs(5), &|| false, &|| false, &stalled, &mut |_| {}).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn staging_is_chosen_by_the_setting_and_skips_failures_a_second_route_cannot_fix() {
        assert_eq!(stage_from_setting("always"), Stage::Always);
        assert_eq!(stage_from_setting("never"), Stage::Never);
        assert_eq!(stage_from_setting("auto"), Stage::Auto);
        assert_eq!(stage_from_setting("anything"), Stage::Auto);
        assert!(worth_staging(&io::Error::new(io::ErrorKind::TimedOut, "x")));
        assert!(worth_staging(&io::Error::other("x")));
        assert!(!worth_staging(&io::Error::new(io::ErrorKind::PermissionDenied, "x")));
        assert!(!worth_staging(&io::Error::new(io::ErrorKind::NotFound, "x")));
        let a = stage_path(std::ffi::OsStr::new("a.bin"));
        let b = stage_path(std::ffi::OsStr::new("a.bin"));
        assert_ne!(a, b);
        assert!(a.parent().unwrap().exists());
        let _ = fs::remove_dir_all(a.parent().unwrap());
    }

    #[test]
    fn a_call_can_be_skipped_by_the_person_and_says_it_is_waiting() {
        use std::sync::atomic::AtomicBool;
        let wish = Arc::new(AtomicBool::new(false));
        let stalled = Arc::new(AtomicU64::new(0));
        let (w2, s2) = (wish.clone(), stalled.clone());
        let t = std::thread::spawn(move || {
            bounded_io(Duration::from_secs(60), "test", &|| w2.load(Ordering::Relaxed), &|| false, &s2, || -> io::Result<()> {
                std::thread::sleep(Duration::from_secs(10));
                Ok(())
            })
        });
        std::thread::sleep(Duration::from_millis(5600));
        assert_eq!(stalled.load(Ordering::Relaxed), 1, "after a few seconds the job is told");
        wish.store(true, Ordering::Relaxed);
        let e = t.join().unwrap().unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::Interrupted);
        assert_eq!(stalled.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn a_guarded_write_passes_results_and_errors_through_when_nothing_stalls() {
        assert_eq!(guarded_write(true, "test", || Ok(5)).unwrap(), 5);
        let e = guarded_write(true, "test", || -> io::Result<()> { Err(io::Error::new(io::ErrorKind::BrokenPipe, "gone")) }).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::BrokenPipe);
        assert_eq!(guarded_write(false, "test", || Ok(1)).unwrap(), 1);
    }

    #[test]
    fn a_call_that_never_returns_is_given_up_on() {
        let t = Instant::now();
        let e = bounded(Duration::from_millis(200), "test", || -> io::Result<()> {
            std::thread::sleep(Duration::from_secs(3));
            Ok(())
        })
        .unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::TimedOut);
        assert!(t.elapsed() < Duration::from_secs(2));
        assert_eq!(bounded(Duration::from_secs(5), "test", || Ok(7)).unwrap(), 7);
    }
}
