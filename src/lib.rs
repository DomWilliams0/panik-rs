//! This crate enabled **application-wide panic handling**, whereby panics occurring in any thread
//! are detected, stored and can be queried to trigger an early application exit.
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
//!     },
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
//! * `use-slog`: log panics with the `slog-scope` crate
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

struct State {
    panics: Vec<Panic>,
    backtrace_resolution_limit: usize,
    is_running: bool,
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

struct GlobalStateGuard;

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

    #[cfg(feature = "use-log")]
    log::error!("handling panic on thread {}: '{}'", thread, message);

    #[cfg(feature = "use-stderr")]
    eprintln!("handling panic on thread {}: '{}'", thread, message);

    #[cfg(feature = "use-slog")]
    slog_scope::error!("handling panic"; "thread" => &thread, "message" => %message);

    let backtrace = Backtrace::new_unresolved();

    let mut state = state_mutex();
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

            #[cfg(feature = "use-log")]
            log::warn!(
                "panic occurred in another thread, swallowing unpanicked result: {}",
                swallowed
            );

            #[cfg(feature = "use-stderr")]
            eprintln!(
                "panic occurred in another thread, swallowing unpanicked result: {}",
                swallowed
            );

            #[cfg(feature = "use-slog")]
            slog_scope::warn!("panic occurred in another thread, swallowing unpanicked result"; "result" => %swallowed);
        }
        (Err(_), false) => {}
        (Err(_), true) => unreachable!(),
    };

    let backtrace_resolution_limit = state.backtrace_resolution_limit;
    let panics = &mut state.panics;
    debug_assert!(!panics.is_empty(), "panics vec should not be empty");

    #[cfg(feature = "use-log")]
    log::info!("{count} threads panicked", count = panics.len());

    #[cfg(all(not(feature = "use-log"), not(feature = "use-slog")))]
    eprintln!("{count} threads panicked", count = panics.len());

    #[cfg(feature = "use-slog")]
    slog_scope::crit!("{count} threads panicked", count = panics.len());

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

                #[cfg(feature = "use-slog")]
                slog_scope::warn!(
                    "handling more than {limit} panics, no longer resolving backtraces",
                    limit = backtrace_resolution_limit
                );
            }
            _ => {}
        };

        #[cfg(feature = "use-log")]
        log::error!(
            "panic on thread {:?}: {:?}\n{:?}",
            thread,
            message,
            backtrace
        );

        #[cfg(feature = "use-stderr")]
        eprintln!(
            "panic on thread {:?}: {:?}\n{:?}",
            thread, message, backtrace
        );

        #[cfg(feature = "use-slog")]
        slog_scope::crit!("panic";
        "backtrace" => ?backtrace,
        "message" => %message,
        "thread" => %thread,
        );
    }

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

// TODO add Builder pattern if there is any more config
/// Sets the limit on backtraces to resolve in the next call to [run_and_handle_panics] only.
///
/// The default of 8 is restored afterwards.
///
/// # Example
/// ```
/// panik::set_maximum_backtrace_resolutions(3);
/// let _ = panik::run_and_handle_panics(|| {
///     /* ... */
/// });
///
/// // limit is reverted to default
/// assert_eq!(panik::maximum_backtrace_resolutions(), 8);
///
/// ```
pub fn set_maximum_backtrace_resolutions(n: usize) {
    state_mutex().backtrace_resolution_limit = n;
}

/// Gets the backtrace resolution limit for the next call to [run_and_handle_panics].
pub fn maximum_backtrace_resolutions() -> usize {
    state_mutex().backtrace_resolution_limit
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
    }
}

impl Default for State {
    fn default() -> Self {
        State {
            panics: Vec::new(),
            backtrace_resolution_limit: DEFAULT_BACKTRACE_RESOLUTION_LIMIT,
            is_running: false,
        }
    }
}
