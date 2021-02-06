mod setup;

#[test]
fn non_debug() {
    setup::init();

    struct MyOpaque(i32);

    let result = panic::run_and_handle_panics_no_debug(|| MyOpaque(100));

    assert!(result.is_some());
    assert!(!panic::has_panicked());

    let opaque = result.unwrap();
    assert_eq!(opaque.0, 100);
}
