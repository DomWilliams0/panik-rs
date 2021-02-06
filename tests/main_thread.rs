mod setup;

#[test]
fn main_thread() {
    setup::init();

    let result = panik::run_and_handle_panics(|| panic!("oh no"));
    assert!(result.is_none());
    assert!(panik::has_panicked());

    let panics = panik::panics();
    assert_eq!(panics.len(), 1);

    let panic = &panics[0];
    assert_eq!(panic.thread_id(), std::thread::current().id());
    assert_eq!(panic.message(), "oh no");
    assert!(panic.is_backtrace_resolved());
}
