//! Print what the Linux collector observes, for diagnosing tracking problems.
//!
//! `DISPLAY=:1 cargo run -p localtrack-collector-linux --example probe`

use std::sync::mpsc::channel;
use std::time::Duration;

use localtrack_collector_common::collector::ActivityCollector;
use localtrack_collector_linux::LinuxDesktopCollector;

fn main() {
    let capabilities = localtrack_collector_linux::collector::detect_capabilities();
    println!("adapter: {capabilities:?}");

    let (tx, rx) = channel();
    let mut collector = LinuxDesktopCollector::new(1000, 180_000);
    if let Err(err) = collector.start(tx) {
        println!("collector failed to start: {err}");
        return;
    }

    let deadline = std::time::Instant::now() + Duration::from_secs(8);
    let mut count = 0;
    while std::time::Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(500)) {
            Ok(observation) => {
                count += 1;
                println!("{count:>3} {observation:?}");
            }
            Err(_) => println!("  … nothing"),
        }
    }
    let _ = collector.stop();
    println!("observations: {count}");
    println!("status: {:?}", collector.status());
}
