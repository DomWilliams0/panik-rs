mod setup;

#[test]
fn nested() {
    setup::init();

    let outer = panic::run_and_handle_panics(|| {
        // naughty
        panic::run_and_handle_panics(|| 5);

        0
    });
    assert!(outer.is_none());
    assert!(panic::has_panicked());

    let panics = panic::panics();
    assert_eq!(panics.len(), 1);
    assert_eq!(
        panics[0].message(),
        "nested calls to panic::run_and_handle_panics are not supported"
    );
}
