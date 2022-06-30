use std::{future::Future, mem::ManuallyDrop, pin::Pin};

use tokio::task::{JoinError, JoinHandle};

/// Abort task on drop.
pub struct AbortOnDropHandle<T> {
    inner: JoinHandle<T>,
}

impl<T> AbortOnDropHandle<T> {
    /// Replace the task handle with a new task. The previous task is aborted.
    pub fn replace_with(&mut self, handle: JoinHandle<T>) {
        self.inner.abort();
        self.inner = handle;
    }

    /// Convert back to a normal JoinHandle.
    pub fn into_inner(self) -> JoinHandle<T> {
        let this = ManuallyDrop::new(self);
        // Safety: safe because this.inner will not be used anymore.
        unsafe { core::ptr::read(&this.inner) }
    }
}

impl<T> From<JoinHandle<T>> for AbortOnDropHandle<T> {
    fn from(inner: JoinHandle<T>) -> Self {
        Self { inner }
    }
}

impl<T> Future for AbortOnDropHandle<T> {
    type Output = Result<T, JoinError>;
    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        Pin::new(&mut self.inner).poll(cx)
    }
}

impl<T> Drop for AbortOnDropHandle<T> {
    fn drop(&mut self) {
        self.inner.abort();
    }
}
