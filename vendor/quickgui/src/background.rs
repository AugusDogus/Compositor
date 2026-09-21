use std::{
    any::Any,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{Arc, Mutex, mpsc},
    thread,
};

use thiserror::Error;
use winit::event_loop::EventLoopProxy;

use crate::{EventContext, WindowHandle, runtime::RuntimeEvent};

/// Maximum application jobs waiting behind the fixed background worker set.
pub const MAX_PENDING_BACKGROUND_TASKS: usize = 64;

const BACKGROUND_WORKER_THREADS: usize = 2;

/// A failure to enqueue application work without blocking the UI thread.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum TaskSpawnError {
    #[error("the bounded background task queue is full")]
    QueueFull,
    #[error("the background worker pool is unavailable")]
    Unavailable,
}

/// A background operation failed before producing its value.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum BackgroundTaskError {
    #[error("the background operation panicked")]
    Panicked,
}

pub(crate) type CompletionCallback =
    Box<dyn FnOnce(&mut dyn Any, &mut EventContext) + Send + 'static>;

pub(crate) struct BackgroundCompletion {
    pub(crate) window: WindowHandle,
    pub(crate) callback: CompletionCallback,
}

type BackgroundJob = Box<dyn FnOnce() + Send + 'static>;

struct BackgroundWorkerPool {
    sender: mpsc::SyncSender<BackgroundJob>,
    // Dropping these handles detaches the workers. Shutdown must not wait for application code
    // that may be blocked in a filesystem or network operation.
    _workers: Vec<thread::JoinHandle<()>>,
}

enum SharedWorkerState {
    Dormant,
    Ready(BackgroundWorkerPool),
    Failed,
}

/// Lazily-created, bounded application worker pool shared by every window.
#[derive(Clone)]
pub(crate) struct BackgroundTaskPoolHandle {
    proxy: EventLoopProxy<RuntimeEvent>,
    state: Arc<Mutex<SharedWorkerState>>,
}

impl BackgroundTaskPoolHandle {
    pub(crate) fn new(proxy: EventLoopProxy<RuntimeEvent>) -> Self {
        Self {
            proxy,
            state: Arc::new(Mutex::new(SharedWorkerState::Dormant)),
        }
    }

    pub(crate) fn spawn<V, T, Work, Complete>(
        &self,
        window: WindowHandle,
        work: Work,
        complete: Complete,
    ) -> Result<(), TaskSpawnError>
    where
        V: 'static,
        T: Send + 'static,
        Work: FnOnce() -> T + Send + 'static,
        Complete:
            FnOnce(&mut V, Result<T, BackgroundTaskError>, &mut EventContext) + Send + 'static,
    {
        let proxy = self.proxy.clone();
        let job = Box::new(move || {
            let result =
                catch_unwind(AssertUnwindSafe(work)).map_err(|_| BackgroundTaskError::Panicked);
            let callback: CompletionCallback = Box::new(move |view, context| {
                let view = view
                    .downcast_mut::<V>()
                    .expect("background completion received the wrong view type");
                complete(view, result, context);
            });
            let _ = proxy.send_event(RuntimeEvent::BackgroundCompleted(BackgroundCompletion {
                window,
                callback,
            }));
        });
        self.try_spawn(job)
    }

    fn try_spawn(&self, job: BackgroundJob) -> Result<(), TaskSpawnError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if matches!(*state, SharedWorkerState::Dormant) {
            *state = match BackgroundWorkerPool::new() {
                Ok(workers) => SharedWorkerState::Ready(workers),
                Err(error) => {
                    tracing::warn!(%error, "background workers could not be started");
                    SharedWorkerState::Failed
                }
            };
        }
        match &*state {
            SharedWorkerState::Ready(workers) => workers.try_spawn(job),
            SharedWorkerState::Dormant => unreachable!("the worker pool was initialized above"),
            SharedWorkerState::Failed => Err(TaskSpawnError::Unavailable),
        }
    }

    pub(crate) fn shutdown(&self) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *state = SharedWorkerState::Failed;
    }
}

impl BackgroundWorkerPool {
    fn new() -> std::io::Result<Self> {
        let (sender, receiver) = mpsc::sync_channel(MAX_PENDING_BACKGROUND_TASKS);
        let receiver = Arc::new(Mutex::new(receiver));
        let workers = (0..BACKGROUND_WORKER_THREADS)
            .map(|index| {
                let receiver = receiver.clone();
                thread::Builder::new()
                    .name(format!("quickgui-task-{index}"))
                    .spawn(move || background_worker(receiver))
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            sender,
            _workers: workers,
        })
    }

    fn try_spawn(&self, job: BackgroundJob) -> Result<(), TaskSpawnError> {
        self.sender.try_send(job).map_err(|error| match error {
            mpsc::TrySendError::Full(_) => TaskSpawnError::QueueFull,
            mpsc::TrySendError::Disconnected(_) => TaskSpawnError::Unavailable,
        })
    }
}

fn background_worker(receiver: Arc<Mutex<mpsc::Receiver<BackgroundJob>>>) {
    loop {
        let job = {
            let receiver = receiver
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            receiver.recv()
        };
        let Ok(job) = job else {
            return;
        };
        // Each public job catches its own application panic so the typed failure reaches the UI.
        // This outer guard also keeps a worker alive if internal completion plumbing ever panics.
        if catch_unwind(AssertUnwindSafe(job)).is_err() {
            tracing::error!("background task completion panicked");
        }
    }
}
