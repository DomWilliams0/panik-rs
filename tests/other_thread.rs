mod setup;

use std::sync::{Arc, Mutex};

#[test]
fn other_thread() {
    setup::init();

    let tid = Arc::new(Mutex::new(None));
    let tid_2 = tid.clone();

    let result = panic::run_and_handle_panics(move || {
        let thread = std::thread::spawn(move || {
            let mut tid = tid_2.lock().unwrap();
            *tid = Some(std::thread::current().id());
            drop(tid); // avoid poison
            panic!("teehee")
        });

        let _ = thread.join();

        5
    });

    assert!(result.is_none());
    assert!(panic::has_panicked());

    let panics = panic::panics();
    assert_eq!(panics.len(), 1);

    let panic_tid = {
        let tid = tid.lock().unwrap();
        tid.expect("tid not set")
    };

    let panic = &panics[0];
    assert_eq!(panic.thread_id(), panic_tid);
    assert_ne!(panic.thread_id(), std::thread::current().id());
    assert_eq!(panic.message(), "teehee");
    assert!(panic.is_backtrace_resolved());
}
