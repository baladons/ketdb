use std::hint::black_box;
use std::time::Instant;

use compio::runtime::Runtime;
use format::{
    context::Context,
    store::{Store, WriteConcern},
};

const OPS: usize = 10_000;

fn main() {
    let dir = std::env::temp_dir().join(format!("ketdb-bench-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_current_dir(&dir).unwrap();
    Context::new(1024).into_current();

    let mut write_order: Vec<u64> = (0..OPS as u64).collect();
    shuffle(&mut write_order, 0xC0FFEE);
    let mut read_order = write_order.clone();
    shuffle(&mut read_order, 0xBAD5EED);

    Runtime::new().unwrap().block_on(async {
        let mut store = Store::open().await;
        store.create_table(1, &[0, 1]).await;

        let write_started = Instant::now();
        for key in &write_order {
            let key_bytes = key.to_be_bytes();
            let value = key.wrapping_mul(10).to_be_bytes();
            store
                .insert(
                    1,
                    &[0, 1],
                    &[&key_bytes, &value],
                    WriteConcern {
                        buffered_journal: true,
                    },
                )
                .await;
        }
        let write_elapsed = write_started.elapsed();

        let flush_started = Instant::now();
        store.flush(1).await;
        let flush_elapsed = flush_started.elapsed();

        let read_started = Instant::now();
        for key in &read_order {
            let key_bytes = key.to_be_bytes();
            let row = store.read(1, &[1], &key_bytes).await.expect("missing row");
            let expected = key.wrapping_mul(10).to_be_bytes();
            assert_eq!(row.get(0), expected);
            black_box(row);
        }
        let read_elapsed = read_started.elapsed();

        report("write", write_elapsed);
        report("flush", flush_elapsed);
        report("read", read_elapsed);
    });

    let _ = std::fs::remove_dir_all(&dir);
}

fn report(label: &str, elapsed: std::time::Duration) {
    let seconds = elapsed.as_secs_f64();
    let rate = OPS as f64 / seconds;
    println!("{label}: {rate:.0} ops/sec ({OPS} ops in {seconds:.3}s)");
}

fn shuffle(items: &mut [u64], mut state: u64) {
    for index in (1..items.len()).rev() {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1);
        let swap = (state >> 33) as usize % (index + 1);
        items.swap(index, swap);
    }
}
