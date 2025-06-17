use crate::task::Task;
use crate::task::manager::TaskManager;
use std::fmt::{Display, Formatter};
use std::panic::catch_unwind;
use std::sync::Arc;
use std::sync::mpsc::SyncSender;
use std::task::{Context, Poll, Waker};

/// Id to identify executors
#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
pub struct ExecutorId(usize);

impl ExecutorId {
    pub fn get(&self) -> usize {
        self.0
    }
}

/// Enum representing a task that the executor can handle.
/// It can either be a `Task` to execute, or a `Finished` signal to stop the executor.
pub enum ExecutorTask {
    /// Represents a task to be executed, wrapped in an `Arc` for shared ownership. This task
    /// can be seen as a lightweight which will not block the executor.
    Task(Arc<Task>),
    /// Represents a signal that indicates the executor should stop.
    Finished,
}

impl Display for ExecutorTask {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ExecutorTask::Task(_) => write!(f, "ExecutorTask::Task"),
            ExecutorTask::Finished => write!(f, "ExecutorTask::Finished"),
        }
    }
}

pub type BlockingFn = dyn FnOnce() + Send + 'static;

/// Enum representing the status of a task.
/// This is used to track whether a task has already completed or if it is still awaited.
pub enum Status {
    /// The task is awaited by a specific `Waker`, which will be used to notify when it can proceed.
    Awaited(Waker),
    /// The task has already happened or completed.
    Happened,
}

/// A basic executor that runs tasks from a ready queue and catches any panics
/// that may occur while polling tasks. It also sends a message on `panic_tx`
/// if a panic is encountered.
///
/// The executor is designed to run tasks until it receives an `ExecutorTask::Finished`
/// signal.
pub struct Executor {
    /// Unique executor identifier
    id: ExecutorId,
    /// A sender for notifying about panics that occur while executing tasks.
    panic_tx: SyncSender<()>,
    /// A receiver for the ready queue that provides tasks to be executed.
    ready_queue: std::sync::mpsc::Receiver<ExecutorTask>,
}

impl Executor {
    /// Creates a new `Executor` with the given ready queue receiver and panic notification sender.
    ///
    /// # Parameters
    /// - `rx`: The receiver end of a sync channel that provides tasks for the executor.
    /// - `panic_tx`: A sender to signal if a panic occurs while polling a task.
    ///
    /// # Returns
    /// A new instance of `Executor`.
    pub fn new(
        rx: std::sync::mpsc::Receiver<ExecutorTask>,
        executor_id: usize,
        panic_tx: SyncSender<()>,
    ) -> Self {
        Self {
            id: ExecutorId(executor_id),
            panic_tx,
            ready_queue: rx,
        }
    }

    pub fn run(&self) {
        while let Ok(task) = self.ready_queue.recv() {
            match task {
                ExecutorTask::Task(task) => {
                    self.forward(task);
                }
                ExecutorTask::Finished => return,
            }

            let tm = TaskManager::get();
            tm.executor_ready(self.id);
        }
    }

    fn forward(&self, task: Arc<Task>) {
        let mut future = task.future.lock().unwrap();

        // Create a waker from the task to construct the context
        let waker = Arc::clone(&task).wake();
        let mut context = Context::from_waker(&waker);

        // Allow the future to make progress by polling it
        if let Err(e) = catch_unwind(move || {
            if let Some(fut) = future.as_mut() {
                match fut.as_mut().poll(&mut context) {
                    Poll::Pending => {
                        // Future is pending still do nothing
                    }

                    Poll::Ready(()) => {
                        // Mark the future as done by setting it to none so it won't be polled
                        // again
                        *future = None;
                    }
                }
            }
        }) {
            println!("EXECUTOR PANIC FUNCTION. ERROR: {:?}", e);
            self.panic_tx.send(()).unwrap(); // Send panic signal
        }
    }

    pub fn id(&self) -> ExecutorId {
        self.id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pool::thread_pool::HoochPool;
    use crate::runtime::RuntimeBuilder;
    use crate::sync::mpsc::bounded_channel;
    use std::sync::Mutex;
    use std::sync::mpsc::sync_channel;

    #[test]
    fn test_runs_on_pool_thread() {
        let handle_executor = RuntimeBuilder::default().build();
        let blocking = Arc::new(Mutex::new(0));

        let blocking_clone = Arc::clone(&blocking);
        handle_executor.run_blocking(async move {
            let (tx, rx) = sync_channel(1);

            let block_fn = move || {
                *blocking_clone.lock().unwrap() += 1;
                tx.send(()).unwrap();
            };

            let hp = HoochPool::get();
            hp.execute(Box::new(block_fn));
            
            rx.recv().unwrap();
        });
        
        println!("blocking: {:?}", blocking.lock().unwrap());
    }
}
