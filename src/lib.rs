//! This crate enables **application-wide panic handling**, whereby panics occurring in any thread
//! are captured and stored, and can later be queried to trigger an early application exit.
//!
//! This goes against the standard panic behaviour where a panic is isolated to the thread that
//! caused it. This library introduces the condition that *any panic in any thread is an error*
//! and the application cannot continue or recover.
//!
//! # Use case
//!
//! The main use case for this crate is when a thread spawns some threads to do work, and blocks on
//! their completion. If a worker thread panics before the result is posted, the waiting thread might get stuck in
//! a blocking call to `recv`, unless it specifically plans and checks for this error case (e.g. poisoned
//! mutex, disconnected mpsc sender).
//!
//! In a large application with thread pools and lots of types of work being posted to it all over
//! the place (like a game engine), it can be hard to handle every panic case properly. Using
//! this library allows the main thread to poll for panics in its core game loop and exit
//! gracefully, rather than soldiering on without its audio/rendering/AI/worker threads.
//!
//!
//! An example that doesn't use panic detection and hangs forever:
//! ```no_run
//! let (tx, rx) = std::sync::mpsc::channel();
//! let worker = std::thread::spawn(move || {
//!     // hopefully do some work...
//!     // tx.send(5).unwrap();
//!
//!     // ...or panic and hold up the main thread forever
//!     todo!()
//! });
//!
//! let result: i32 = rx.recv().expect("recv failed"); // blocks forever
//! println!("result: {}", result);
//! ```
//!
//! The same example detecting and handling panics and exiting gracefully:
//! ```should_panic
//! # use std::time::Duration;
//! let application_result = panik::run_and_handle_panics(|| {
//!     let (tx, rx) = std::sync::mpsc::channel();
//!     let worker = std::thread::spawn(move || {
//!         // do some work...
//!         // tx.send(5).unwrap();
//!
//!         // ...or panic and hold up the main thread forever
//!         todo!()
//!     });
//!
//!     // periodically check if a panic has occurred
//!     let poll_freq = Duration::from_secs(5);
//!     while !panik::has_panicked() {
//!         # let poll_freq = Duration::from_secs(0);
//!         if let Ok(res) = rx.recv_timeout(poll_freq) {
//!             return res;
//!         }
//!     }
//!
//!     // return value is irrelevant here, the panic on the worker
//!     // thread will clobber this when `run_and_handle_panics`
//!     // returns None
//!     0
//! });
//!
//! match application_result {
//!     None => {
//!         eprintln!("something went wrong: {:?}", panik::panics());
//!         std::process::exit(1);
//!     }
//!     Some(result) => {
//!         println!("result: {}", result);
//!         std::process::exit(0);
//!     }
//! }
//! ```
//!
//! This looks pretty heavyweight, but this intentional - this library is meant for large
//! and heavyweight applications!
//!
//! # Features
//! * `use-stderr`: log panics to stderr
//! * `use-log`: log panics with the `log` crate
//! * `use-slog`: log panics with the `slog` crate (configured in [Builder])
//! * `use-parking-lot`: use `parking_lot::Mutex` instead of `std::sync::Mutex`

use backtrace::Backtrace;

use std::borrow::Cow;
use std::fmt::Debug;
use std::panic::{PanicInfo, UnwindSafe};
use std::thread::ThreadId;

use std::cmp::Ordering;
use std::ops::DerefMut;

#[cfg(feature = "use-parking-lot")]
use parking_lot::Mutex;

#[cfg(not(feature = "use-parking-lot"))]
use std::sync::Mutex;

const DEFAULT_BACKTRACE_RESOLUTION_LIMIT: usize = 8;

lazy_static::lazy_static! {
    static ref STATE: Mutex<State> = Mutex::new(State::default());
}

macro_rules! log_warn {
($state:expr, $($arg:tt)+) => {
        #[cfg(feature = "use-slog")]
        slog::warn!(&$state.slogger, $($arg)+);
        #[cfg(feature = "use-log")]
        log::warn!($($arg)+);
        #[cfg(feature = "use-stderr")]
        eprintln!($($arg)+);
    }
}

macro_rules! log_error {
($state:expr, $($arg:tt)+) => {
        #[cfg(feature = "use-slog")]
        slog::error!(&$state.slogger, $($arg)+);
        #[cfg(feature = "use-log")]
        log::error!($($arg)+);
        #[cfg(feature = "use-stderr")]
        eprintln!($($arg)+);
    }
}

macro_rules! log_crit {
($state:expr, $($arg:tt)+) => {
        #[cfg(feature = "use-slog")]
        slog::crit!(&$state.slogger, $($arg)+);
        #[cfg(feature = "use-log")]
        log::error!($($arg)+);
        #[cfg(feature = "use-stderr")]
        eprintln!($($arg)+);
    }
}

struct State {
    panics: Vec<Panic>,
    backtrace_resolution_limit: usize,
    is_running: bool,

    #[cfg(feature = "use-slog")]
    slogger: slog::Logger,
}

/// Describes a panic that has occurred.
#[derive(Debug, Clone)]
pub struct Panic {
    message: String,
    thread_id: ThreadId,
    thread: String,
    backtrace: Backtrace,
    backtrace_resolved: bool,
}

/// Builder for panic handling configuration.
#[derive(Clone)]
pub struct Builder {
    #[cfg(feature = "use-slog")]
    slogger: Option<slog::Logger>,

    backtrace_resolution_limit: usize,
}

struct GlobalStateGuard;

impl Builder {
    pub fn new() -> Self {
        Builder {
            #[cfg(feature = "use-slog")]
            slogger: None,

            backtrace_resolution_limit: DEFAULT_BACKTRACE_RESOLUTION_LIMIT,
        }
    }

    #[cfg(feature = "use-slog")]
    /// Sets the slog logger to log to.
    pub fn slogger(mut self, slogger: impl Into<slog::Logger>) -> Self {
        self.slogger = Some(slogger.into());
        self
    }

    /// Sets the limit on backtraces to resolve. Defaults to 8.
    ///
    /// Useful in the case where there are many threads panicking with the same reason, and it can
    /// take a long time to resolve them all.
    pub fn backtrace_resolution_limit(mut self, n: usize) -> Self {
        self.backtrace_resolution_limit = n;
        self
    }

    fn apply_settings(&mut self) {
        let mut state = state_mutex();

        #[cfg(feature = "use-slog")]
        {
            state.slogger = self.slogger.take().unwrap_or_else(default_slogger);
        }

        state.backtrace_resolution_limit = self.backtrace_resolution_limit;
    }

    /// See [run_and_handle_panics].
    pub fn run_and_handle_panics<R: Debug>(
        mut self,
        do_me: impl FnOnce() -> R + UnwindSafe,
    ) -> Option<R> {
        self.apply_settings();
        run_and_handle_panics(do_me)
    }

    /// See [run_and_handle_panics_no_debug].
    pub fn run_and_handle_panics_no_debug<R>(
        mut self,
        do_me: impl FnOnce() -> R + UnwindSafe,
    ) -> Option<R> {
        self.apply_settings();
        run_and_handle_panics_no_debug(do_me)
    }
}

impl Default for Builder {
    fn default() -> Self {
        Self::new()
    }
}

fn register_panic(panic: &PanicInfo) {
    let (thread, tid) = {
        let t = std::thread::current();
        let name = t.name().unwrap_or("<unnamed>");
        (format!("{:?} ({})", t.id(), name), t.id())
    };

    // TODO use panic.message() when it stabilises
    let message = panic
        .payload()
        .downcast_ref::<&str>()
        .map(|s| Cow::Borrowed(*s))
        .unwrap_or_else(|| Cow::from(format!("{}", panic)));

    let backtrace = Backtrace::new_unresolved();

    let mut state = state_mutex();
    log_error!(&state, "handling panic on thread {}: '{}'", thread, message);

    state.panics.push(Panic {
        message: message.into_owned(),
        thread_id: tid,
        thread,
        backtrace,
        backtrace_resolved: false,
    });
}

fn state_mutex() -> impl DerefMut<Target = State> {
    #[cfg(feature = "use-parking-lot")]
    return STATE.lock();

    #[cfg(not(feature = "use-parking-lot"))]
    STATE.lock().unwrap()
}

/// Identical to [run_and_handle_panics] except the return type doesn't need to be [Debug].
///
/// This only matters when logging a return value has been swallowed due to a different thread
/// panicking.
pub fn run_and_handle_panics_no_debug<R>(do_me: impl FnOnce() -> R + UnwindSafe) -> Option<R> {
    run_and_handle_panics_with_maybe_debug(do_me, |_| Cow::Borrowed("<unprintable>"))
}

/// Runs the given closure, catching any panics that occur on **all threads** while in the scope of
/// the closure.
///
/// See [Builder] for configuration.
///
/// The [Debug] bound on `R` is only for logging purposes (in the event a successful result is
/// swallowed by a panic on another thread) - see [run_and_handle_panics_no_debug] for an
/// unconstrained return value.
///
/// This function can be called multiple times **serially**, but cannot be nested.
///
/// # Return value
/// If any thread(s) panicked, `None` is returned and the offending [Panic]s are available in
/// [panics] and [has_panicked] until the next call to this function. Otherwise
/// if no panics occur, `Some(R)` is returned.
///
/// # Example
///
/// See the `tests/` directory for more examples.
///
/// ```
/// # fn main() {
/// let result = panik::run_and_handle_panics(|| panic!("oh no"));
/// assert!(result.is_none());
/// assert!(panik::has_panicked());
///
/// let panics = panik::panics();
/// assert_eq!(panics.len(), 1);
///
/// let panic = &panics[0];
/// assert_eq!(panic.thread_id(), std::thread::current().id());
/// assert_eq!(panic.message(), "oh no");
/// # }
/// ```
pub fn run_and_handle_panics<R: Debug>(do_me: impl FnOnce() -> R + UnwindSafe) -> Option<R> {
    run_and_handle_panics_with_maybe_debug(do_me, |res| Cow::Owned(format!("{:?}", res)))
}

fn run_and_handle_panics_with_maybe_debug<R>(
    do_me: impl FnOnce() -> R + UnwindSafe,
    format_swallowed: impl FnOnce(R) -> Cow<'static, str>,
) -> Option<R> {
    let _guard = GlobalStateGuard::init();

    let result = std::panic::catch_unwind(|| do_me());

    let mut state = state_mutex();
    match (result, state.panics.is_empty()) {
        (Ok(res), true) => {
            // no panics
            return Some(res);
        }
        (Ok(res), false) => {
            let swallowed = format_swallowed(res);

            log_warn!(
                &state,
                "panic occurred in another thread, swallowing unpanicked result: {}",
                swallowed
            );
        }
        (Err(_), false) => {}
        (Err(_), true) => unreachable!(),
    };

    log_error!(
        &state,
        "{count} threads panicked",
        count = state.panics.len()
    );

    let backtrace_resolution_limit = state.backtrace_resolution_limit;
    let mut panics = std::mem::take(&mut state.panics);
    debug_assert!(!panics.is_empty(), "panics vec should not be empty");

    for (
        i,
        Panic {
            message,
            thread,
            ref mut backtrace,
            backtrace_resolved,
            ..
        },
    ) in panics.iter_mut().enumerate()
    {
        match i.cmp(&backtrace_resolution_limit) {
            Ordering::Less => {
                backtrace.resolve();
                *backtrace_resolved = true;
            }
            Ordering::Equal => {
                #[cfg(feature = "use-log")]
                log::warn!(
                    "handling more than {limit} panics, no longer resolving backtraces",
                    limit = backtrace_resolution_limit
                );

                #[cfg(feature = "use-stderr")]
                eprintln!(
                    "handling more than {limit} panics, no longer resolving backtraces",
                    limit = backtrace_resolution_limit
                );
            }
            _ => {}
        };

        if *backtrace_resolved {
            log_crit!(
                &state,
                "panic on thread {:?}: {:?}\n{:?}",
                thread,
                message,
                backtrace
            );
        } else {
            // dont log empty backtrace
            log_crit!(&state, "panic on thread {:?}: {:?}", thread, message,);
        }
    }

    // put panics back
    let empty = std::mem::replace(&mut state.panics, panics);
    debug_assert!(empty.is_empty());
    std::mem::forget(empty);

    None
}

/// Gets a copy of all panics that have occurred since the last call to [run_and_handle_panics].
pub fn panics() -> Vec<Panic> {
    let state = state_mutex();
    state.panics.clone() // efficiency be damned we're dying
}

/// Whether any panic has occurred since the last call to [run_and_handle_panics].
pub fn has_panicked() -> bool {
    !state_mutex().panics.is_empty()
}

impl Panic {
    /// Whether the backtrace for this panic has been resolved.
    pub fn is_backtrace_resolved(&self) -> bool {
        self.backtrace_resolved
    }

    /// The panic message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// The thread that this panic occurred on.
    pub fn thread_id(&self) -> ThreadId {
        self.thread_id
    }

    /// A string describing the thread e.g. "ThreadId(12) (worker-thread)".
    pub fn thread_name(&self) -> &str {
        &self.thread
    }

    /// The backtrace for this panic.
    pub fn backtrace(&self) -> &Backtrace {
        &self.backtrace
    }
}

impl GlobalStateGuard {
    fn init() -> Self {
        let mut state = state_mutex();

        // prevent nesting
        if state.is_running {
            drop(state); // avoid poisoning mutex
            panic!("nested calls to panik::run_and_handle_panics are not supported")
        }
        state.panics.clear();
        state.is_running = true;

        std::panic::set_hook(Box::new(|panic| {
            register_panic(panic);
        }));

        Self
    }
}

impl Drop for GlobalStateGuard {
    fn drop(&mut self) {
        let _ = std::panic::take_hook();

        let mut state = state_mutex();
        state.backtrace_resolution_limit = DEFAULT_BACKTRACE_RESOLUTION_LIMIT;
        state.is_running = false;

        #[cfg(feature = "use-slog")]
        {
            state.slogger = default_slogger();
        }
    }
}

impl Default for State {
    fn default() -> Self {
        State {
            panics: Vec::new(),
            backtrace_resolution_limit: DEFAULT_BACKTRACE_RESOLUTION_LIMIT,
            is_running: false,

            #[cfg(feature = "use-slog")]
            slogger: default_slogger(),
        }
    }
}

#[cfg(feature = "use-slog")]
fn default_slogger() -> slog::Logger {
    use slog::Drain;
    slog::Logger::root(slog_stdlog::StdLog.fuse(), slog::o!())
}
