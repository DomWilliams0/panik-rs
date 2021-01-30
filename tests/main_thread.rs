#[test]
fn main_thread() {
    let result = panic::run_and_handle_panics(|| panic!("oh no"));

    assert!(result.is_none());
    assert!(panic::has_panicked());

    let panics = panic::panics();
    assert_eq!(panics.len(), 1);

    let panic = &panics[0];
    assert_eq!(panic.thread_id, std::thread::current().id());
    assert_eq!(panic.message, "oh no");
}
