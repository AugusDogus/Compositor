use std::{
    collections::{HashMap, VecDeque},
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{Arc, Mutex, mpsc},
    thread,
};
use web_time::{Duration, Instant};

use winit::event_loop::EventLoopProxy;

use crate::{
    Element, ImageResource, WindowHandle,
    animated_image::ImageAsset,
    element::{ElementKind, ImageResolution},
    image::ImageResourceKey,
    runtime::RuntimeEvent,
};

/// Delay before a loading replacement is shown, avoiding flashes for fast local resources.
pub const IMAGE_LOADING_DELAY: Duration = Duration::from_millis(200);
/// Maximum decoded image bytes retained by the asynchronous CPU resource cache.
pub const MAX_CPU_IMAGE_CACHE_BYTES: u64 = 128 * 1024 * 1024;
/// Maximum ready, loading, deferred, and failed image resource entries retained by one window.
pub const MAX_IMAGE_RESOURCE_CACHE_ENTRIES: usize = 256;
/// Maximum image decode jobs waiting behind the fixed worker set.
pub const MAX_PENDING_IMAGE_LOADS: usize = 64;

const IMAGE_WORKER_THREADS: usize = 2;
const MAX_REPLACEMENT_DEPTH: usize = 16;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ImageResourceStats {
    pub entries: usize,
    pub ready: usize,
    pub loading: usize,
    pub failed: usize,
    pub decoded_bytes: u64,
}

pub(crate) struct ImageLoadCompletion {
    key: ImageResourceKey,
    request_id: u64,
    result: Result<ImageAsset, Arc<str>>,
}

struct ImageLoadJob {
    window: WindowHandle,
    resource: ImageResource,
    request_id: u64,
}

struct ImageWorkerPool {
    sender: mpsc::SyncSender<ImageLoadJob>,
    // Dropping a JoinHandle detaches it. Shutdown must never wait on an application-provided
    // loader that may be blocked in file or network code.
    _workers: Vec<thread::JoinHandle<()>>,
}

enum SharedWorkerState {
    Dormant,
    Ready(ImageWorkerPool),
    Failed,
}

/// One lazily-created, bounded decode pool shared by every window in an application runtime.
#[derive(Clone)]
pub(crate) struct ImageWorkerPoolHandle {
    proxy: EventLoopProxy<RuntimeEvent>,
    state: Arc<Mutex<SharedWorkerState>>,
}

impl ImageWorkerPoolHandle {
    pub(crate) fn new(proxy: EventLoopProxy<RuntimeEvent>) -> Self {
        Self {
            proxy,
            state: Arc::new(Mutex::new(SharedWorkerState::Dormant)),
        }
    }

    fn try_load(&self, job: ImageLoadJob) -> Result<(), WorkerQueueError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if matches!(*state, SharedWorkerState::Dormant) {
            *state = match ImageWorkerPool::new(self.proxy.clone()) {
                Ok(workers) => SharedWorkerState::Ready(workers),
                Err(error) => {
                    tracing::warn!(%error, "image decode workers could not be started");
                    SharedWorkerState::Failed
                }
            };
        }
        match &*state {
            SharedWorkerState::Ready(workers) => workers.try_load(job),
            SharedWorkerState::Dormant => unreachable!("the worker pool was initialized above"),
            SharedWorkerState::Failed => Err(WorkerQueueError::Disconnected),
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

impl ImageWorkerPool {
    fn new(proxy: EventLoopProxy<RuntimeEvent>) -> std::io::Result<Self> {
        let (sender, receiver) = mpsc::sync_channel(MAX_PENDING_IMAGE_LOADS);
        let receiver = Arc::new(Mutex::new(receiver));
        let workers = (0..IMAGE_WORKER_THREADS)
            .map(|index| {
                let receiver = receiver.clone();
                let proxy = proxy.clone();
                thread::Builder::new()
                    .name(format!("quickgui-image-{index}"))
                    .spawn(move || image_worker(receiver, proxy))
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            sender,
            _workers: workers,
        })
    }

    fn try_load(&self, job: ImageLoadJob) -> Result<(), WorkerQueueError> {
        self.sender.try_send(job).map_err(|error| match error {
            mpsc::TrySendError::Full(_) => WorkerQueueError::Full,
            mpsc::TrySendError::Disconnected(_) => WorkerQueueError::Disconnected,
        })
    }
}

fn image_worker(
    receiver: Arc<Mutex<mpsc::Receiver<ImageLoadJob>>>,
    proxy: EventLoopProxy<RuntimeEvent>,
) {
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
        let key = job.resource.key().clone();
        let result = catch_unwind(AssertUnwindSafe(|| job.resource.load()))
            .unwrap_or_else(|_| Err(Arc::from("the application image loader panicked")));
        let _ = proxy.send_event(RuntimeEvent::ImageLoaded(
            job.window,
            ImageLoadCompletion {
                key,
                request_id: job.request_id,
                result,
            },
        ));
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WorkerQueueError {
    Full,
    Disconnected,
}

enum ResourceState {
    Deferred {
        started: Instant,
        loading_announced: bool,
    },
    Loading {
        request_id: u64,
        started: Instant,
        loading_announced: bool,
    },
    Ready(ImageAsset),
    Failed,
}

struct ResourceEntry {
    resource: ImageResource,
    state: ResourceState,
    last_used_frame: u64,
    loading_ui_frame: u64,
}

#[derive(Clone)]
enum ResourceStatus {
    Loading { show_replacement: bool },
    Ready(ImageAsset),
    Failed,
}

pub(crate) struct ImageAssetCache {
    window: WindowHandle,
    workers: ImageWorkerPoolHandle,
    entries: HashMap<ImageResourceKey, ResourceEntry>,
    deferred: VecDeque<ImageResourceKey>,
    frame: u64,
    next_request_id: u64,
    resident_bytes: u64,
}

impl ImageAssetCache {
    pub(crate) fn new(window: WindowHandle, workers: ImageWorkerPoolHandle) -> Self {
        Self {
            window,
            workers,
            entries: HashMap::with_capacity(32),
            deferred: VecDeque::with_capacity(16),
            frame: 0,
            next_request_id: 1,
            resident_bytes: 0,
        }
    }

    pub(crate) fn resolve_tree(&mut self, root: &mut Element) {
        self.begin_resolve_tree(root);
        for _ in 0..MAX_REPLACEMENT_DEPTH {
            if !self.finish_resolve_frame() {
                break;
            }
            self.resolve_element(root, 0);
        }
    }

    /// Begin one declaration-wide cache frame without pruning resources that may be produced by
    /// size-dependent callbacks later in the same declaration.
    pub(crate) fn begin_resolve_tree(&mut self, root: &mut Element) {
        self.frame = self.frame.wrapping_add(1).max(1);
        self.resolve_element(root, 0);
    }

    /// Finish the current declaration after every container-query callback has visited its image
    /// resources. Returns whether an active worker failure changed a resource to its fallback
    /// state and therefore needs one correcting declaration.
    pub(crate) fn finish_resolve_frame(&mut self) -> bool {
        self.remove_inactive_deferred();
        self.pump_deferred()
    }

    /// Resolve a subtree materialized after the surrounding cache frame has already begun.
    ///
    /// Container-query callbacks run only after their box has a layout size. Reusing the current
    /// frame here keeps their resources active without advancing LRU time once per nested query.
    /// Queue pumping is deliberately deferred until every sibling callback has run; otherwise an
    /// older deferred sibling could be skipped before it is marked active in the new frame.
    pub(crate) fn resolve_subtree(&mut self, root: &mut Element) {
        self.resolve_element(root, 0);
    }

    pub(crate) fn complete(&mut self, completion: ImageLoadCompletion) -> bool {
        let Some(entry) = self.entries.get(&completion.key) else {
            return self.pump_deferred();
        };
        let ResourceState::Loading { request_id, .. } = &entry.state else {
            return self.pump_deferred();
        };
        if *request_id != completion.request_id {
            return self.pump_deferred();
        }
        let active = entry.last_used_frame == self.frame;
        match completion.result {
            Ok(image) => {
                let bytes = image.byte_len();
                if self.make_room_for_bytes(bytes, &completion.key) {
                    self.resident_bytes += bytes;
                    self.entries
                        .get_mut(&completion.key)
                        .expect("completion entry remains resident")
                        .state = ResourceState::Ready(image);
                } else {
                    tracing::warn!(
                        resource = ?completion.key,
                        bytes,
                        "image resource exceeds the active CPU cache budget"
                    );
                    self.entries
                        .get_mut(&completion.key)
                        .expect("completion entry remains resident")
                        .state = ResourceState::Failed;
                }
            }
            Err(error) => {
                tracing::warn!(resource = ?completion.key, %error, "image resource failed to load");
                self.entries
                    .get_mut(&completion.key)
                    .expect("completion entry remains resident")
                    .state = ResourceState::Failed;
            }
        }
        let deferred_changed = self.pump_deferred();
        active || deferred_changed
    }

    /// Retry this window's deferred jobs after the application-wide worker queue releases a slot.
    pub(crate) fn resume_deferred(&mut self) -> bool {
        self.pump_deferred()
    }

    pub(crate) fn announce_due_loading(&mut self, now: Instant) -> bool {
        let mut changed = false;
        for entry in self.entries.values_mut() {
            if entry.last_used_frame != self.frame || entry.loading_ui_frame != self.frame {
                continue;
            }
            match &mut entry.state {
                ResourceState::Deferred {
                    started,
                    loading_announced,
                }
                | ResourceState::Loading {
                    started,
                    loading_announced,
                    ..
                } if !*loading_announced && now >= *started + IMAGE_LOADING_DELAY => {
                    *loading_announced = true;
                    changed = true;
                }
                _ => {}
            }
        }
        changed
    }

    pub(crate) fn next_loading_deadline(&self) -> Option<Instant> {
        self.entries
            .values()
            .filter(|entry| {
                entry.last_used_frame == self.frame && entry.loading_ui_frame == self.frame
            })
            .filter_map(|entry| match &entry.state {
                ResourceState::Deferred {
                    started,
                    loading_announced: false,
                }
                | ResourceState::Loading {
                    started,
                    loading_announced: false,
                    ..
                } => Some(*started + IMAGE_LOADING_DELAY),
                _ => None,
            })
            .min()
    }

    pub(crate) fn stats(&self) -> ImageResourceStats {
        let mut stats = ImageResourceStats {
            entries: self.entries.len(),
            decoded_bytes: self.resident_bytes,
            ..Default::default()
        };
        for entry in self.entries.values() {
            match &entry.state {
                ResourceState::Deferred { .. } | ResourceState::Loading { .. } => {
                    stats.loading += 1;
                }
                ResourceState::Ready(_) => stats.ready += 1,
                ResourceState::Failed => stats.failed += 1,
            }
        }
        stats
    }

    fn resolve_element(&mut self, element: &mut Element, replacement_depth: usize) {
        // `display: none` is a semantic subtree boundary, not only a paint optimization. Hidden
        // resources must not start workers, consume cache residency, or keep loading deadlines
        // alive until the application declares the subtree visible again.
        if element.is_display_none() {
            return;
        }
        let mut replacement = None;
        let mut is_image = false;
        if let ElementKind::Image(image) = &mut element.kind {
            is_image = true;
            if let Some(ready) = image.source.image() {
                image.resolved = ImageResolution::Ready(ready.clone());
            } else if let Some(resource) = image.source.resource() {
                let status = self.resolve_resource(resource, image.loading.is_some());
                match status {
                    ResourceStatus::Ready(ready) => {
                        image.resolved = match ready {
                            ImageAsset::Static(image) => ImageResolution::Ready(image),
                            ImageAsset::Animated(animation) => ImageResolution::Animated(animation),
                        };
                    }
                    ResourceStatus::Loading { show_replacement } => {
                        image.resolved = ImageResolution::Loading;
                        if show_replacement && replacement_depth < MAX_REPLACEMENT_DEPTH {
                            replacement = image.loading.as_ref().map(|loading| loading.render());
                        }
                    }
                    ResourceStatus::Failed => {
                        image.resolved = ImageResolution::Failed;
                        if replacement_depth < MAX_REPLACEMENT_DEPTH {
                            replacement = image.fallback.as_ref().map(|fallback| fallback.render());
                        }
                    }
                }
            }
        }

        if is_image {
            element.children.clear();
            if let Some(replacement) = replacement {
                element.children.push(replacement);
            }
        }
        if let Some(tooltip) = &mut element.tooltip {
            self.resolve_element(tooltip.content_mut(), replacement_depth);
        }
        let child_depth = replacement_depth + usize::from(is_image && !element.children.is_empty());
        for child in &mut element.children {
            self.resolve_element(child, child_depth);
        }
    }

    fn resolve_resource(
        &mut self,
        resource: &ImageResource,
        wants_loading_ui: bool,
    ) -> ResourceStatus {
        let key = resource.key().clone();
        if let Some(entry) = self.entries.get_mut(&key) {
            entry.last_used_frame = self.frame;
            if wants_loading_ui {
                entry.loading_ui_frame = self.frame;
            }
            return status_for(entry);
        }

        if !self.make_room_for_entry() {
            return ResourceStatus::Failed;
        }
        let now = Instant::now();
        self.entries.insert(
            key.clone(),
            ResourceEntry {
                resource: resource.clone(),
                state: ResourceState::Deferred {
                    started: now,
                    loading_announced: false,
                },
                last_used_frame: self.frame,
                loading_ui_frame: if wants_loading_ui { self.frame } else { 0 },
            },
        );
        self.deferred.push_back(key);
        ResourceStatus::Loading {
            show_replacement: false,
        }
    }

    fn make_room_for_entry(&mut self) -> bool {
        while self.entries.len() >= MAX_IMAGE_RESOURCE_CACHE_ENTRIES {
            let candidate = self
                .entries
                .iter()
                .filter(|(_, entry)| entry.last_used_frame != self.frame)
                .min_by_key(|(_, entry)| entry.last_used_frame)
                .map(|(key, _)| key.clone());
            let Some(candidate) = candidate else {
                return false;
            };
            self.remove_entry(&candidate);
        }
        true
    }

    fn make_room_for_bytes(&mut self, incoming: u64, protected: &ImageResourceKey) -> bool {
        while self.resident_bytes.saturating_add(incoming) > MAX_CPU_IMAGE_CACHE_BYTES {
            let candidate = self
                .entries
                .iter()
                .filter(|(key, entry)| {
                    *key != protected
                        && entry.last_used_frame != self.frame
                        && matches!(entry.state, ResourceState::Ready(_))
                })
                .min_by_key(|(_, entry)| entry.last_used_frame)
                .map(|(key, _)| key.clone());
            let Some(candidate) = candidate else {
                return false;
            };
            self.remove_entry(&candidate);
        }
        true
    }

    fn remove_entry(&mut self, key: &ImageResourceKey) {
        if let Some(entry) = self.entries.remove(key)
            && let ResourceState::Ready(image) = entry.state
        {
            self.resident_bytes = self.resident_bytes.saturating_sub(image.byte_len());
        }
    }

    fn remove_inactive_deferred(&mut self) {
        let frame = self.frame;
        let keys: Vec<_> = self
            .entries
            .iter()
            .filter(|(_, entry)| {
                entry.last_used_frame != frame
                    && matches!(entry.state, ResourceState::Deferred { .. })
            })
            .map(|(key, _)| key.clone())
            .collect();
        for key in keys {
            self.remove_entry(&key);
        }
    }

    fn pump_deferred(&mut self) -> bool {
        let mut active_failure = false;
        while let Some(key) = self.deferred.pop_front() {
            let Some(entry) = self.entries.get(&key) else {
                continue;
            };
            if entry.last_used_frame != self.frame
                || !matches!(entry.state, ResourceState::Deferred { .. })
            {
                continue;
            }
            let request_id = self.next_request_id;
            self.next_request_id = self.next_request_id.wrapping_add(1).max(1);
            let job = ImageLoadJob {
                window: self.window,
                resource: entry.resource.clone(),
                request_id,
            };
            match self.workers.try_load(job) {
                Ok(()) => {
                    let entry = self
                        .entries
                        .get_mut(&key)
                        .expect("queued image resource remains resident");
                    let (started, loading_announced) = match &entry.state {
                        ResourceState::Deferred {
                            started,
                            loading_announced,
                        } => (*started, *loading_announced),
                        _ => unreachable!("only deferred entries are queued"),
                    };
                    entry.state = ResourceState::Loading {
                        request_id,
                        started,
                        loading_announced,
                    };
                }
                Err(WorkerQueueError::Full) => {
                    self.deferred.push_front(key);
                    return active_failure;
                }
                Err(WorkerQueueError::Disconnected) => {
                    self.entries
                        .get_mut(&key)
                        .expect("disconnected resource remains resident")
                        .state = ResourceState::Failed;
                    active_failure = true;
                }
            }
        }
        active_failure
    }
}

fn status_for(entry: &ResourceEntry) -> ResourceStatus {
    match &entry.state {
        ResourceState::Deferred {
            loading_announced, ..
        }
        | ResourceState::Loading {
            loading_announced, ..
        } => ResourceStatus::Loading {
            show_replacement: *loading_announced,
        },
        ResourceState::Ready(image) => ResourceStatus::Ready(image.clone()),
        ResourceState::Failed => ResourceStatus::Failed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Image;

    #[test]
    fn loading_deadline_is_exactly_delayed() {
        let started = Instant::now();
        let mut entry = ResourceEntry {
            resource: ImageResource::custom(|| {
                Ok::<_, &str>(Image::from_rgba(1, 1, vec![0; 4]).unwrap())
            }),
            state: ResourceState::Deferred {
                started,
                loading_announced: false,
            },
            last_used_frame: 1,
            loading_ui_frame: 1,
        };
        assert!(matches!(
            status_for(&entry),
            ResourceStatus::Loading {
                show_replacement: false
            }
        ));
        assert_eq!(
            started + IMAGE_LOADING_DELAY,
            started + Duration::from_millis(200)
        );
        let ResourceState::Deferred {
            loading_announced, ..
        } = &mut entry.state
        else {
            unreachable!()
        };
        *loading_announced = true;
        assert!(matches!(
            status_for(&entry),
            ResourceStatus::Loading {
                show_replacement: true
            }
        ));
    }

    #[test]
    fn path_resources_deduplicate_but_custom_resources_do_not() {
        let first = ImageResource::from_path("same.png");
        let second = ImageResource::from_path("same.png");
        assert_eq!(first, second);

        let first = ImageResource::custom(|| Err::<Image, _>("no image"));
        assert_eq!(first, first.clone());
        let second = ImageResource::custom(|| Err::<Image, _>("no image"));
        assert_ne!(first, second);
    }
}
