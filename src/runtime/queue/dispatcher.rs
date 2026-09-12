//! Background scheduling and lifecycle. Queue policies perform individual deliveries.
use super::dispatch::dispatch_queue;
use super::QueueRegistry;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

#[derive(Default)]
struct Signal {
    pending: Mutex<bool>,
    changed: Condvar,
    shutdown: AtomicBool,
}

impl Signal {
    fn wake(&self) {
        if let Ok(mut pending) = self.pending.lock() {
            *pending = true;
            self.changed.notify_all();
        }
    }

    fn wait(&self) {
        if let Ok(mut pending) = self.pending.lock() {
            if !*pending {
                let _ = self
                    .changed
                    .wait_timeout(pending, Duration::from_millis(50));
            } else {
                *pending = false;
            }
        }
    }

    fn is_shutdown(&self) -> bool {
        self.shutdown.load(Ordering::SeqCst)
    }
}

pub(super) struct Dispatcher {
    signal: Arc<Signal>,
    handle: Mutex<Option<JoinHandle<()>>>,
}

impl Dispatcher {
    pub(super) fn start(queues: QueueRegistry) -> Self {
        let signal = Arc::new(Signal::default());
        let thread_signal = Arc::clone(&signal);
        let handle = std::thread::Builder::new()
            .name("airbus-dispatcher".to_string())
            .spawn(move || run(queues, thread_signal))
            .expect("spawn dispatcher thread");
        Self {
            signal,
            handle: Mutex::new(Some(handle)),
        }
    }

    pub(super) fn wake(&self) {
        self.signal.wake();
    }
}

impl Drop for Dispatcher {
    fn drop(&mut self) {
        self.signal.shutdown.store(true, Ordering::SeqCst);
        self.signal.wake();
        if let Ok(mut handle) = self.handle.lock() {
            if let Some(handle) = handle.take() {
                let _ = handle.join();
            }
        }
    }
}

fn run(queues: QueueRegistry, signal: Arc<Signal>) {
    while !signal.is_shutdown() {
        signal.wait();
        if signal.is_shutdown() {
            break;
        }

        // Release the registry lock before processing queues or doing network I/O.
        let entries: Vec<_> = {
            let Ok(queues) = queues.lock() else { continue };
            queues
                .iter()
                .map(|(name, queue)| (name.clone(), Arc::clone(queue)))
                .collect()
        };
        for (name, queue) in entries {
            dispatch_queue(&name, &queue);
        }
    }
}
