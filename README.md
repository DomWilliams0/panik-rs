# panik-rs

![Build Status](https://img.shields.io/github/workflow/status/DomWilliams0/panik-rs/Build%20and%20test)
[![Documentation](https://docs.rs/panik/badge.svg)](https://docs.rs/panik)
[![Version](https://img.shields.io/crates/v/panik)](https://crates.io/crates/panik)
[![License](https://img.shields.io/crates/l/panik)](https://github.com/DomWilliams0/panik-rs/blob/master/LICENSE)

<p align="center"> <img src="panik.jpg" width=256/> </p>

This crate enables **application-wide panic handling**, whereby panics occurring in any thread
are captured and stored, and can later be queried to trigger an early application exit.

This goes against the standard panic behaviour where a panic is isolated to the thread that
caused it. This library introduces the condition that *any panic in any thread is an error*
and the application cannot continue or recover.

# Use case

The main use case for this crate is when a thread spawns some threads to do work, and blocks on
their completion. If a worker thread panics before the result is posted, the waiting thread might get stuck in
a blocking call to `recv`, unless it specifically plans and checks for this error case (e.g. poisoned
mutex, disconnected mpsc sender).

In a large application with thread pools and lots of types of work being posted to it all over
the place (like a game engine), it can be hard to handle every panic case properly. Using
this library allows the main thread to poll for panics in its core game loop and exit
gracefully, rather than soldiering on without its audio/rendering/AI/worker threads.


An example that doesn't use panic detection and hangs forever:
```rust
let (tx, rx) = std::sync::mpsc::channel();
let worker = std::thread::spawn(move || {
    // hopefully do some work...
    // tx.send(5).unwrap();

    // ...or panic and hold up the main thread forever
    todo!()
});

let result: i32 = rx.recv().expect("recv failed"); // blocks forever
println!("result: {}", result);
```

The same example detecting and handling panics and exiting gracefully:
```rust
let application_result = panic::run_and_handle_panics(|| {
    let (tx, rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        // do some work...
        // tx.send(5).unwrap();

        // ...or panic and hold up the main thread forever
        todo!()
    });

    // periodically check if a panic has occurred
    let poll_freq = Duration::from_secs(5);
    while !panic::has_panicked() {
        if let Ok(res) = rx.recv_timeout(poll_freq) {
            return res;
        }
    }

    // return value is irrelevant here, the panic on the worker
    // thread will clobber this when `run_and_handle_panics`
    // returns None
    0
});

match application_result {
    None => {
        eprintln!("something went wrong: {:?}", panic::panics());
        std::process::exit(1);
    },
    Some(result) => {
        println!("result: {}", result);
        std::process::exit(0);
    }
}
```

This looks pretty heavyweight, but this intentional - this library is meant for large
and heavyweight applications!

# Features
* `use-stderr`: log panics to stderr
* `use-log`: log panics with the `log` crate
* `use-slog`: log panics with the `slog` crate (see `Builder::slogger`)
* `use-parking-lot`: use `parking_lot::Mutex` instead of `std::sync::Mutex`
