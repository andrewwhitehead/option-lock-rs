use std::{hint::spin_loop, sync::Arc, thread};

use option_lock::Mutex;

// FIXME - this example would work equally well with a simple atomic

fn main() {
    let shared = Arc::new(Mutex::new(0i32));
    let threads = 100;
    for _ in 0..threads {
        let shared = shared.clone();
        thread::spawn(move || {
            let mut guard = shared.spin_lock().unwrap();
            *guard += 1;
        });
    }
    loop {
        if shared.try_copy() == Ok(threads) {
            break;
        }
        spin_loop()
    }
    println!("Completed {} threads", threads);
}
