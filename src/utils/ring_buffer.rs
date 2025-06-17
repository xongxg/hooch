use std::mem::MaybeUninit;
use std::process::id;
use std::ptr;
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct LockFreeBoundedRingBuffer<T> {
    /// Internal buffer for storing elements in `MaybeUninit<T>` to avoid unnecessary initializations.
    buffer: Vec<MaybeUninit<T>>,
    start: AtomicUsize,
    end: AtomicUsize,
    count: AtomicUsize,
}

impl<T> LockFreeBoundedRingBuffer<T> {
    /// default buffer size of 1MB * size of T
    const DEFAULT_BUFFER_SIZE: usize = 1024;

    pub fn new(bound: usize) -> Self {
        Self {
            buffer: (0..bound).map(|_| MaybeUninit::uninit()).collect(),
            start: AtomicUsize::new(0),
            end: AtomicUsize::new(0),
            count: AtomicUsize::new(0),
        }
    }

    /// Inserts a value into the buffer at the specified index.
    /// This function is unsafe due to manual memory management.
    ///
    /// # Safety
    /// Caller must ensure the index is within bounds and the buffer slot
    /// is not being accessed concurrently.
    pub fn insert_value(&self, idx: usize, value: T) {
        unsafe {
            let buffer_ptr = self.buffer.as_ptr() as *mut MaybeUninit<T>;
            // Clear existing value
            buffer_ptr.add(idx).drop_in_place();

            // Write new value
            buffer_ptr.add(idx).write(MaybeUninit::new(value));
        }
    }

    /// Returns the capacity of the buffer.
    pub fn capacity(&self) -> usize {
        self.buffer.capacity()
    }

    /// Returns the current number of elements in the buffer.
    pub fn len(&self) -> usize {
        self.count.load(Ordering::Relaxed)
    }

    /// Checks if the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Pushes a value into the buffer. Returns an error if the buffer is full.
    ///
    /// # Parameters
    /// - `value`: The value to be added to the buffer.
    ///
    /// # Returns
    /// - `Ok(())` on success.
    /// - `Err(String)` if the buffer is full.
    pub fn push(&self, value: T) -> Result<(), String> {
        // check if it is full
        if self.len() == self.capacity() {
            return Err("Buffer is full!".into());
        }

        let current_end = self.end.load(Ordering::Acquire);

        // Calculate the next end position in a circular manner
        let new_end = if current_end + 1 < self.capacity() {
            current_end + 1
        } else {
            0
        };

        self.insert_value(current_end, value);
        self.count.fetch_add(1, Ordering::Relaxed);
        self.end.store(new_end, Ordering::Release);

        Ok(())
    }

    /// Retrieves a value from the buffer at the specified index and clears that position.
    ///
    /// # Safety
    /// The caller must ensure the index is within bounds and the slot is being accessed correctly.
    fn get_value(&self, idx: usize) -> T {
        unsafe {
            let buffer_ptr = self.buffer.as_ptr() as *mut MaybeUninit<T>;
            let value = ptr::replace(buffer_ptr.add(idx), MaybeUninit::uninit());
            value.assume_init()
        }
    }

    pub fn pop(&self) -> Option<T> {
        let current_start = self.start.load(Ordering::Acquire);
        let current_end = self.end.load(Ordering::Acquire);

        if current_start == current_end && self.len() == 0 {
            return None;
        }

        let value = self.get_value(current_start);
        self.count.fetch_sub(1, Ordering::Relaxed);

        let new_start = if current_start + 1 >= self.capacity() {
            0
        } else {
            current_start + 1
        };

        self.start.store(new_start, Ordering::Release);
        Some(value)
    }
}

impl<T> Default for LockFreeBoundedRingBuffer<T> {
    fn default() -> Self {
        Self::new(Self::DEFAULT_BUFFER_SIZE)
    }
}
