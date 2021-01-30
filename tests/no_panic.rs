#[test]
fn no_panic() {
    let result = panic::run_and_handle_panics(|| "nice");

    assert_eq!(result.to_owned(), Some("nice"));
    assert!(!panic::has_panicked());

    let panics = panic::panics();
    assert!(panics.is_empty());
}
