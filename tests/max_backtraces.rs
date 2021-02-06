mod setup;

#[test]
fn max_backtraces() {
    setup::init();

    panik::set_maximum_backtrace_resolutions(3);

    let result = panik::run_and_handle_panics(move || {
        for _ in 0..5 {
            let thread = std::thread::spawn(|| panic!("uh oh"));
            let _ = thread.join();
        }

        "epic"
    });

    assert!(result.is_none());
    assert!(panik::has_panicked());

    let panics = panik::panics();
    assert_eq!(panics.len(), 5);

    let resolved_count = panics.iter().filter(|p| p.is_backtrace_resolved()).count();
    let unresolved_count = panics.iter().filter(|p| !p.is_backtrace_resolved()).count();

    assert_eq!(resolved_count, 3);
    assert_eq!(unresolved_count, 2);
}
