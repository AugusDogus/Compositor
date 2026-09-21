use std::{
    any::Any,
    cell::{Cell, RefCell},
    collections::{BTreeSet, HashMap, HashSet, VecDeque},
    fmt,
    future::Future,
    marker::PhantomData,
    mem,
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    pin::Pin,
    rc::{Rc, Weak},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll, Waker},
    thread::{self, ThreadId},
};
use web_time::{Duration, Instant};

use thiserror::Error;
use winit::event_loop::EventLoopProxy;

use crate::{EventContext, WindowHandle, runtime::RuntimeEvent};

/// Maximum live foreground futures owned by one native window.
pub const MAX_FOREGROUND_TASKS_PER_WINDOW: usize = 1_024;
/// Maximum live foreground futures owned by one application.
pub const MAX_FOREGROUND_TASKS_PER_APPLICATION: usize = 4_096;
/// Maximum foreground futures polled from one event-loop wake before yielding to platform input.
pub const MAX_FOREGROUND_POLLS_PER_TURN: usize = 256;
/// Maximum UI updates one foreground future may have queued at once.
pub const MAX_FOREGROUND_UPDATES_PER_TASK: usize = 64;
/// Maximum exact timers one foreground future may await concurrently.
pub const MAX_FOREGROUND_TIMERS_PER_TASK: usize = 64;
/// Maximum exact foreground timers retained by one application.
pub const MAX_FOREGROUND_TIMERS_PER_APPLICATION: usize = 4_096;

/// A foreground future could not be scheduled without exceeding a hard ownership bound.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum ForegroundTaskSpawnError {
    #[error("the window already owns {MAX_FOREGROUND_TASKS_PER_WINDOW} foreground tasks")]
    WindowCapacity,
    #[error("the application already owns {MAX_FOREGROUND_TASKS_PER_APPLICATION} foreground tasks")]
    ApplicationCapacity,
    #[error("the foreground executor is shutting down")]
    Unavailable,
}

/// A fallible operation requested through [`AsyncViewContext`].
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum AsyncContextError {
    #[error("the foreground task or its owning window no longer exists")]
    TaskEnded,
    #[error("the foreground task already has {MAX_FOREGROUND_UPDATES_PER_TASK} pending UI updates")]
    UpdateQueueFull,
    #[error("the foreground timer capacity has been reached")]
    TimerCapacity,
    #[error("the foreground task was started for a different view type")]
    ViewTypeMismatch,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct ForegroundTaskId(u64);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
struct ForegroundTimerId(u64);

/// A running foreground future.
///
/// The future starts immediately on QuickGUI's UI-thread executor. Dropping this handle requests
/// cancellation; [`Self::detach`] instead lets it continue until completion or until its owning
/// window closes. Like GPUI's task handle, it can also be awaited by another foreground future.
#[must_use = "dropping a Task cancels it; store it, await it, or call detach()"]
pub struct Task<T> {
    inner: Option<async_task::Task<Option<T>>>,
    abort: Arc<AbortState>,
    not_send: PhantomData<Rc<()>>,
}

impl<T> Task<T> {
    /// Let the future continue without retaining a result handle.
    ///
    /// Detached view tasks are still cancelled when their owning window closes.
    pub fn detach(mut self) {
        if let Some(task) = self.inner.take() {
            task.detach();
        }
    }

    /// Request cancellation immediately.
    pub fn cancel(mut self) {
        self.abort.abort();
        self.inner.take();
    }

    pub fn is_finished(&self) -> bool {
        self.inner
            .as_ref()
            .is_none_or(async_task::Task::is_finished)
    }

    pub fn is_cancelled(&self) -> bool {
        self.abort.is_aborted()
    }
}

impl<T> Drop for Task<T> {
    fn drop(&mut self) {
        if self.inner.is_some() {
            self.abort.abort();
        }
    }
}

impl<T: 'static> Future for Task<T> {
    type Output = T;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let task = self
            .inner
            .as_mut()
            .expect("a detached or explicitly cancelled task cannot be awaited");
        match Pin::new(task).poll(cx) {
            Poll::Ready(Some(output)) => Poll::Ready(output),
            Poll::Ready(None) => panic!("foreground task was cancelled before producing a value"),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl<T> fmt::Debug for Task<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Task")
            .field("finished", &self.is_finished())
            .field("cancelled", &self.is_cancelled())
            .finish_non_exhaustive()
    }
}

#[derive(Default)]
struct AbortState {
    aborted: AtomicBool,
    waker: Mutex<Option<Waker>>,
}

impl AbortState {
    fn abort(&self) {
        if self.aborted.swap(true, Ordering::AcqRel) {
            return;
        }
        if let Some(waker) = self
            .waker
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
        {
            waker.wake();
        }
    }

    fn is_aborted(&self) -> bool {
        self.aborted.load(Ordering::Acquire)
    }

    fn register(&self, waker: &Waker) {
        let mut slot = self
            .waker
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if slot
            .as_ref()
            .is_none_or(|current| !current.will_wake(waker))
        {
            *slot = Some(waker.clone());
        }
    }

    fn clear_waker(&self) {
        self.waker
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
    }
}

struct ManagedFuture<F> {
    future: Pin<Box<F>>,
    abort: Arc<AbortState>,
    _finish: ForegroundTaskFinishGuard,
}

impl<F: Future> Future for ManagedFuture<F> {
    type Output = Option<F::Output>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        if this.abort.is_aborted() {
            this.abort.clear_waker();
            return Poll::Ready(None);
        }
        this.abort.register(cx.waker());
        if this.abort.is_aborted() {
            this.abort.clear_waker();
            return Poll::Ready(None);
        }
        match this.future.as_mut().poll(cx) {
            Poll::Ready(output) => {
                this.abort.clear_waker();
                Poll::Ready(Some(output))
            }
            Poll::Pending if this.abort.is_aborted() => {
                this.abort.clear_waker();
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

struct ForegroundTaskFinishGuard {
    task: ForegroundTaskId,
    registry: Weak<RefCell<ForegroundTaskRegistry>>,
}

impl Drop for ForegroundTaskFinishGuard {
    fn drop(&mut self) {
        if let Some(registry) = self.registry.upgrade() {
            registry.borrow_mut().finish(self.task);
        }
    }
}

pub(crate) type QueuedViewUpdate = Box<dyn FnOnce(&mut dyn Any, &mut EventContext)>;

struct ForegroundTaskMetadata {
    window: Option<WindowHandle>,
    abort: Arc<AbortState>,
    updates: VecDeque<QueuedViewUpdate>,
    timers: HashSet<ForegroundTimerId>,
}

struct ForegroundTimerEntry {
    task: ForegroundTaskId,
    state: Weak<ForegroundTimerState>,
}

#[derive(Default)]
struct ForegroundTaskRegistry {
    next_task: u64,
    next_timer: u64,
    tasks: HashMap<ForegroundTaskId, ForegroundTaskMetadata>,
    tasks_per_window: HashMap<Option<WindowHandle>, usize>,
    timer_deadlines: BTreeSet<(Instant, ForegroundTimerId)>,
    timers: HashMap<ForegroundTimerId, ForegroundTimerEntry>,
    shutting_down: bool,
}

impl ForegroundTaskRegistry {
    fn reserve(
        &mut self,
        window: impl Into<Option<WindowHandle>>,
    ) -> Result<(ForegroundTaskId, Arc<AbortState>), ForegroundTaskSpawnError> {
        let window = window.into();
        if self.shutting_down {
            return Err(ForegroundTaskSpawnError::Unavailable);
        }
        self.prune_aborted();
        if self.tasks.len() == MAX_FOREGROUND_TASKS_PER_APPLICATION {
            return Err(ForegroundTaskSpawnError::ApplicationCapacity);
        }
        if self.tasks_per_window.get(&window).copied().unwrap_or(0)
            == MAX_FOREGROUND_TASKS_PER_WINDOW
        {
            return Err(ForegroundTaskSpawnError::WindowCapacity);
        }
        self.next_task = self.next_task.wrapping_add(1).max(1);
        let task = ForegroundTaskId(self.next_task);
        let abort = Arc::new(AbortState::default());
        self.tasks.insert(
            task,
            ForegroundTaskMetadata {
                window,
                abort: abort.clone(),
                updates: VecDeque::new(),
                timers: HashSet::new(),
            },
        );
        *self.tasks_per_window.entry(window).or_default() += 1;
        Ok((task, abort))
    }

    fn prune_aborted(&mut self) {
        let aborted = self
            .tasks
            .iter()
            .filter_map(|(task, metadata)| metadata.abort.is_aborted().then_some(*task))
            .collect::<Vec<_>>();
        for task in aborted {
            self.remove_task(task, false);
        }
    }

    fn finish(&mut self, task: ForegroundTaskId) {
        self.remove_task(task, false);
    }

    fn cancel_task(&mut self, task: ForegroundTaskId) {
        self.remove_task(task, true);
    }

    fn cancel_window(&mut self, window: WindowHandle) {
        let tasks = self
            .tasks
            .iter()
            .filter_map(|(task, metadata)| (metadata.window == Some(window)).then_some(*task))
            .collect::<Vec<_>>();
        for task in tasks {
            self.remove_task(task, true);
        }
    }

    fn shutdown(&mut self) {
        self.shutting_down = true;
        let tasks = self.tasks.keys().copied().collect::<Vec<_>>();
        for task in tasks {
            self.remove_task(task, true);
        }
    }

    fn remove_task(&mut self, task: ForegroundTaskId, abort: bool) {
        let Some(metadata) = self.tasks.remove(&task) else {
            return;
        };
        if abort {
            metadata.abort.abort();
        }
        for timer in metadata.timers {
            self.remove_timer_entry(timer);
        }
        if let Some(count) = self.tasks_per_window.get_mut(&metadata.window) {
            *count -= 1;
            if *count == 0 {
                self.tasks_per_window.remove(&metadata.window);
            }
        }
    }

    fn owns(&self, task: ForegroundTaskId, window: impl Into<Option<WindowHandle>>) -> bool {
        let window = window.into();
        self.tasks
            .get(&task)
            .is_some_and(|metadata| metadata.window == window && !metadata.abort.is_aborted())
    }

    fn enqueue_update(
        &mut self,
        task: ForegroundTaskId,
        update: QueuedViewUpdate,
    ) -> Result<(), AsyncContextError> {
        let Some(metadata) = self.tasks.get_mut(&task) else {
            return Err(AsyncContextError::TaskEnded);
        };
        if metadata.abort.is_aborted() {
            return Err(AsyncContextError::TaskEnded);
        }
        if metadata.updates.len() == MAX_FOREGROUND_UPDATES_PER_TASK {
            return Err(AsyncContextError::UpdateQueueFull);
        }
        metadata.updates.push_back(update);
        Ok(())
    }

    fn take_updates(&mut self, task: ForegroundTaskId) -> VecDeque<QueuedViewUpdate> {
        self.tasks
            .get_mut(&task)
            .map(|metadata| mem::take(&mut metadata.updates))
            .unwrap_or_default()
    }

    fn register_timer(
        &mut self,
        task: ForegroundTaskId,
        deadline: Instant,
        state: &Rc<ForegroundTimerState>,
    ) -> Result<ForegroundTimerId, AsyncContextError> {
        if self.timers.len() == MAX_FOREGROUND_TIMERS_PER_APPLICATION {
            return Err(AsyncContextError::TimerCapacity);
        }
        let Some(metadata) = self.tasks.get_mut(&task) else {
            return Err(AsyncContextError::TaskEnded);
        };
        if metadata.abort.is_aborted() {
            return Err(AsyncContextError::TaskEnded);
        }
        if metadata.timers.len() == MAX_FOREGROUND_TIMERS_PER_TASK {
            return Err(AsyncContextError::TimerCapacity);
        }
        self.next_timer = self.next_timer.wrapping_add(1).max(1);
        let timer = ForegroundTimerId(self.next_timer);
        metadata.timers.insert(timer);
        self.timer_deadlines.insert((deadline, timer));
        self.timers.insert(
            timer,
            ForegroundTimerEntry {
                task,
                state: Rc::downgrade(state),
            },
        );
        Ok(timer)
    }

    fn cancel_timer(&mut self, timer: ForegroundTimerId) {
        let Some(entry) = self.timers.get(&timer) else {
            return;
        };
        if let Some(metadata) = self.tasks.get_mut(&entry.task) {
            metadata.timers.remove(&timer);
        }
        self.remove_timer_entry(timer);
    }

    fn remove_timer_entry(&mut self, timer: ForegroundTimerId) {
        let Some(entry) = self.timers.remove(&timer) else {
            return;
        };
        let deadline = entry.state.upgrade().map(|state| state.deadline);
        if let Some(deadline) = deadline {
            self.timer_deadlines.remove(&(deadline, timer));
        } else {
            self.timer_deadlines
                .retain(|(_, candidate)| *candidate != timer);
        }
    }

    fn wake_due_timers(&mut self, now: Instant) {
        let due = self
            .timer_deadlines
            .range(..=(now, ForegroundTimerId(u64::MAX)))
            .copied()
            .collect::<Vec<_>>();
        let mut wakers = Vec::with_capacity(due.len());
        for (deadline, timer) in due {
            self.timer_deadlines.remove(&(deadline, timer));
            let Some(entry) = self.timers.remove(&timer) else {
                continue;
            };
            if let Some(metadata) = self.tasks.get_mut(&entry.task) {
                metadata.timers.remove(&timer);
            }
            if let Some(state) = entry.state.upgrade() {
                state.ready.set(true);
                if let Some(waker) = state.waker.borrow_mut().take() {
                    wakers.push(waker);
                }
            }
        }
        for waker in wakers {
            waker.wake();
        }
    }

    fn next_timer_deadline(&self) -> Option<Instant> {
        self.timer_deadlines.first().map(|(deadline, _)| *deadline)
    }
}

pub(crate) struct ScheduledForegroundTask {
    pub(crate) task: ForegroundTaskId,
    pub(crate) window: Option<WindowHandle>,
    pub(crate) runnable: async_task::Runnable,
}

struct ForegroundReadyQueue {
    queue: Mutex<VecDeque<ScheduledForegroundTask>>,
    wake_pending: AtomicBool,
    closed: AtomicBool,
    owner_thread: ThreadId,
    wake: ForegroundWake,
}

enum ForegroundWake {
    EventLoop(EventLoopProxy<RuntimeEvent>),
    #[cfg(any(test, feature = "test-support"))]
    Test,
}

#[derive(Clone)]
enum ForegroundClock {
    System,
    #[cfg(any(test, feature = "test-support"))]
    Controlled(Rc<Cell<Instant>>),
}

impl ForegroundClock {
    fn system() -> Self {
        Self::System
    }

    #[cfg(any(test, feature = "test-support"))]
    fn controlled(now: Rc<Cell<Instant>>) -> Self {
        Self::Controlled(now)
    }

    fn now(&self) -> Instant {
        match self {
            Self::System => Instant::now(),
            #[cfg(any(test, feature = "test-support"))]
            Self::Controlled(now) => now.get(),
        }
    }
}

impl ForegroundReadyQueue {
    fn new(proxy: EventLoopProxy<RuntimeEvent>) -> Arc<Self> {
        Self::with_wake(ForegroundWake::EventLoop(proxy))
    }

    fn with_wake(wake: ForegroundWake) -> Arc<Self> {
        Arc::new(Self {
            queue: Mutex::new(VecDeque::with_capacity(8)),
            wake_pending: AtomicBool::new(false),
            closed: AtomicBool::new(false),
            owner_thread: thread::current().id(),
            wake,
        })
    }

    fn schedule(&self, scheduled: ScheduledForegroundTask) {
        if self.closed.load(Ordering::Acquire) {
            self.dispose(scheduled);
            return;
        }
        {
            let mut queue = self
                .queue
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if queue.len() == MAX_FOREGROUND_TASKS_PER_APPLICATION {
                drop(queue);
                self.dispose(scheduled);
                return;
            }
            queue.push_back(scheduled);
        }
        self.signal();
    }

    fn signal(&self) {
        if self.wake_pending.swap(true, Ordering::AcqRel) {
            return;
        }
        let signaled = match &self.wake {
            ForegroundWake::EventLoop(proxy) => {
                proxy.send_event(RuntimeEvent::ForegroundTasksReady).is_ok()
            }
            #[cfg(any(test, feature = "test-support"))]
            ForegroundWake::Test => true,
        };
        if !signaled {
            self.wake_pending.store(false, Ordering::Release);
        }
    }

    fn take_batch(&self) -> VecDeque<ScheduledForegroundTask> {
        self.wake_pending.store(false, Ordering::Release);
        let (batch, has_more) = {
            let mut queue = self
                .queue
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let count = queue.len().min(MAX_FOREGROUND_POLLS_PER_TURN);
            let batch = queue.drain(..count).collect::<VecDeque<_>>();
            (batch, !queue.is_empty())
        };
        if has_more {
            self.signal();
        }
        batch
    }

    fn close_and_drain(&self) {
        self.closed.store(true, Ordering::Release);
        self.wake_pending.store(false, Ordering::Release);
        let scheduled = self
            .queue
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .drain(..)
            .collect::<Vec<_>>();
        debug_assert_eq!(thread::current().id(), self.owner_thread);
        drop(scheduled);
    }

    fn dispose(&self, scheduled: ScheduledForegroundTask) {
        if thread::current().id() == self.owner_thread {
            drop(scheduled);
        } else {
            // A local future must be destroyed on the application thread. At shutdown that thread
            // no longer accepts work, so leaking this already-bounded allocation is safer than
            // dropping application `Rc` state on an arbitrary wake thread.
            mem::forget(scheduled);
        }
    }
}

/// Main-thread task scheduler shared by all windows in one application.
#[derive(Clone)]
pub(crate) struct ForegroundTaskSpawner {
    registry: Rc<RefCell<ForegroundTaskRegistry>>,
    ready: Arc<ForegroundReadyQueue>,
    clock: ForegroundClock,
}

impl ForegroundTaskSpawner {
    pub(crate) fn new(proxy: EventLoopProxy<RuntimeEvent>) -> Self {
        Self {
            registry: Rc::new(RefCell::new(ForegroundTaskRegistry::default())),
            ready: ForegroundReadyQueue::new(proxy),
            clock: ForegroundClock::system(),
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn new_for_test(now: Rc<Cell<Instant>>) -> Self {
        Self {
            registry: Rc::new(RefCell::new(ForegroundTaskRegistry::default())),
            // Tests explicitly drain the ready queue. Recording the coalesced wake as successful
            // preserves the production queue contract without creating an OS event loop.
            ready: ForegroundReadyQueue::with_wake(ForegroundWake::Test),
            clock: ForegroundClock::controlled(now),
        }
    }

    pub(crate) fn spawn<V, Build, Fut, R>(
        &self,
        window: WindowHandle,
        build: Build,
    ) -> Result<Task<R>, ForegroundTaskSpawnError>
    where
        V: 'static,
        Build: FnOnce(AsyncViewContext<V>) -> Fut,
        Fut: Future<Output = R> + 'static,
        R: 'static,
    {
        if self.ready.closed.load(Ordering::Acquire) {
            return Err(ForegroundTaskSpawnError::Unavailable);
        }
        let (task_id, abort) = self.registry.borrow_mut().reserve(window)?;
        let async_context = AsyncViewContext {
            task: task_id,
            window,
            registry: Rc::downgrade(&self.registry),
            clock: self.clock.clone(),
            marker: PhantomData,
        };
        let future = match catch_unwind(AssertUnwindSafe(|| build(async_context))) {
            Ok(future) => future,
            Err(panic) => {
                self.registry.borrow_mut().cancel_task(task_id);
                resume_unwind(panic);
            }
        };
        let managed = ManagedFuture {
            future: Box::pin(future),
            abort: abort.clone(),
            _finish: ForegroundTaskFinishGuard {
                task: task_id,
                registry: Rc::downgrade(&self.registry),
            },
        };
        let ready = self.ready.clone();
        let (runnable, task) = async_task::spawn_local(managed, move |runnable| {
            ready.schedule(ScheduledForegroundTask {
                task: task_id,
                window: Some(window),
                runnable,
            });
        });
        runnable.schedule();
        Ok(Task {
            inner: Some(task),
            abort,
            not_send: PhantomData,
        })
    }

    #[cfg(any(
        target_os = "windows",
        target_os = "linux",
        target_os = "freebsd",
        target_os = "dragonfly",
        target_os = "openbsd",
        target_os = "netbsd"
    ))]
    pub(crate) fn spawn_application<Fut, R>(
        &self,
        future: Fut,
    ) -> Result<Task<R>, ForegroundTaskSpawnError>
    where
        Fut: Future<Output = R> + 'static,
        R: 'static,
    {
        if self.ready.closed.load(Ordering::Acquire) {
            return Err(ForegroundTaskSpawnError::Unavailable);
        }
        let (task_id, abort) = self.registry.borrow_mut().reserve(None)?;
        let managed = ManagedFuture {
            future: Box::pin(future),
            abort: abort.clone(),
            _finish: ForegroundTaskFinishGuard {
                task: task_id,
                registry: Rc::downgrade(&self.registry),
            },
        };
        let ready = self.ready.clone();
        let (runnable, task) = async_task::spawn_local(managed, move |runnable| {
            ready.schedule(ScheduledForegroundTask {
                task: task_id,
                window: None,
                runnable,
            });
        });
        runnable.schedule();
        Ok(Task {
            inner: Some(task),
            abort,
            not_send: PhantomData,
        })
    }

    pub(crate) fn take_ready_batch(&self) -> VecDeque<ScheduledForegroundTask> {
        self.ready.take_batch()
    }

    pub(crate) fn owns(
        &self,
        task: ForegroundTaskId,
        window: impl Into<Option<WindowHandle>>,
    ) -> bool {
        self.registry.borrow().owns(task, window)
    }

    pub(crate) fn take_updates(&self, task: ForegroundTaskId) -> VecDeque<QueuedViewUpdate> {
        self.registry.borrow_mut().take_updates(task)
    }

    pub(crate) fn cancel_task(&self, task: ForegroundTaskId) {
        self.registry.borrow_mut().cancel_task(task);
    }

    pub(crate) fn cancel_window(&self, window: WindowHandle) {
        self.registry.borrow_mut().cancel_window(window);
    }

    pub(crate) fn wake_due_timers(&self, now: Instant) {
        self.registry.borrow_mut().wake_due_timers(now);
    }

    pub(crate) fn next_timer_deadline(&self) -> Option<Instant> {
        self.registry.borrow().next_timer_deadline()
    }

    pub(crate) fn shutdown(&self) {
        self.registry.borrow_mut().shutdown();
        self.ready.close_and_drain();
    }
}

impl fmt::Debug for ForegroundTaskSpawner {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let registry = self.registry.borrow();
        formatter
            .debug_struct("ForegroundTaskSpawner")
            .field("tasks", &registry.tasks.len())
            .field("timers", &registry.timers.len())
            .finish_non_exhaustive()
    }
}

/// Owned, fallible access to one task's view across `await` points.
///
/// The context is main-thread-only. [`Self::update`] defers its callback until the current future
/// poll releases executor state; [`Self::sleep`] uses the application event loop's exact next
/// deadline rather than a timer thread or polling frame.
pub struct AsyncViewContext<V> {
    task: ForegroundTaskId,
    window: WindowHandle,
    registry: Weak<RefCell<ForegroundTaskRegistry>>,
    clock: ForegroundClock,
    marker: PhantomData<fn(&mut V)>,
}

impl<V> Clone for AsyncViewContext<V> {
    fn clone(&self) -> Self {
        Self {
            task: self.task,
            window: self.window,
            registry: self.registry.clone(),
            clock: self.clock.clone(),
            marker: PhantomData,
        }
    }
}

impl<V: 'static> AsyncViewContext<V> {
    pub fn window(&self) -> WindowHandle {
        self.window
    }

    /// Read the monotonic application clock used by this task's exact timers.
    ///
    /// Native applications receive the system monotonic clock. A deterministic
    /// `TestAppContext` supplies its controlled clock instead.
    pub fn now(&self) -> Instant {
        self.clock.now()
    }

    /// Update the owning view after the current future poll has returned.
    pub fn update<R>(
        &self,
        update: impl FnOnce(&mut V, &mut EventContext) -> R + 'static,
    ) -> AsyncViewUpdate<V, R>
    where
        R: 'static,
    {
        AsyncViewUpdate {
            task: self.task,
            registry: self.registry.clone(),
            update: Some(Box::new(update)),
            state: Rc::new(AsyncViewUpdateState::default()),
            queued: false,
        }
    }

    /// Sleep until one exact application event-loop deadline.
    pub fn sleep(&self, duration: Duration) -> ForegroundTimer {
        self.sleep_until(
            self.now()
                .checked_add(duration)
                .unwrap_or_else(|| self.now()),
        )
    }

    /// Sleep until an exact monotonic deadline.
    pub fn sleep_until(&self, deadline: Instant) -> ForegroundTimer {
        ForegroundTimer {
            task: self.task,
            registry: self.registry.clone(),
            clock: self.clock.clone(),
            state: Rc::new(ForegroundTimerState {
                deadline,
                ready: Cell::new(false),
                waker: RefCell::new(None),
            }),
            timer: None,
            finished: false,
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.registry
            .upgrade()
            .and_then(|registry| {
                registry
                    .borrow()
                    .tasks
                    .get(&self.task)
                    .map(|metadata| metadata.abort.is_aborted())
            })
            .unwrap_or(true)
    }
}

impl<V> fmt::Debug for AsyncViewContext<V> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AsyncViewContext")
            .field("task", &self.task)
            .field("window", &self.window)
            .finish_non_exhaustive()
    }
}

struct AsyncViewUpdateState<R> {
    active: Cell<bool>,
    result: RefCell<Option<Result<R, AsyncContextError>>>,
    waker: RefCell<Option<Waker>>,
}

type TypedViewUpdate<V, R> = Box<dyn FnOnce(&mut V, &mut EventContext) -> R>;

impl<R> Default for AsyncViewUpdateState<R> {
    fn default() -> Self {
        Self {
            active: Cell::new(true),
            result: RefCell::new(None),
            waker: RefCell::new(None),
        }
    }
}

/// Future returned by [`AsyncViewContext::update`].
pub struct AsyncViewUpdate<V, R> {
    task: ForegroundTaskId,
    registry: Weak<RefCell<ForegroundTaskRegistry>>,
    update: Option<TypedViewUpdate<V, R>>,
    state: Rc<AsyncViewUpdateState<R>>,
    queued: bool,
}

impl<V: 'static, R: 'static> Future for AsyncViewUpdate<V, R> {
    type Output = Result<R, AsyncContextError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        if let Some(result) = this.state.result.borrow_mut().take() {
            this.state.active.set(false);
            return Poll::Ready(result);
        }
        {
            let mut waker = this.state.waker.borrow_mut();
            if waker
                .as_ref()
                .is_none_or(|current| !current.will_wake(cx.waker()))
            {
                *waker = Some(cx.waker().clone());
            }
        }
        if this.queued {
            return Poll::Pending;
        }
        let Some(registry) = this.registry.upgrade() else {
            return Poll::Ready(Err(AsyncContextError::TaskEnded));
        };
        let update = this
            .update
            .take()
            .expect("an unqueued async view update retains its callback");
        let state = this.state.clone();
        let queued: QueuedViewUpdate = Box::new(move |view, cx| {
            if !state.active.get() {
                return;
            }
            let result = view
                .downcast_mut::<V>()
                .map(|view| Ok(update(view, cx)))
                .unwrap_or(Err(AsyncContextError::ViewTypeMismatch));
            *state.result.borrow_mut() = Some(result);
            if let Some(waker) = state.waker.borrow_mut().take() {
                waker.wake();
            }
        });
        match registry.borrow_mut().enqueue_update(this.task, queued) {
            Ok(()) => {
                this.queued = true;
                Poll::Pending
            }
            Err(error) => {
                this.state.active.set(false);
                Poll::Ready(Err(error))
            }
        }
    }
}

impl<V, R> Drop for AsyncViewUpdate<V, R> {
    fn drop(&mut self) {
        self.state.active.set(false);
        self.state.waker.borrow_mut().take();
    }
}

struct ForegroundTimerState {
    deadline: Instant,
    ready: Cell<bool>,
    waker: RefCell<Option<Waker>>,
}

/// Exact, event-loop-driven timer returned by [`AsyncViewContext::sleep`].
pub struct ForegroundTimer {
    task: ForegroundTaskId,
    registry: Weak<RefCell<ForegroundTaskRegistry>>,
    clock: ForegroundClock,
    state: Rc<ForegroundTimerState>,
    timer: Option<ForegroundTimerId>,
    finished: bool,
}

impl Future for ForegroundTimer {
    type Output = Result<(), AsyncContextError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        if this.state.ready.get() || this.clock.now() >= this.state.deadline {
            if let Some(timer) = this.timer.take()
                && let Some(registry) = this.registry.upgrade()
            {
                registry.borrow_mut().cancel_timer(timer);
            }
            this.finished = true;
            this.state.waker.borrow_mut().take();
            return Poll::Ready(Ok(()));
        }
        {
            let mut waker = this.state.waker.borrow_mut();
            if waker
                .as_ref()
                .is_none_or(|current| !current.will_wake(cx.waker()))
            {
                *waker = Some(cx.waker().clone());
            }
        }
        if this.timer.is_none() {
            let Some(registry) = this.registry.upgrade() else {
                this.finished = true;
                return Poll::Ready(Err(AsyncContextError::TaskEnded));
            };
            match registry
                .borrow_mut()
                .register_timer(this.task, this.state.deadline, &this.state)
            {
                Ok(timer) => this.timer = Some(timer),
                Err(error) => {
                    this.finished = true;
                    return Poll::Ready(Err(error));
                }
            }
        }
        Poll::Pending
    }
}

impl Drop for ForegroundTimer {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        self.state.waker.borrow_mut().take();
        if let Some(timer) = self.timer.take()
            && let Some(registry) = self.registry.upgrade()
        {
            registry.borrow_mut().cancel_timer(timer);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::task::Wake;

    #[derive(Default)]
    struct WakeCounter(AtomicBool);

    impl Wake for WakeCounter {
        fn wake(self: Arc<Self>) {
            self.0.store(true, Ordering::Release);
        }
    }

    fn test_waker() -> (Waker, Arc<WakeCounter>) {
        let counter = Arc::new(WakeCounter::default());
        (Waker::from(counter.clone()), counter)
    }

    #[test]
    fn task_capacity_reuses_aborted_slots_without_growing() {
        let window = WindowHandle::next();
        let mut registry = ForegroundTaskRegistry::default();
        let mut aborts = Vec::with_capacity(MAX_FOREGROUND_TASKS_PER_WINDOW);

        for _ in 0..MAX_FOREGROUND_TASKS_PER_WINDOW {
            let (_, abort) = registry.reserve(window).unwrap();
            aborts.push(abort);
        }
        assert_eq!(registry.tasks.len(), MAX_FOREGROUND_TASKS_PER_WINDOW);
        assert_eq!(
            registry.reserve(window).err(),
            Some(ForegroundTaskSpawnError::WindowCapacity)
        );

        aborts.pop().unwrap().abort();
        let _replacement = registry.reserve(window).unwrap();
        assert_eq!(registry.tasks.len(), MAX_FOREGROUND_TASKS_PER_WINDOW);
    }

    #[test]
    fn due_timers_wake_once_and_release_exact_registry_state() {
        let window = WindowHandle::next();
        let mut registry = ForegroundTaskRegistry::default();
        let (task, _) = registry.reserve(window).unwrap();
        let deadline = Instant::now() + Duration::from_millis(20);
        let state = Rc::new(ForegroundTimerState {
            deadline,
            ready: Cell::new(false),
            waker: RefCell::new(None),
        });
        let (waker, counter) = test_waker();
        *state.waker.borrow_mut() = Some(waker);
        let _timer = registry.register_timer(task, deadline, &state).unwrap();

        assert_eq!(registry.next_timer_deadline(), Some(deadline));
        registry.wake_due_timers(deadline - Duration::from_millis(1));
        assert!(!counter.0.load(Ordering::Acquire));
        registry.wake_due_timers(deadline);
        assert!(counter.0.load(Ordering::Acquire));
        assert!(state.ready.get());
        assert!(registry.timers.is_empty());
        assert!(registry.timer_deadlines.is_empty());
        assert!(registry.tasks[&task].timers.is_empty());
    }

    #[test]
    fn async_view_updates_are_deferred_typed_and_wake_the_task() {
        let window = WindowHandle::next();
        let registry = Rc::new(RefCell::new(ForegroundTaskRegistry::default()));
        let (task, _) = registry.borrow_mut().reserve(window).unwrap();
        let context = AsyncViewContext::<u32> {
            task,
            window,
            registry: Rc::downgrade(&registry),
            clock: ForegroundClock::system(),
            marker: PhantomData,
        };
        let mut update = Box::pin(context.update(|view, cx| {
            *view += 1;
            cx.invalidate();
            *view
        }));
        let (waker, counter) = test_waker();
        let mut task_context = Context::from_waker(&waker);

        assert!(update.as_mut().poll(&mut task_context).is_pending());
        let mut updates = registry.borrow_mut().take_updates(task);
        assert_eq!(updates.len(), 1);
        let mut view = 7_u32;
        let mut event_context = EventContext::default();
        updates.pop_front().unwrap()(&mut view, &mut event_context);
        assert_eq!(view, 8);
        assert!(event_context.invalidate);
        assert!(counter.0.load(Ordering::Acquire));
        assert_eq!(update.as_mut().poll(&mut task_context), Poll::Ready(Ok(8)));
    }

    #[test]
    fn cancelling_a_window_releases_updates_timers_and_counts() {
        let window = WindowHandle::next();
        let mut registry = ForegroundTaskRegistry::default();
        let (task, abort) = registry.reserve(window).unwrap();
        registry
            .enqueue_update(task, Box::new(|_, _| panic!("cancelled update ran")))
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(60);
        let timer_state = Rc::new(ForegroundTimerState {
            deadline,
            ready: Cell::new(false),
            waker: RefCell::new(None),
        });
        registry
            .register_timer(task, deadline, &timer_state)
            .unwrap();

        registry.cancel_window(window);

        assert!(abort.is_aborted());
        assert!(registry.tasks.is_empty());
        assert!(registry.tasks_per_window.is_empty());
        assert!(registry.timers.is_empty());
        assert!(registry.timer_deadlines.is_empty());
    }
}
