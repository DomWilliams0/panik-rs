mod setup;

#[test]
fn nested() {
    let outer = setup::panik_builder().run_and_handle_panics(|| {
        // naughty
        panik::run_and_handle_panics(|| 5);

        0
    });
    assert!(outer.is_none());
    assert!(panik::has_panicked());

    let panics = panik::panics();
    assert_eq!(panics.len(), 1);
    assert_eq!(
        panics[0].message(),
        "nested calls to panik::run_and_handle_panics are not supported"
    );
}
