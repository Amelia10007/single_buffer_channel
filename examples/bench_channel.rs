use single_buffer_channel::{channel, TryRecvError};
use std::time::Instant;

fn bench_update_recv<T: Clone + Send + 'static>(id: &str, value: T, update_count: usize) {
    let (updater, receiver) = channel();

    // Update value on another thread
    let join_handle = std::thread::spawn(move || {
        for _ in 0..update_count {
            updater
                .update(value.clone())
                .ok()
                .expect("Failed to update");
        }
    });

    // Start benchmark
    let recv_start = Instant::now();
    let mut recv_count = 0;

    // Receive the latest values
    loop {
        if let Err(_) = receiver.recv() {
            break;
        }
        recv_count += 1;
    }

    // End benchmark
    let duration = Instant::now() - recv_start;
    join_handle.join().unwrap();

    // Print
    print!("Benchmark {} by bench_update_recv():", id);
    print!("\tUpdate/recv times: {}/{}", update_count, recv_count);
    let sum = duration.as_nanos();
    let ave = sum / update_count as u128;
    print!("\tTook {} (Ave. {}) ns", sum, ave);
    println!();
}

fn bench_update_try_recv<T: Clone + Send + 'static>(id: &str, value: T, update_count: usize) {
    let (updater, receiver) = channel();

    // Update value on another thread
    let join_handle = std::thread::spawn(move || {
        for _ in 0..update_count {
            updater
                .update(value.clone())
                .ok()
                .expect("Failed to update");
        }
    });

    // Start benchmark
    let recv_start = Instant::now();
    let mut recv_count = 0;

    // Receive the latest values
    loop {
        match receiver.try_recv() {
            Ok(_) => recv_count += 1,
            Err(TryRecvError::Disconnected) => break,
            Err(_) => {}
        }
    }

    // End benchmark
    let duration = Instant::now() - recv_start;
    join_handle.join().unwrap();

    // Print
    print!("Benchmark {} by bench_update_try_recv():", id);
    print!("\tUpdate/recv times: {}/{}", update_count, recv_count);
    let sum = duration.as_nanos();
    let ave = sum / update_count as u128;
    print!("\tTook {} (Ave. {}) ns", sum, ave);
    println!();
}

fn main() {
    let update_count = 1_000_000;

    bench_update_recv("i32", 1, update_count);
    bench_update_recv("f64", 1.0, update_count);
    bench_update_recv("u128", 1_u128, update_count);
    bench_update_recv("Unit", (), update_count);
    bench_update_recv("str", "Hololive", update_count);
    bench_update_recv("String", String::from("Hololive"), update_count);
    bench_update_recv("Vec1000", vec![0; 1000], update_count);
    bench_update_recv("Array1000", [0; 1000], update_count);

    bench_update_try_recv("i32", 1, update_count);
    bench_update_try_recv("f64", 1.0, update_count);
    bench_update_try_recv("u128", 1_u128, update_count);
    bench_update_try_recv("Unit", (), update_count);
    bench_update_try_recv("str", "Hololive", update_count);
    bench_update_try_recv("String", String::from("Hololive"), update_count);
    bench_update_try_recv("Vec1000", vec![0; 1000], update_count);
    bench_update_try_recv("Array1000", [0; 1000], update_count);
}
