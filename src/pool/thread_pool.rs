use std::cell::OnceCell;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::mpsc::SyncSender;
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::SystemTime;

pub type BoxedFn = Box<dyn FnOnce() + Send + 'static>;

thread_local! {
    pub static HOOCH_POOL:OnceCell<Arc<HoochPool>> = const { OnceCell::new() };
}

#[derive(Debug)]
pub struct HoochPool {
    num_threads: usize,
    cursor: AtomicUsize,
    senders: Vec<SyncSender<BoxedFn>>,
}

impl HoochPool {
    pub fn init(num_threads: usize) {
        let mut senders = Vec::with_capacity(num_threads);
        let mut handles = Vec::with_capacity(num_threads);

        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();

        (0..num_threads).for_each(|idx| {
            let (tx, rx) = mpsc::sync_channel::<BoxedFn>(1);
            let handle = thread::Builder::new()
                .name(format!("hooch_pool_{}_{}", idx, now))
                .spawn(move || {
                    while let Ok(boxed_fn) = rx.recv() {
                        boxed_fn();
                    }
                })
                .unwrap();

            senders.push(tx);
            handles.push(handle);
        });

        let hooch_pool = HoochPool {
            num_threads,
            cursor: AtomicUsize::new(0),
            senders,
        };

        HOOCH_POOL.with(move |cell| {
            cell.get_or_init(move || Arc::new(hooch_pool));
        })
    }

    pub fn get() -> Arc<Self> {
        HOOCH_POOL.with(|cell| Arc::clone(cell.get().unwrap()))
    }

    fn get_and_update_index(&self) -> usize {
        let current_value = self.cursor.load(Relaxed);
        let new_idx = (current_value + 1) % self.num_threads;
        self.cursor.store(new_idx, Relaxed);
        current_value
    }

    pub fn execute(self: &Arc<Self>, boxed_fn: BoxedFn) {
        let idx = self.get_and_update_index();
        let sender = &self.senders[idx];
        sender.send(boxed_fn).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::panic::catch_unwind;
    use std::sync::Mutex;
    use std::sync::atomic::Ordering;
    use std::sync::mpsc::sync_channel;

    #[test]
    fn test_hooch_pool_execution() {
        HoochPool::init(1);

        let pool = HoochPool::get();
        let actual_name = Arc::new(Mutex::new(String::new()));

        let actual_name_clone = Arc::clone(&actual_name);

        let (tx, rx) = sync_channel(1);
        let test_fn = move || {
            *actual_name.lock().unwrap() = std::thread::current().name().unwrap().to_string();
            tx.send(()).unwrap();
        };

        pool.execute(Box::new(test_fn));

        let d = rx.recv().unwrap();

        let expected_contains = "hooch_pool_0";
        assert!(
            actual_name_clone
                .lock()
                .unwrap()
                .contains(expected_contains)
        );
    }

    #[test]
    fn test_index_update() {
        HoochPool::init(2);
        let hooch_executor = HoochPool::get();
        assert!(hooch_executor.cursor.load(Ordering::Relaxed) == 0);
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let test_fn = move || {
            tx.send(()).unwrap();
        };
        hooch_executor.execute(Box::new(test_fn));
        rx.recv().unwrap();
        assert!(hooch_executor.cursor.load(Ordering::Relaxed) == 1);

        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let tx_clone = tx.clone();
        let test_fn = move || {
            tx.send(()).unwrap();
        };
        hooch_executor.execute(Box::new(test_fn));
        rx.recv().unwrap();
        assert!(hooch_executor.cursor.load(Ordering::Relaxed) == 0);

        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let test_fn1 = move || {
            tx.send(()).unwrap();
        };
        hooch_executor.execute(Box::new(test_fn1));
        rx.recv().unwrap();
        assert!(hooch_executor.cursor.load(Ordering::Relaxed) == 1);
    }
}
