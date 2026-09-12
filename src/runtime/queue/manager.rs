use super::queue::{DispatchStrategy, ListenerRegistration, Queue, QueueMode};
use crate::io::log;
use crate::runtime::UuidV7;
use serde_json::Value;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct ListenerInfo {
    pub id: UuidV7,
    pub queue: String,
    pub host: String,
    pub port: u16,
    pub mode: QueueMode,
    pub failure_count: u64,
    pub active: bool,
}

fn deliver_event(
    host: &str,
    port: u16,
    queue: &str,
    event_id: UuidV7,
    event: &Value,
    timeout: Duration,
) -> Result<(), String> {
    let addr_str = format!("{host}:{port}");
    let addr: SocketAddr = match addr_str.parse() {
        Ok(a) => a,
        Err(_) => match addr_str.to_socket_addrs() {
            Ok(mut addrs) => match addrs.next() {
                Some(a) => a,
                None => return Err(format!("cannot resolve address: {addr_str}")),
            },
            Err(e) => return Err(format!("cannot resolve address {addr_str}: {e}")),
        },
    };

    let mut stream = TcpStream::connect_timeout(&addr, timeout)
        .map_err(|e| format!("connect failed to {addr}: {e}"))?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(|e| e.to_string())?;

    let payload = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "on_event",
        "params": {
            "queue": queue,
            "id": event_id.to_string(),
            "event": event
        },
        "id": 1
    });

    let body = serde_json::to_vec(&payload).map_err(|e| e.to_string())?;
    stream.write_all(&body).map_err(|e| e.to_string())?;
    let _ = stream.shutdown(Shutdown::Write);

    let mut resp_bytes = Vec::new();
    stream
        .read_to_end(&mut resp_bytes)
        .map_err(|e| e.to_string())?;

    let resp: Value =
        serde_json::from_slice(&resp_bytes).map_err(|e| format!("invalid json response: {e}"))?;
    if let Some(err) = resp.get("error") {
        return Err(format!("rpc error from client: {err}"));
    }
    if resp.get("result").is_none() {
        return Err("missing result in rpc response".to_string());
    }

    Ok(())
}

fn dispatch_queue(queue_name: &str, queue_arc: &Arc<Mutex<Queue>>) {
    loop {
        let (mode, event_opt, listeners_snapshot) = {
            let mut q = match queue_arc.lock() {
                Ok(q) => q,
                Err(_) => break,
            };
            if q.depth() == 0 || q.listener_count() == 0 {
                break;
            }
            let event = q.first_event();
            let listeners = match (q.mode(), &event) {
                (QueueMode::Broadcast, Some((id, _))) => q.broadcast_listeners(*id),
                _ => q.listeners().to_vec(),
            };
            (q.mode(), event, listeners)
        };

        let Some((event_id, event_val)) = event_opt else {
            break;
        };
        if listeners_snapshot.is_empty() {
            break;
        }

        match mode {
            QueueMode::Broadcast => {
                let mut delivery_results = Vec::new();
                for listener in &listeners_snapshot {
                    let timeout = Duration::from_millis(2000).min(listener.exhaustion_timeout);
                    let res = deliver_event(
                        &listener.host,
                        listener.port,
                        queue_name,
                        event_id,
                        &event_val,
                        timeout,
                    );
                    delivery_results.push((listener.id, res));
                }

                let any_succeeded = delivery_results.iter().any(|(_, res)| res.is_ok());

                let mut q = match queue_arc.lock() {
                    Ok(q) => q,
                    Err(_) => break,
                };

                let now = Instant::now();
                for (listener_id, res) in delivery_results {
                    if res.is_ok() {
                        q.acknowledge_broadcast(&event_id, &listener_id);
                    }
                    if let Some(pos) = q.listeners_mut().iter().position(|l| l.id == listener_id) {
                        if res.is_ok() {
                            let l = &mut q.listeners_mut()[pos];
                            l.failure_count = 0;
                            l.unreachable_since = None;
                        } else {
                            let l = &mut q.listeners_mut()[pos];
                            l.failure_count += 1;
                            let since = *l.unreachable_since.get_or_insert(now);
                            let is_exhausted = l.failure_count >= l.max_retries
                                || now.duration_since(since) >= l.exhaustion_timeout;
                            if is_exhausted {
                                let evicted = q.listeners()[pos].clone();
                                q.detach_listener(&listener_id);
                                log::warn(&format!(
                                    "client unreachable exhaustion: evicted listener {} on {}:{} for queue '{}' (retries={})",
                                    evicted.id, evicted.host, evicted.port, queue_name, evicted.failure_count
                                ));
                            }
                        }
                    }
                }

                if !any_succeeded {
                    break;
                }
            }
            QueueMode::Worker => {
                let mut attempts = 0;
                let max_attempts = listeners_snapshot.len();
                let mut delivered = false;

                while attempts < max_attempts && !delivered {
                    attempts += 1;
                    let candidate = {
                        let mut q = match queue_arc.lock() {
                            Ok(q) => q,
                            Err(_) => break,
                        };
                        match q.next_round_robin_listener() {
                            Some(l) => l,
                            None => break,
                        }
                    };

                    let timeout = Duration::from_millis(2000).min(candidate.exhaustion_timeout);
                    let res = deliver_event(
                        &candidate.host,
                        candidate.port,
                        queue_name,
                        event_id,
                        &event_val,
                        timeout,
                    );

                    let mut q = match queue_arc.lock() {
                        Ok(q) => q,
                        Err(_) => break,
                    };

                    if res.is_ok() {
                        q.remove_event(&event_id);
                        log::info("dispatched event to worker listener");
                        if let Some(pos) =
                            q.listeners_mut().iter().position(|l| l.id == candidate.id)
                        {
                            let l = &mut q.listeners_mut()[pos];
                            l.failure_count = 0;
                            l.unreachable_since = None;
                        }
                        delivered = true;
                    } else {
                        let now = Instant::now();
                        if let Some(pos) =
                            q.listeners_mut().iter().position(|l| l.id == candidate.id)
                        {
                            let l = &mut q.listeners_mut()[pos];
                            l.failure_count += 1;
                            let since = *l.unreachable_since.get_or_insert(now);
                            let is_exhausted = l.failure_count >= l.max_retries
                                || now.duration_since(since) >= l.exhaustion_timeout;
                            if is_exhausted {
                                let evicted = q.listeners_mut().remove(pos);
                                log::warn(&format!(
                                    "client unreachable exhaustion: evicted listener {} on {}:{} for queue '{}' (retries={})",
                                    evicted.id, evicted.host, evicted.port, queue_name, evicted.failure_count
                                ));
                            }
                        }
                    }
                }

                if !delivered {
                    break;
                }
            }
        }
    }
}

fn dispatcher_loop(
    queues: Arc<Mutex<HashMap<String, Arc<Mutex<Queue>>>>>,
    notify: Arc<(Mutex<bool>, Condvar)>,
    shutdown: Arc<AtomicBool>,
) {
    while !shutdown.load(Ordering::SeqCst) {
        {
            let (lock, cvar) = &*notify;
            if let Ok(mut wake) = lock.lock() {
                if !*wake {
                    let _ = cvar.wait_timeout(wake, Duration::from_millis(50));
                } else {
                    *wake = false;
                }
            }
        }

        if shutdown.load(Ordering::SeqCst) {
            break;
        }

        let queue_entries: Vec<(String, Arc<Mutex<Queue>>)> = {
            let map = match queues.lock() {
                Ok(m) => m,
                Err(_) => continue,
            };
            map.iter()
                .map(|(k, v)| (k.clone(), Arc::clone(v)))
                .collect()
        };

        for (name, q_arc) in queue_entries {
            dispatch_queue(&name, &q_arc);
        }
    }
}

pub struct QueueManager {
    queues: Arc<Mutex<HashMap<String, Arc<Mutex<Queue>>>>>,
    dispatcher_notify: Arc<(Mutex<bool>, Condvar)>,
    shutdown: Arc<AtomicBool>,
    dispatcher_handle: Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl QueueManager {
    pub fn new() -> Self {
        let mut map = HashMap::new();
        map.insert(
            "demo".to_string(),
            Arc::new(Mutex::new(Queue::new(
                QueueMode::Broadcast,
                DispatchStrategy::RoundRobin,
            ))),
        );
        let queues = Arc::new(Mutex::new(map));
        let dispatcher_notify = Arc::new((Mutex::new(false), Condvar::new()));
        let shutdown = Arc::new(AtomicBool::new(false));

        let q_clone = Arc::clone(&queues);
        let n_clone = Arc::clone(&dispatcher_notify);
        let s_clone = Arc::clone(&shutdown);

        let handle = std::thread::Builder::new()
            .name("airbus-dispatcher".to_string())
            .spawn(move || {
                dispatcher_loop(q_clone, n_clone, s_clone);
            })
            .expect("spawn dispatcher thread");

        Self {
            queues,
            dispatcher_notify,
            shutdown,
            dispatcher_handle: Mutex::new(Some(handle)),
        }
    }

    fn wake_dispatcher(&self) {
        let (lock, cvar) = &*self.dispatcher_notify;
        if let Ok(mut wake) = lock.lock() {
            *wake = true;
            cvar.notify_all();
        }
    }

    pub fn create_queue(
        &self,
        queue_name: &str,
        mode: QueueMode,
        strategy: DispatchStrategy,
    ) -> Result<bool, String> {
        let mut map = self.queues.lock().map_err(|_| "queue manager poisoned")?;
        if map.contains_key(queue_name) {
            Ok(false)
        } else {
            map.insert(
                queue_name.to_string(),
                Arc::new(Mutex::new(Queue::new(mode, strategy))),
            );
            Ok(true)
        }
    }

    pub fn get_queue_mode(&self, queue_name: &str) -> Option<QueueMode> {
        let q = {
            let map = self.queues.lock().ok()?;
            map.get(queue_name)?.clone()
        };
        let guard = q.lock().ok()?;
        Some(guard.mode())
    }

    pub fn publish(&self, queue_name: &str, event_id: UuidV7, value: Value) -> Result<(), String> {
        let queue = {
            let map = self.queues.lock().map_err(|_| "queue manager poisoned")?;
            map.get(queue_name)
                .ok_or_else(|| format!("Queue not found: {queue_name}"))?
                .clone()
        };
        {
            let mut q = queue.lock().map_err(|_| "queue poisoned")?;
            q.publish(event_id, value)?;
        }
        self.wake_dispatcher();
        Ok(())
    }

    pub fn consume(&self, queue_name: &str, event_count: i64) -> Vec<Value> {
        let queue = {
            let map = match self.queues.lock() {
                Ok(m) => m,
                Err(_) => return Vec::new(),
            };
            match map.get(queue_name) {
                Some(q) => q.clone(),
                None => return Vec::new(),
            }
        };
        let result = match queue.lock() {
            Ok(mut q) => q.consume(event_count),
            Err(_) => Vec::new(),
        };
        result
    }

    pub fn list(&self) -> Vec<(String, usize, QueueMode, usize)> {
        let map = match self.queues.lock() {
            Ok(m) => m,
            Err(_) => return Vec::new(),
        };
        let mut out: Vec<(String, usize, QueueMode, usize)> = map
            .iter()
            .filter_map(|(name, queue)| {
                let q = queue.lock().ok()?;
                Some((name.clone(), q.depth(), q.mode(), q.listener_count()))
            })
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    pub fn peek(&self, queue_name: &str, event_count: i64) -> Vec<(UuidV7, Value)> {
        let queue = {
            let map = match self.queues.lock() {
                Ok(m) => m,
                Err(_) => return Vec::new(),
            };
            match map.get(queue_name) {
                Some(q) => q.clone(),
                None => return Vec::new(),
            }
        };
        let result = match queue.lock() {
            Ok(q) => q.peek(event_count),
            Err(_) => Vec::new(),
        };
        result
    }

    pub fn attach_listener(
        &self,
        queue_name: &str,
        listener_id: UuidV7,
        host: String,
        port: u16,
        max_retries: u64,
        exhaustion_timeout: Duration,
    ) -> Result<(), String> {
        let queue = {
            let map = self.queues.lock().map_err(|_| "queue manager poisoned")?;
            map.get(queue_name)
                .ok_or_else(|| format!("Queue not found: {queue_name}"))?
                .clone()
        };
        {
            let mut q = queue.lock().map_err(|_| "queue poisoned")?;
            q.attach_listener(ListenerRegistration::new(
                listener_id,
                host,
                port,
                max_retries,
                exhaustion_timeout,
            ));
        }
        self.wake_dispatcher();
        Ok(())
    }

    pub fn detach_listener(&self, listener_id: &UuidV7) -> bool {
        let map = match self.queues.lock() {
            Ok(m) => m,
            Err(_) => return false,
        };
        for queue in map.values() {
            if let Ok(mut q) = queue.lock() {
                if q.detach_listener(listener_id) {
                    return true;
                }
            }
        }
        false
    }

    pub fn list_listeners(&self, filter_queue: Option<&str>) -> Vec<ListenerInfo> {
        let map = match self.queues.lock() {
            Ok(m) => m,
            Err(_) => return Vec::new(),
        };
        let mut out = Vec::new();
        for (name, queue) in map.iter() {
            if let Some(fq) = filter_queue {
                if name != fq {
                    continue;
                }
            }
            if let Ok(q) = queue.lock() {
                let mode = q.mode();
                for l in q.listeners() {
                    out.push(ListenerInfo {
                        id: l.id,
                        queue: name.clone(),
                        host: l.host.clone(),
                        port: l.port,
                        mode,
                        failure_count: l.failure_count,
                        active: l.failure_count == 0,
                    });
                }
            }
        }
        out.sort_by_key(|a| a.id);
        out
    }
}

impl Drop for QueueManager {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        let (lock, cvar) = &*self.dispatcher_notify;
        if let Ok(mut wake) = lock.lock() {
            *wake = true;
            cvar.notify_all();
        }
        if let Ok(mut handle) = self.dispatcher_handle.lock() {
            if let Some(h) = handle.take() {
                let _ = h.join();
            }
        }
    }
}

impl Default for QueueManager {
    fn default() -> Self {
        Self::new()
    }
}
