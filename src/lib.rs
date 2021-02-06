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
    has_panicked: bool,
    panics: Vec<Panic>,
    backtrace_resolution_limit: usize,
    nested_count: i8,
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

/// Gets all panics that have occurred in this program.
pub fn panics() -> Vec<Panic> {
    let state = state_mutex();
    state.panics.clone() // efficiency be damned we're dying
}

pub fn has_panicked() -> bool {
    state_mutex().has_panicked
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

    #[cfg(feature = "use-log")]
    log::error!("handling panic on thread {}: '{}'", thread, message);

    #[cfg(all(not(feature = "use-log"), not(feature = "use-slog")))]
    eprintln!("handling panic on thread {}: '{}'", thread, message);

    #[cfg(feature = "use-slog")]
    slog_scope::error!("handling panic"; "thread" => &thread, "message" => %message);

    let backtrace = Backtrace::new_unresolved();

    let mut state = state_mutex();
    state.has_panicked = true;
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

// TODO add Builder pattern if there is any more config
pub fn set_maximum_backtrace_resolutions(n: usize) {
    state_mutex().backtrace_resolution_limit = n;
}

/// Same as [run_and_handle_panics] but the return type doesn't implement [Debug]. This only
/// matters when logging that the return value has been swallowed due to a different thread
/// panicking.
pub fn run_and_handle_panics_no_debug<R>(do_me: impl FnOnce() -> R + UnwindSafe) -> Option<R> {
    run_and_handle_panics_with_maybe_debug(do_me, |_| Cow::Borrowed("<unprintable>"))
}

/// If the return type doesn't implement [Debug], use [run_and_handle_panics_no_debug] instead.
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

            #[cfg(all(not(feature = "use-log"), not(feature = "use-slog")))]
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
    eprintln!("{count} threads panicked", count = state.len());

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

                #[cfg(all(not(feature = "use-log"), not(feature = "use-slog")))]
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

        #[cfg(all(not(feature = "use-log"), not(feature = "use-slog")))]
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

impl Panic {
    /// Whether the backtrace for this panic has been resolved
    pub fn is_backtrace_resolved(&self) -> bool {
        self.backtrace_resolved
    }

    /// The panic message
    pub fn message(&self) -> &str {
        &self.message
    }

    /// The thread that this panic occurred on
    pub fn thread_id(&self) -> ThreadId {
        self.thread_id
    }

    /// A string describing the thread e.g. "ThreadId(12) (worker-thread)"
    pub fn thread_name(&self) -> &str {
        &self.thread
    }

    /// The backtrace for this panic
    pub fn backtrace(&self) -> &Backtrace {
        &self.backtrace
    }
}

impl GlobalStateGuard {
    fn init() -> Self {
        let mut state = state_mutex();

        // prevent nesting
        if state.nested_count != 0 {
            drop(state); // prevent deadlock in panic handler
            panic!("nested calls to panic::run_and_handle_panics are not supported")
        }
        state.panics.clear();
        state.has_panicked = false;
        state.nested_count += 1;

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
        state.nested_count -= 1;
    }
}

impl Default for State {
    fn default() -> Self {
        State {
            has_panicked: false,
            panics: Vec::new(),
            backtrace_resolution_limit: DEFAULT_BACKTRACE_RESOLUTION_LIMIT,
            nested_count: 0,
        }
    }
}