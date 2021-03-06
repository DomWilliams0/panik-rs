pub fn panik_builder() -> panik::Builder {
    #[cfg(feature = "use-log")]
    env_logger::builder()
        .is_test(true)
        .filter_level(log::LevelFilter::Debug)
        .init();

    #[cfg(feature = "use-slog")]
    {
        use slog::{slog_o, Drain};

        let plain = slog_term::PlainSyncDecorator::new(std::io::stderr());
        let log = slog::Logger::root(slog_term::FullFormat::new(plain).build().fuse(), slog_o!());
        let guard = slog_scope::set_global_logger(log);

        let builder = panik::Builder::default().slogger(slog_scope::logger());
        std::mem::forget(guard);

        return builder;
    }

    panik::Builder::default()
}
