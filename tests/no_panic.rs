mod setup;

#[test]
fn no_panic() {
    setup::init();

    let result = panik::run_and_handle_panics(|| "nice");

    assert_eq!(result.to_owned(), Some("nice"));
    assert!(!panik::has_panicked());

    let panics = panik::panics();
    assert!(panics.is_empty());
}
