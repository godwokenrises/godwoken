pub fn blocking_async<T, F>(f: T) -> F
where
    T: std::future::Future<Output = F> + Send + 'static,
    F: Send + 'static,
{
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        let (tx, rx) = crossbeam_channel::bounded(1);
        handle.clone().spawn_blocking(move || {
            let res = handle.block_on(f);
            let _ = tx.send(res);
        });
        tokio::task::block_in_place(|| rx.recv().unwrap())
    } else {
        handle().block_on(f)
    }
}

#[inline]
pub fn block_on<T, F>(f: T) -> F
where
    T: std::future::Future<Output = F>,
{
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        tokio::task::block_in_place(move || handle.block_on(f))
    } else {
        handle().block_on(f)
    }
}

pub fn spawn<T>(f: T) -> tokio::task::JoinHandle<T::Output>
where
    T: std::future::Future + Send + 'static,
    T::Output: Send + 'static,
{
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        handle.spawn(f)
    } else {
        handle().spawn(f)
    }
}

pub fn spawn_blocking<F, R>(f: F) -> tokio::task::JoinHandle<R>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        handle.spawn_blocking(f)
    } else {
        handle().spawn_blocking(f)
    }
}

fn handle() -> &'static tokio::runtime::Handle {
    rt().handle()
}

fn rt() -> &'static tokio::runtime::Runtime {
    static RT: once_cell::sync::OnceCell<tokio::runtime::Runtime> =
        once_cell::sync::OnceCell::new();
    RT.get_or_init(|| {
        let num = std::cmp::max(4, num_cpus::get());
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .worker_threads(num)
            .build()
            .unwrap()
    })
}
