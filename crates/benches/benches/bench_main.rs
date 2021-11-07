//! Godwoken Benchmarks main entry.
mod benchmarks;

use criterion::criterion_main;

criterion_main! {
    benchmarks::init_db::init_db,
    benchmarks::sudt::sudt,
    benchmarks::smt::smt,
}
