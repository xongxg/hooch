use crate::executor::{Executor, ExecutorId, ExecutorTask};
use crate::sync::mpsc::{BoundedReceiver, bounded_channel};
use crate::task::Task;
use crate::task::manager::TaskManager;
use std::any::Any;
use std::error::Error;
use std::fmt::Display;
use std::marker::PhantomData;
use std::panic::UnwindSafe;
use std::pin::Pin;
use std::sync::atomic::Ordering::{Relaxed, SeqCst};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{RecvError, SyncSender};
use std::sync::{Arc, Mutex, Weak, mpsc};
use std::task::{Context, Poll};

/// Type alias for boxed, pinned future results with any `Send` type.
pub type BoxedFutureResult =
    Pin<Box<dyn Future<Output = Result<Box<dyn Any + Send>, RecvError>> + Send>>;

/// `Spawner` is responsible for spawning and managing tasks within the runtime.
/// It provides a method for directly spawning tasks and for wrapping futures into `JoinHandle`s.
#[derive(Debug, Clone)]
pub struct Spawner;

impl Spawner {
    /// Convenience method for spawning tasks with the runtime handle.
    pub fn spawn<T>(future: impl Future<Output = T> + Send + 'static) -> JoinHandle<T>
    where
        T: Send + 'static,
    {
        let (tx, rx_bound) = bounded_channel(1);
        let wrapped_future = async move {
            let res = future.await;
            let res: Box<dyn Any + Send + 'static> = Box::new(res);
            let _ = tx.send(res);
        };

        let abort = Arc::new(AtomicBool::new(false));
        let tm = TaskManager::get();

        let task = Arc::new(Task {
            future: Mutex::new(Some(Box::pin(wrapped_future))),
            task_tag: Task::generate_tag(),
            manager: Arc::downgrade(&tm),
            abort: Arc::clone(&abort),
        });

        let jh = JoinHandle {
            rx: Arc::new(rx_bound),
            has_awaited: Arc::new(AtomicBool::new(false)),
            recv_fut: None,
            _marker: PhantomData,
            task: Arc::downgrade(&task),
            abort,
        };

        tm.register_or_execute_non_blocking_task(task);
        jh
    }

    /// Creates a new `Executor` and `Spawner` pair, with a maximum task queue size of 10,000.
    pub fn new_executor_spawner(
        panic_tx: SyncSender<()>,
        executor_id: usize,
    ) -> (Executor, Spawner, SyncSender<ExecutorTask>) {
        const MAX_QUEUED_TASKS: usize = 10_000;
        let (task_sender, ready_queue) = mpsc::sync_channel(MAX_QUEUED_TASKS);
        (
            Executor::new(ready_queue, executor_id, panic_tx),
            Spawner {},
            task_sender,
        )
    }
}

/// A `JoinHandle` represents the handle for a spawned task, allowing the caller to await its completion.
pub struct JoinHandle<T> {
    /// Receiver for the result of the future.
    rx: Arc<BoundedReceiver<Box<dyn Any + Send + 'static>>>,
    /// Flag indicating if the task has been awaited.
    has_awaited: Arc<AtomicBool>,
    ///Future for receiving the result.
    recv_fut: Option<BoxedFutureResult>,
    _marker: PhantomData<T>,
    pub task: Weak<Task>,
    abort: Arc<AtomicBool>,
}

impl<T> UnwindSafe for JoinHandle<T> {}

unsafe impl<T> Send for JoinHandle<T> {}
unsafe impl<T> Sync for JoinHandle<T> {}

impl<T> Future for JoinHandle<T>
where
    T: Unpin + 'static,
{
    type Output = Result<T, JoinHandleError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.as_mut().get_mut();
        this.has_awaited.store(true, Ordering::Relaxed);

        if this.recv_fut.is_none() {
            let rx_clone = Arc::clone(&this.rx);
            let fut = async move { rx_clone.recv().await };
            this.recv_fut = Some(Box::pin(fut));
        }

        match this.recv_fut.as_mut().unwrap().as_mut().poll(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(res) => {
                if let Ok(res) = res {
                    let ret_val = *res.downcast::<T>().unwrap();
                    return Poll::Ready(Ok(ret_val));
                }

                Poll::Ready(Err(JoinHandleError {
                    msg: "Error occurred while awaiting task".into(),
                }))
            }
        }
    }
}

impl<T> Drop for JoinHandle<T> {
    fn drop(&mut self) {
        self.abort.swap(true, SeqCst);
    }
}
/// Error type for `JoinHandle` indicating task execution issues.
#[derive(Debug)]
pub struct JoinHandleError {
    msg: String, // Error message
}

impl Display for JoinHandleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.msg)
    }
}

impl Error for JoinHandleError {}
