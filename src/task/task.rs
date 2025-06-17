use crate::task::manager::TaskManager;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::task::{RawWaker, RawWakerVTable, Waker};

#[derive(Debug, Hash, PartialEq, Eq, Clone, Copy)]
pub struct TaskTag(usize);

static TASK_TAG_NUM: AtomicUsize = AtomicUsize::new(0);

/// A `Task` represents an asynchronous operation to be executed by an executor.
/// It stores the future that represents the task, as well as a `Spawner` for task management.
pub struct Task {
    pub future: Mutex<Option<Pin<Box<dyn Future<Output = ()> + Send + 'static>>>>,
    pub task_tag: TaskTag,
    pub manager: Weak<TaskManager>,
    pub abort: Arc<AtomicBool>,
}

impl Task {
    const WAKER_VTABLE: RawWakerVTable = RawWakerVTable::new(clone, wake, wake_by_ref, drop);

    /// Creates a `Waker` for this task, allowing it to be polled by an executor.
    pub fn wake(self: Arc<Self>) -> Waker {
        let opaque_ptr = Arc::into_raw(self) as *const ();
        let vtable = &Self::WAKER_VTABLE;
        unsafe { Waker::from_raw(RawWaker::new(opaque_ptr, vtable)) }
    }

    pub fn generate_tag() -> TaskTag {
        TaskTag(TASK_TAG_NUM.fetch_add(1, Ordering::Relaxed))
    }

    pub fn has_aborted(&self) -> bool {
        self.abort.load(Ordering::SeqCst)
    }
}

pub fn clone(ptr: *const ()) -> RawWaker {
    let original = unsafe { Arc::from_raw(ptr as *const Task) };
    let arc_clone = Arc::clone(&original);
    std::mem::forget(original);

    RawWaker::new(Arc::into_raw(arc_clone) as *const (), &Task::WAKER_VTABLE)
}

pub fn wake(ptr: *const ()) {
    let original = unsafe { Arc::from_raw(ptr as *const Task) };
    let tm = original.manager.upgrade().unwrap();
    tm.register_or_execute_non_blocking_task(original);
}

pub fn wake_by_ref(ptr: *const ()) {
    let original = unsafe { Arc::from_raw(ptr as *const Task) };
    let tm = original.manager.upgrade().unwrap();
    tm.register_or_execute_non_blocking_task(Arc::clone(&original));
    std::mem::forget(original);
}

pub fn drop(ptr: *const ()) {
    let _ = unsafe { Arc::from_raw(ptr as *const Task) };
}
