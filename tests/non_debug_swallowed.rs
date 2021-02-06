mod setup;

#[test]
fn non_debug_swallowed() {
    setup::init();

    struct MyOpaque(i32);

    let result = panic::run_and_handle_panics_no_debug(|| {
        let _ = std::thread::spawn(|| panic!("oh no")).join();
        MyOpaque(100)
    });

    assert!(result.is_none());
    assert!(panic::has_panicked());
}
