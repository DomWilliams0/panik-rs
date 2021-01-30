mod setup;

#[test]
fn max_backtraces() {
    setup::init();

    panic::set_maximum_backtrace_resolutions(3);

    let result = panic::run_and_handle_panics(move || {
        for _ in 0..5 {
            let thread = std::thread::spawn(|| panic!("uh oh"));
            let _ = thread.join();
        }
    });

    assert!(result.is_none());
    assert!(panic::has_panicked());

    let panics = panic::panics();
    assert_eq!(panics.len(), 5);

    let resolved_count = panics.iter().filter(|p| p.is_backtrace_resolved()).count();
    let unresolved_count = panics.iter().filter(|p| !p.is_backtrace_resolved()).count();

    assert_eq!(resolved_count, 3);
    assert_eq!(unresolved_count, 2);
}
