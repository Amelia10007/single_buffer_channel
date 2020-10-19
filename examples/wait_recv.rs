use single_buffer_channel::channel;

fn main() {
    let (updater, receiver) = channel();

    let join_handle = std::thread::spawn(move || {
        // Send data later...
        std::thread::sleep(std::time::Duration::from_secs(5));
        updater.update(1).unwrap();
    });

    // recv() blocks the current thread, but consumes no CPU time.
    let data = receiver.recv().unwrap();
    join_handle.join().unwrap();
    println!("data: {}", data);
}
