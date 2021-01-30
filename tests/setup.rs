pub fn init() {
    #[cfg(feature = "use-log")]
    env_logger::init();

    #[cfg(feature = "use-slog")]
    {
        use slog::{slog_o, Drain};

        let plain = slog_term::PlainSyncDecorator::new(std::io::stderr());
        let log = slog::Logger::root(slog_term::FullFormat::new(plain).build().fuse(), slog_o!());
        let guard = slog_scope::set_global_logger(log);
        std::mem::forget(guard);
    }
}
