mod setup;

#[test]
fn multiple_threads() {
    setup::init();

    let result = panic::run_and_handle_panics(move || {
        for _ in 0..4 {
            let thread = std::thread::spawn(|| panic!("uh oh"));
            let _ = thread.join();
        }

        panic!("me too")
    });

    assert!(result.is_none());
    assert!(panic::has_panicked());

    let mut panics = panic::panics();
    assert_eq!(panics.len(), 5);

    let main_idx = panics
        .iter()
        .enumerate()
        .find_map(|(idx, p)| {
            if p.thread_id == std::thread::current().id() {
                Some(idx)
            } else {
                None
            }
        })
        .unwrap();

    let main = panics.remove(main_idx);
    assert_eq!(main.message, "me too");
    assert!(panics.iter().all(|p| p.message == "uh oh"));
}
