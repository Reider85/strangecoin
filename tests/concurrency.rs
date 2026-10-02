mod common;

use common::*;
use rand::seq::SliceRandom;
use rand::{rngs::StdRng, SeedableRng};
use std::sync::{Arc, Mutex};
use std::thread;

#[test]
fn deadlock_test_blockchain_wallet_lock_order() {
    let _dir = TestDir::new("deadlock");
    let blockchain = Arc::new(create_test_blockchain(_dir.path()));
    let wallet_lock = Arc::new(Mutex::new(()));

    let mut handles = vec![];

    for i in 0..100 {
        let bc = Arc::clone(&blockchain);
        let wl = Arc::clone(&wallet_lock);
        handles.push(thread::spawn(move || {
            let mut rng = StdRng::seed_from_u64(3735928559 + i);
            let mut order: [u8; 2] = [0, 1];
            order.shuffle(&mut rng);

            for &o in &order {
                match o {
                    0 => {
                        bc.with_inner(|_| {});
                    }
                    1 => {
                        let _g = wl.lock().unwrap();
                    }
                    _ => unreachable!(),
                }
            }
            thread::sleep(std::time::Duration::from_millis(1));
        }));
    }

    for h in handles {
        h.join().expect("Thread panicked - possible deadlock");
    }
}
