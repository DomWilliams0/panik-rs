use backtrace::Backtrace;

use parking_lot::Mutex;
use std::borrow::Cow;
use std::fmt::Debug;
use std::panic::{PanicInfo, UnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::ThreadId;

// TODO parking_lot feature
// TODO log/slog/neither as feature
// TODO backtrace count configurable 0+ with test

lazy_static::lazy_static! {
    static ref HAS_PANICKED: AtomicBool = AtomicBool::default();
    static ref PANICS: Mutex<Vec<Panic>> = Mutex::new(Vec::new());
}

#[derive(Debug, Clone)]
pub struct Panic {
    pub message: String,
    pub thread_id: ThreadId,
    pub thread: String,
    pub backtrace: Backtrace,
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
    });
}

/// None on error
pub fn run_and_handle_panics<R: Debug>(do_me: impl FnOnce() -> R + UnwindSafe) -> Option<R> {
    std::panic::set_hook(Box::new(|panic| {
        register_panic(panic);
    }));

    let result = std::panic::catch_unwind(|| do_me());

    let all_panics = panics();

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

    debug_assert!(!all_panics.is_empty());

    #[cfg(feature = "use-log")]
    log::info!("{count} threads panicked", count = all_panics.len());

    #[cfg(all(not(feature = "use-log"), not(feature = "use-slog")))]
    eprintln!("{count} threads panicked", count = all_panics.len());

    #[cfg(feature = "use-slog")]
    slog_scope::crit!("{count} threads panicked", count = all_panics.len());

    const BACKTRACE_RESOLUTION_LIMIT: usize = 8;
    for (
        i,
        Panic {
            message,
            thread,
            mut backtrace,
            ..
        },
    ) in all_panics.into_iter().enumerate()
    {
        if i == BACKTRACE_RESOLUTION_LIMIT {
            #[cfg(feature = "use-log")]
            log::warn!(
                "handling more than {limit} panics, no longer resolving backtraces",
                limit = BACKTRACE_RESOLUTION_LIMIT
            );

            #[cfg(all(not(feature = "use-log"), not(feature = "use-slog")))]
            eprintln!(
                "handling more than {limit} panics, no longer resolving backtraces",
                limit = BACKTRACE_RESOLUTION_LIMIT
            );

            #[cfg(feature = "use-slog")]
            slog_scope::warn!(
                "handling more than {limit} panics, no longer resolving backtraces",
                limit = BACKTRACE_RESOLUTION_LIMIT
            );
        }
        if i < BACKTRACE_RESOLUTION_LIMIT {
            backtrace.resolve();
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
