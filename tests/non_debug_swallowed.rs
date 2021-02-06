mod setup;

#[test]
fn non_debug_swallowed() {
    struct MyOpaque(i32);

    let result = setup::panik_builder().run_and_handle_panics_no_debug(|| {
        let _ = std::thread::spawn(|| panic!("oh no")).join();
        MyOpaque(100)
    });

    assert!(result.is_none());
    assert!(panik::has_panicked());
}
