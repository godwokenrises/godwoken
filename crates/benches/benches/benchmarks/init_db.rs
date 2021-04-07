use criterion::{criterion_group, Criterion};
use gw_store::Store;

pub fn criterion_benchmark(c: &mut Criterion) {
    c.bench_function("db init", |b| b.iter(|| Store::open_tmp().unwrap()));
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(10);
    targets = criterion_benchmark
}
