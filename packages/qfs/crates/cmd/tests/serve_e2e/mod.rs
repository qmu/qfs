//! Shared subprocess support for the `qfs serve` end-to-end tests.
//!
//! One thing lives here: a **bounded readiness wait** over the daemon's stderr. Every serve
//! e2e scenario has the same shape — spawn the real binary, wait until it has reached some
//! observable point, signal it, then assert on the whole log — and the only honest way to
//! know it reached that point is to READ the line it prints there. A fixed `sleep` is a guess
//! about how long boot takes; on a loaded runner the guess is wrong and the signal lands too
//! early (that is what reddened `main` on PR #74's merge commit).
//!
//! The pump runs on its own thread so a child that never prints the awaited line cannot block
//! the test: the deadline belongs to the waiter, not to the read. A timeout fails loudly,
//! naming the line it waited for and quoting everything it did see. The consumed bytes stay in
//! the buffer, so the assertions that follow (`boot complete`, `entries=N`, …) still read the
//! full log — waiting costs no evidence.

// Test support: assertions may panic/expect/unwrap freely, and each integration-test binary
// that includes this module uses only the part of it that it needs.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, dead_code)]

use std::io::Read;
use std::process::{Child, ChildStderr, ExitStatus};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// How often the waiter re-reads the accumulating buffer. A poll interval inside a bounded
/// wait, never a guess about the child's speed.
const POLL: Duration = Duration::from_millis(5);

/// The line that means **`qfs serve` will take a shutdown signal gracefully**.
///
/// It is `server running` — logged inside `Runtime::run`, so it says the run loop was reached —
/// and it became a *true* readiness signal only once the daemon started arming its handlers
/// before boot (ticket `20260818132305`). Until then the line was printed before the listener
/// was installed, so waiting on it narrowed the window without closing it; a wait keyed on it
/// today is deterministic because the property it claims now holds strictly earlier than the
/// line itself.
pub const SERVE_READY: &str = "server running";

/// The line the daemon prints the instant its SIGINT/SIGTERM handlers are installed — before
/// boot, and therefore before [`SERVE_READY`]. A test that wants to signal *during* boot waits
/// for this one instead.
pub const SHUTDOWN_ARMED: &str = "shutdown signal armed";

/// The budget for a readiness wait. Generous by design: the timeout exists to turn a child that
/// never reaches the awaited line into a loud failure, not to police how fast a runner is.
pub const READINESS_TIMEOUT: Duration = Duration::from_secs(30);

/// The budget for a shutdown to complete once the signal has been delivered.
pub const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(10);

/// Wait for a signalled child to exit, returning its status — a bounded poll whose failure names
/// what it was waiting out.
pub fn wait_for_exit(child: &mut Child, after: &str) -> ExitStatus {
    let deadline = Instant::now() + SHUTDOWN_TIMEOUT;
    loop {
        if let Some(status) = child.try_wait().expect("try_wait") {
            return status;
        }
        assert!(
            Instant::now() < deadline,
            "`qfs serve` did not exit within {SHUTDOWN_TIMEOUT:?} after {after}"
        );
        std::thread::sleep(POLL);
    }
}

/// A live capture of a spawned `qfs serve`'s stderr, readable while the child is still running.
pub struct ServeLog {
    text: Arc<Mutex<String>>,
    eof: Arc<AtomicBool>,
    pump: Option<JoinHandle<()>>,
}

impl ServeLog {
    /// Start pumping `stderr` into an in-memory buffer on a background thread. Take the handle
    /// off the child first (`child.stderr.take()`), exactly as a `read_to_string` at the end
    /// would have.
    pub fn pump(mut stderr: ChildStderr) -> Self {
        let text = Arc::new(Mutex::new(String::new()));
        let eof = Arc::new(AtomicBool::new(false));
        let (sink, done) = (Arc::clone(&text), Arc::clone(&eof));
        let pump = std::thread::spawn(move || {
            let mut chunk = [0_u8; 4096];
            loop {
                match stderr.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => sink
                        .lock()
                        .expect("serve stderr buffer")
                        .push_str(&String::from_utf8_lossy(&chunk[..n])),
                }
            }
            done.store(true, Ordering::SeqCst);
        });
        Self {
            text,
            eof,
            pump: Some(pump),
        }
    }

    /// Everything the child has written so far.
    pub fn snapshot(&self) -> String {
        self.text.lock().expect("serve stderr buffer").clone()
    }

    /// Block until `needle` appears in the child's stderr, or fail loudly at `timeout`.
    ///
    /// Returns the log as of the moment the line appeared. Panicking is the point: a readiness
    /// wait that silently gave up would be the fixed `sleep` again, with extra steps.
    pub fn wait_for(&self, needle: &str, timeout: Duration) -> String {
        let deadline = Instant::now() + timeout;
        loop {
            let seen = self.snapshot();
            if seen.contains(needle) {
                return seen;
            }
            assert!(
                Instant::now() < deadline,
                "timed out after {timeout:?} waiting for `{needle}` on `qfs serve` stderr \
                 (child stderr {}):\n{seen}",
                if self.eof.load(Ordering::SeqCst) {
                    "closed"
                } else {
                    "still open"
                },
            );
            std::thread::sleep(POLL);
        }
    }

    /// Join the pump and return the complete log. Call it after the child has exited — the pipe
    /// closes then, so the reader thread ends on its own.
    pub fn finish(mut self) -> String {
        if let Some(pump) = self.pump.take() {
            let _ = pump.join();
        }
        self.snapshot()
    }
}
