mod setup;

#[test]
fn multiple_usage() {
    let builder = setup::panik_builder();

    let a = builder
        .clone()
        .run_and_handle_panics(|| panic!("numero one"));
    assert!(a.is_none());
    assert!(panik::has_panicked());

    let panics = panik::panics();
    assert_eq!(panics.len(), 1);
    assert_eq!(panics[0].message(), "numero one");

    let b = builder.clone().run_and_handle_panics(|| 1);
    assert_eq!(b, Some(1));
    assert!(!panik::has_panicked());
    assert!(panik::panics().is_empty());

    let c = builder.run_and_handle_panics(|| panic!("numero two"));
    assert!(c.is_none());
    assert!(panik::has_panicked());

    let panics = panik::panics();
    assert_eq!(panics.len(), 1);
    assert_eq!(panics[0].message(), "numero two");
}
