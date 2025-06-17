use crate::executor::{ExecutorId, ExecutorTask};
use crate::task::Task;
use crate::utils::ring_buffer::LockFreeBoundedRingBuffer;
use dashmap::DashMap;
use std::cell::OnceCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::SyncSender;

pub const MAX_TASKS: usize = 1024 * 1024;

static TASK_MANAGER_ID: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    pub static TASK_MANAGER: OnceCell<Arc<TaskManager>> = const {OnceCell::new()}
}

pub struct TaskManager {
    id: usize,
    waiting_tasks: LockFreeBoundedRingBuffer<Arc<Task>>,
    waiting_executors: LockFreeBoundedRingBuffer<ExecutorId>,
    executors: DashMap<ExecutorId, SyncSender<ExecutorTask>>,
}

/// For debugging
fn generate_task_manager_id() -> usize {
    TASK_MANAGER_ID.fetch_add(1, Ordering::Relaxed)
}

unsafe impl Sync for TaskManager {}

impl TaskManager {
    pub fn get() -> Arc<TaskManager> {
        let mut arc = None;
        TASK_MANAGER.with(|cell| {
            let arc_inner = cell.get_or_init(|| {
                Arc::new(TaskManager {
                    id: generate_task_manager_id(),
                    waiting_tasks: LockFreeBoundedRingBuffer::new(128 * 1000),
                    waiting_executors: LockFreeBoundedRingBuffer::new(128 * 1000),
                    executors: DashMap::with_capacity(128),
                })
            });

            arc = Some(Arc::clone(arc_inner));
        });
        arc.unwrap()
    }

    /// For debugging
    pub fn id(&self) -> usize {
        self.id
    }

    /// Register an executor to the task manager
    pub fn register_executor(
        &self,
        executor_id: ExecutorId,
        executor_sender: SyncSender<ExecutorTask>,
    ) {
        self.executors.insert(executor_id, executor_sender);
        self.waiting_executors.push(executor_id).unwrap()
    }

    /// Executor is ready for another task, if a task is immediately available then execute it,
    /// otherwise wait for a task to be executed
    pub fn executor_ready(&self, executor_id: ExecutorId) {
        while let Some(task) = self.waiting_tasks.pop() {
            if task.has_aborted() {
                continue;
            }

            let sender = self.executors.get(&executor_id).unwrap();
            sender.send(ExecutorTask::Task(task)).unwrap();

            return;
        }

        self.waiting_executors.push(executor_id).unwrap();
    }

    /// If no executor is ready, register the task as waiting otherwise execute immediately.
    pub fn register_or_execute_non_blocking_task(&self, task: Arc<Task>) {
        if task.has_aborted() {
            return;
        }

        if let Some(exeId) = self.waiting_executors.pop() {
            let sender = self.executors.get(&exeId).unwrap();
            sender.send(ExecutorTask::Task(task)).unwrap();

            return;
        }

        self.waiting_tasks.push(task).unwrap();
    }
}
