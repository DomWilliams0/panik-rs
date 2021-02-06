mod setup;

#[test]
fn max_backtraces() {
    let result = setup::panik_builder()
        .backtrace_resolution_limit(3)
        .run_and_handle_panics(move || {
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
