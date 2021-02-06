mod setup;

#[test]
fn multiple_usage() {
    setup::init();

    let a = panic::run_and_handle_panics(|| panic!("numero one"));
    assert!(a.is_none());
    assert!(panic::has_panicked());

    let panics = panic::panics();
    assert_eq!(panics.len(), 1);
    assert_eq!(panics[0].message(), "numero one");

    let b = panic::run_and_handle_panics(|| 1);
    assert_eq!(b, Some(1));
    assert!(!panic::has_panicked());
    assert!(panic::panics().is_empty());

    let c = panic::run_and_handle_panics(|| panic!("numero two"));
    assert!(c.is_none());
    assert!(panic::has_panicked());

    let panics = panic::panics();
    assert_eq!(panics.len(), 1);
    assert_eq!(panics[0].message(), "numero two");
}
