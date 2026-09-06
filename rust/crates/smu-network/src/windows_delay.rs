use std::{
    collections::VecDeque,
    fmt,
    sync::{Arc, Condvar, Mutex, MutexGuard},
    thread::JoinHandle,
    time::{Duration, Instant},
};

#[derive(Debug)]
struct DelayedItem<T> {
    value: T,
    send_at: Instant,
}

#[derive(Debug)]
struct DelayedQueue<T> {
    items: VecDeque<DelayedItem<T>>,
}

impl<T> Default for DelayedQueue<T> {
    fn default() -> Self {
        Self { items: VecDeque::new() }
    }
}

impl<T> DelayedQueue<T> {
    fn push(&mut self, value: T, send_at: Instant) {
        self.items.push_back(DelayedItem { value, send_at });
    }

    fn next_deadline(&self) -> Option<Instant> {
        self.items.front().map(|item| item.send_at)
    }

    fn pop_due(&mut self, now: Instant) -> Option<T> {
        if self.items.front().is_none_or(|item| item.send_at > now) {
            return None;
        }
        self.items.pop_front().map(|item| item.value)
    }

    fn clear(&mut self) {
        self.items.clear();
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.items.len()
    }
}

#[derive(Debug)]
struct DelayedSenderState<T> {
    running: bool,
    queue: DelayedQueue<T>,
}

impl<T> Default for DelayedSenderState<T> {
    fn default() -> Self {
        Self { running: true, queue: DelayedQueue::default() }
    }
}

struct DelayedSenderShared<T> {
    state: Mutex<DelayedSenderState<T>>,
    wake: Condvar,
}

struct WindowsDelayedSender<T> {
    shared: Arc<DelayedSenderShared<T>>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl<T> fmt::Debug for WindowsDelayedSender<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("WindowsDelayedSender").finish_non_exhaustive()
    }
}

impl<T: Send + 'static> WindowsDelayedSender<T> {
    fn start(send: impl Fn(T) + Send + Sync + 'static) -> Self {
        let shared = Arc::new(DelayedSenderShared {
            state: Mutex::new(DelayedSenderState::default()),
            wake: Condvar::new(),
        });
        let worker_shared = Arc::clone(&shared);
        let send = Arc::new(send);
        let worker = std::thread::spawn(move || sender_loop(worker_shared.as_ref(), send.as_ref()));
        Self { shared, worker: Mutex::new(Some(worker)) }
    }

    fn enqueue(&self, value: T, delay: Duration) -> bool {
        let mut state = lock(&self.shared.state);
        if !state.running {
            return false;
        }
        let send_at = Instant::now().checked_add(delay).unwrap_or_else(Instant::now);
        state.queue.push(value, send_at);
        drop(state);
        self.shared.wake.notify_one();
        true
    }

    fn stop(&self) {
        let mut state = lock(&self.shared.state);
        state.running = false;
        drop(state);
        self.shared.wake.notify_all();
        let worker = lock(&self.worker).take();
        if let Some(worker) = worker {
            let _ = worker.join();
        }
    }
}

impl<T> Drop for WindowsDelayedSender<T> {
    fn drop(&mut self) {
        let mut state = lock(&self.shared.state);
        state.running = false;
        drop(state);
        self.shared.wake.notify_all();
        let worker = lock(&self.worker).take();
        if let Some(worker) = worker {
            let _ = worker.join();
        }
    }
}

fn sender_loop<T: Send + 'static>(shared: &DelayedSenderShared<T>, send: &dyn Fn(T)) {
    let mut state = lock(&shared.state);
    loop {
        if !state.running {
            state.queue.clear();
            return;
        }
        let Some(deadline) = state.queue.next_deadline() else {
            state = wait(&shared.wake, state);
            continue;
        };
        let now = Instant::now();
        if let Some(value) = state.queue.pop_due(now) {
            send(value);
            continue;
        }
        let timeout = deadline.saturating_duration_since(now);
        state = wait_timeout(&shared.wake, state, timeout);
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn wait<'a, T>(condvar: &Condvar, guard: MutexGuard<'a, T>) -> MutexGuard<'a, T> {
    match condvar.wait(guard) {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn wait_timeout<'a, T>(
    condvar: &Condvar,
    guard: MutexGuard<'a, T>,
    timeout: Duration,
) -> MutexGuard<'a, T> {
    match condvar.wait_timeout(guard, timeout) {
        Ok((guard, _)) => guard,
        Err(poisoned) => poisoned.into_inner().0,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use std::sync::mpsc;

    #[test]
    fn queue_preserves_cpp_front_of_deque_deadline_behavior() {
        let start = Instant::now();
        let mut queue = DelayedQueue::default();
        queue.push("first", start + Duration::from_millis(20));
        queue.push("second", start + Duration::from_millis(5));

        assert_eq!(queue.pop_due(start + Duration::from_millis(10)), None);
        assert_eq!(queue.pop_due(start + Duration::from_millis(25)), Some("first"));
        assert_eq!(queue.pop_due(start + Duration::from_millis(25)), Some("second"));
    }

    #[test]
    fn sender_delivers_due_packet_and_stop_rejects_new_work() {
        let (tx, rx) = mpsc::channel();
        let sender = WindowsDelayedSender::start(move |value| {
            let _ = tx.send(value);
        });
        assert!(sender.enqueue(7_u32, Duration::ZERO));
        assert_eq!(rx.recv_timeout(Duration::from_secs(1)).expect("delayed packet"), 7);
        sender.stop();
        assert!(!sender.enqueue(8_u32, Duration::ZERO));
    }

    #[test]
    fn queue_clear_drops_every_pending_packet() {
        let start = Instant::now();
        let mut queue = DelayedQueue::default();
        queue.push(1_u8, start + Duration::from_secs(1));
        queue.push(2_u8, start + Duration::from_secs(2));
        assert_eq!(queue.len(), 2);
        queue.clear();
        assert_eq!(queue.len(), 0);
    }
}
