use criterion::{criterion_group, Criterion};
use gw_store::Store;

pub fn bench(c: &mut Criterion) {
    c.bench_function("db init", |b| b.iter(|| Store::open_tmp().unwrap()));
}

criterion_group! {
    name = init_db;
    config = Criterion::default().sample_size(10);
    targets = bench
}
