use backtrace::Backtrace;

use parking_lot::Mutex;
use std::borrow::Cow;
use std::fmt::Debug;
use std::panic::{PanicInfo, UnwindSafe};
use std::sync::atomic::Ordering::Relaxed;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread::ThreadId;

// TODO parking_lot feature

lazy_static::lazy_static! {
    static ref HAS_PANICKED: AtomicBool = AtomicBool::default();
    static ref PANICS: Mutex<Vec<Panic>> = Mutex::new(Vec::new());
    static ref BACKTRACE_RESOLUTION_LIMIT: AtomicUsize = AtomicUsize::new(8);
}

#[derive(Debug, Clone)]
pub struct Panic {
    message: String,
    thread_id: ThreadId,
    thread: String,
    backtrace: Backtrace,
    backtrace_resolved: bool,
}

pub fn panics() -> Vec<Panic> {
    let panics = PANICS.lock();
    panics.clone() // efficiency be damned we're dying
}

pub fn with_panics(do_this: impl FnOnce(&[Panic])) {
    let panics = PANICS.lock();
    do_this(&panics);
}

pub fn has_panicked() -> bool {
    HAS_PANICKED.load(Ordering::Relaxed)
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

    HAS_PANICKED.store(true, Ordering::Relaxed);

    let mut panics = PANICS.lock();
    panics.push(Panic {
        message: message.into_owned(),
        thread_id: tid,
        thread,
        backtrace,
        backtrace_resolved: false,
    });
}

pub fn set_maximum_backtrace_resolutions(n: usize) {
    BACKTRACE_RESOLUTION_LIMIT.store(n, Relaxed);
}

/// None on error
pub fn run_and_handle_panics<R: Debug>(do_me: impl FnOnce() -> R + UnwindSafe) -> Option<R> {
    std::panic::set_hook(Box::new(|panic| {
        register_panic(panic);
    }));

    let result = std::panic::catch_unwind(|| do_me());
    let mut all_panics = PANICS.lock();

    match (result, all_panics.is_empty()) {
        (Ok(res), true) => {
            // no panics
            return Some(res);
        }
        (Ok(res), false) => {
            #[cfg(feature = "use-log")]
            log::warn!(
                "panic occurred in another thread, swallowing unpanicked result: {:?}",
                res
            );

            #[cfg(all(not(feature = "use-log"), not(feature = "use-slog")))]
            eprintln!(
                "panic occurred in another thread, swallowing unpanicked result: {:?}",
                res
            );

            #[cfg(feature = "use-slog")]
            slog_scope::warn!("panic occurred in another thread, swallowing unpanicked result"; "result" => ?res);
        }
        (Err(_), false) => {}
        (Err(_), true) => unreachable!(),
    };

    debug_assert!(!all_panics.is_empty(), "panics vec should not be empty");

    #[cfg(feature = "use-log")]
    log::info!("{count} threads panicked", count = all_panics.len());

    #[cfg(all(not(feature = "use-log"), not(feature = "use-slog")))]
    eprintln!("{count} threads panicked", count = all_panics.len());

    #[cfg(feature = "use-slog")]
    slog_scope::crit!("{count} threads panicked", count = all_panics.len());

    let backtrace_resolution_limit = BACKTRACE_RESOLUTION_LIMIT.load(Relaxed);
    for (
        i,
        Panic {
            message,
            thread,
            ref mut backtrace,
            backtrace_resolved,
            ..
        },
    ) in all_panics.iter_mut().enumerate()
    {
        if i == backtrace_resolution_limit {
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
        if i < backtrace_resolution_limit {
            backtrace.resolve();
            *backtrace_resolved = true;
        }

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
        "message" => message,
        "thread" => thread,
        );
    }

    let _ = std::panic::take_hook();
    None
}

impl Panic {
    pub fn is_backtrace_resolved(&self) -> bool {
        self.backtrace_resolved
    }

    pub fn message(&self) -> &str {
        &self.message
    }
    pub fn thread_id(&self) -> ThreadId {
        self.thread_id
    }
    pub fn thread_name(&self) -> &str {
        &self.thread
    }
    pub fn backtrace(&self) -> &Backtrace {
        &self.backtrace
    }
}
