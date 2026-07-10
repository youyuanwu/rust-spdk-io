//! Callback-to-future utilities for SPDK async operations.
//!
//! SPDK uses callback-based async APIs. This module provides utilities
//! to convert these to Rust futures using oneshot channels.
//!
//! # Pattern
//!
//! 1. Create a completion pair with [`completion()`]
//! 2. Convert sender to raw pointer via [`CompletionSender::into_raw()`]
//! 3. Pass raw pointer as callback context to SPDK
//! 4. In callback, reconstruct sender via [`CompletionSender::from_raw()`]
//! 5. Send the result
//! 6. Await the receiver

use std::ffi::c_void;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};

use futures_channel::oneshot;

use crate::error::{Error, Result};
use crate::thread::SpdkThread;

/// Sender half of a completion pair.
///
/// Convert to raw pointer with [`into_raw()`](Self::into_raw) to pass through
/// C callbacks, then reconstruct with [`from_raw()`](Self::from_raw).
pub struct CompletionSender<T> {
    tx: oneshot::Sender<Result<T>>,
}

impl<T> CompletionSender<T> {
    /// Convert sender to raw pointer for passing to C callbacks.
    ///
    /// # Safety
    ///
    /// The returned pointer must be passed to [`from_raw()`](Self::from_raw)
    /// exactly once to avoid memory leaks.
    pub fn into_raw(self) -> *mut c_void {
        Box::into_raw(Box::new(self.tx)) as *mut c_void
    }

    /// Reconstruct sender from raw pointer.
    ///
    /// # Safety
    ///
    /// The pointer must have been created by [`into_raw()`](Self::into_raw)
    /// and must not have been used already.
    pub unsafe fn from_raw(ptr: *mut c_void) -> Self {
        let tx = unsafe { *Box::from_raw(ptr as *mut oneshot::Sender<Result<T>>) };
        Self { tx }
    }

    /// Send a successful result.
    pub fn complete(self, result: Result<T>) {
        // Ignore send error - receiver may have been dropped
        let _ = self.tx.send(result);
    }

    /// Send a successful value.
    pub fn success(self, value: T) {
        let _ = self.tx.send(Ok(value));
    }

    /// Send an error.
    pub fn error(self, err: Error) {
        let _ = self.tx.send(Err(err));
    }
}

/// Receiver half of a completion pair.
///
/// Implements `Future` - await this to get the result.
pub struct CompletionReceiver<T> {
    rx: oneshot::Receiver<Result<T>>,
}

impl<T> Future for CompletionReceiver<T> {
    type Output = Result<T>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match Pin::new(&mut self.rx).poll(cx) {
            Poll::Ready(Ok(result)) => Poll::Ready(result),
            Poll::Ready(Err(_)) => Poll::Ready(Err(Error::Cancelled)),
            Poll::Pending => Poll::Pending,
        }
    }
}

/// Create a completion sender/receiver pair.
pub fn completion<T>() -> (CompletionSender<T>, CompletionReceiver<T>) {
    let (tx, rx) = oneshot::channel();
    (CompletionSender { tx }, CompletionReceiver { rx })
}

/// Helper to create a completion for I/O operations that return `()` on success.
///
/// This is a convenience for the common case of operations that only
/// signal success/failure without returning data.
pub fn io_completion() -> (CompletionSender<()>, CompletionReceiver<()>) {
    completion()
}

/// Block on a future, polling the SPDK thread while waiting.
///
/// This function runs the future to completion by repeatedly:
/// 1. Polling the future
/// 2. If pending, polling the SPDK thread to process I/O completions
///
/// This is necessary because SPDK's I/O callbacks only fire when the
/// thread is polled.
///
/// # Panics
///
/// Panics if called from outside an SPDK thread context (i.e., if there's
/// no current SPDK thread attached to this OS thread).
///
/// # Example
///
/// ```no_run
/// use spdk_io::{Bdev, DmaBuf, complete::block_on};
///
/// // Inside an SPDK app callback:
/// let bdev = Bdev::get_by_name("Null0").unwrap();
/// let desc = bdev.open(true).unwrap();
/// let channel = desc.get_io_channel().unwrap();
///
/// let mut buf = DmaBuf::alloc(512, 512).unwrap();
///
/// // Block until I/O completes
/// block_on(desc.read(&channel, &mut buf, 0)).unwrap();
/// ```
pub fn block_on<F: Future>(mut future: F) -> F::Output {
    // We poll manually, so a no-op waker is sufficient.
    let mut cx = Context::from_waker(Waker::noop());

    // Get the current SPDK thread - panic if not in SPDK context
    let thread = SpdkThread::get_current().expect("block_on called outside SPDK thread context");

    // Pin the future on the stack
    let mut future = unsafe { Pin::new_unchecked(&mut future) };

    loop {
        // Poll the future
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(result) => return result,
            Poll::Pending => {
                // Poll the SPDK thread to process I/O completions
                thread.poll();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_completion_success() {
        let (tx, rx) = completion::<i32>();
        tx.success(42);

        // Use noop waker to poll the future
        let waker = futures_task::noop_waker();
        let mut cx = Context::from_waker(&waker);

        let mut rx = rx;
        match Pin::new(&mut rx).poll(&mut cx) {
            Poll::Ready(Ok(v)) => assert_eq!(v, 42),
            other => panic!("unexpected: {:?}", other),
        }
    }

    #[test]
    fn test_completion_error() {
        let (tx, rx) = completion::<()>();
        tx.error(Error::IoError);

        let waker = futures_task::noop_waker();
        let mut cx = Context::from_waker(&waker);

        let mut rx = rx;
        match Pin::new(&mut rx).poll(&mut cx) {
            Poll::Ready(Err(Error::IoError)) => {}
            other => panic!("unexpected: {:?}", other),
        }
    }

    #[test]
    fn test_into_raw_from_raw() {
        let (tx, rx) = completion::<i32>();
        let ptr = tx.into_raw();
        assert!(!ptr.is_null());

        let tx2 = unsafe { CompletionSender::<i32>::from_raw(ptr) };
        tx2.success(123);

        let waker = futures_task::noop_waker();
        let mut cx = Context::from_waker(&waker);

        let mut rx = rx;
        match Pin::new(&mut rx).poll(&mut cx) {
            Poll::Ready(Ok(v)) => assert_eq!(v, 123),
            other => panic!("unexpected: {:?}", other),
        }
    }
}
