use std::{
    any::{Any, TypeId},
    cell::{Cell, RefCell},
    fmt,
    rc::{Rc, Weak},
    sync::atomic::{AtomicU64, Ordering},
};

use crate::EventContext;

static NEXT_ENTITY_ID: AtomicU64 = AtomicU64::new(1);

/// Maximum distinct entities one retained window may observe in one view declaration.
pub const MAX_OBSERVED_ENTITIES_PER_WINDOW: usize = 4_096;

/// Maximum distinct entity notifications retained from one event callback.
///
/// Crossing the bound falls back to one all-window invalidation instead of dropping a state
/// change or allocating without limit.
pub const MAX_ENTITY_NOTIFICATIONS_PER_EVENT: usize = 1_024;

/// Maximum typed entity subscriptions retained by one window declaration.
pub const MAX_ENTITY_SUBSCRIPTIONS_PER_WINDOW: usize = 4_096;

/// Maximum typed entity events one callback may enqueue.
pub const MAX_ENTITY_EVENTS_PER_CALLBACK: usize = 1_024;

/// Maximum typed events retained across one deferred application effect cycle.
pub const MAX_PENDING_ENTITY_EVENTS: usize = 4_096;

/// Maximum subscriber callbacks invoked before a recursive event cycle is rejected.
pub const MAX_ENTITY_EVENT_DELIVERIES_PER_TURN: usize = 65_536;

/// Stable identity for application state shared by one or more retained views.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct EntityId(u64);

impl EntityId {
    fn next() -> Self {
        Self(NEXT_ENTITY_ID.fetch_add(1, Ordering::Relaxed).max(1))
    }
}

/// Associates an entity state type with one typed event it is allowed to emit.
///
/// An entity may implement this trait for any number of event types.
pub trait EventEmitter<E: Any>: 'static {}

pub(crate) struct SubscriptionState {
    active: Cell<bool>,
}

impl SubscriptionState {
    fn new() -> Self {
        Self {
            active: Cell::new(true),
        }
    }

    pub(crate) fn is_active(&self) -> bool {
        self.active.get()
    }

    pub(crate) fn cancel(&self) {
        self.active.set(false);
    }
}

/// An RAII view subscription that cancels entity-event or global-change delivery when dropped.
///
/// Store this handle on the subscribing view for an explicit lifetime, or call [`Self::detach`] to
/// retain the callback until its component scope is removed (or the window closes for a root
/// subscription). The handle is deliberately
/// main-thread-only, matching [`Entity`].
#[must_use = "dropping a Subscription immediately cancels it; store it or call detach()"]
pub struct Subscription {
    state: Rc<SubscriptionState>,
    cancel_on_drop: bool,
}

impl Subscription {
    pub(crate) fn new() -> (Self, Rc<SubscriptionState>) {
        let state = Rc::new(SubscriptionState::new());
        (
            Self {
                state: state.clone(),
                cancel_on_drop: true,
            },
            state,
        )
    }

    /// Keep the subscription active until its owning component scope or window is destroyed.
    pub fn detach(mut self) {
        self.cancel_on_drop = false;
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        if self.cancel_on_drop {
            self.state.cancel();
        }
    }
}

impl fmt::Debug for Subscription {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Subscription")
            .field("active", &self.state.is_active())
            .field("detached", &!self.cancel_on_drop)
            .finish()
    }
}

pub(crate) struct EntityEvent {
    pub(crate) source: EntityId,
    pub(crate) event_type: TypeId,
    pub(crate) value: Box<dyn Any>,
    // Keep the source alive through deferred delivery. Subscriber callbacks capture only a weak
    // handle, so retaining one queued event cannot create a permanent ownership cycle.
    _source: Rc<dyn Any>,
}

impl EntityEvent {
    pub(crate) fn new<T, E>(entity: &Entity<T>, event: E) -> Self
    where
        T: EventEmitter<E>,
        E: Any,
    {
        let source: Rc<dyn Any> = entity.value.clone();
        Self {
            source: entity.id,
            event_type: TypeId::of::<E>(),
            value: Box::new(event),
            _source: source,
        }
    }
}

impl fmt::Debug for EntityEvent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EntityEvent")
            .field("source", &self.source)
            .field("event_type", &self.event_type)
            .finish_non_exhaustive()
    }
}

/// Main-thread application state that can be observed by independently retained windows.
///
/// `Entity` deliberately uses `Rc<RefCell<_>>`, not a cross-thread lock. QuickGUI invokes view
/// rendering and event callbacks serially on the application thread, so reads and updates stay
/// cheap. Background jobs should return `Send` data through [`crate::ViewContext::spawn`], then
/// update an entity from that job's UI-thread completion callback.
pub struct Entity<T> {
    id: EntityId,
    value: Rc<RefCell<T>>,
}

impl<T> Entity<T> {
    pub fn new(value: T) -> Self {
        Self {
            id: EntityId::next(),
            value: Rc::new(RefCell::new(value)),
        }
    }

    pub fn id(&self) -> EntityId {
        self.id
    }

    /// Read the current value without subscribing a window.
    ///
    /// Use [`crate::ViewContext::observe`] while rendering when later updates should invalidate
    /// that window automatically.
    pub fn read<R>(&self, read: impl FnOnce(&T) -> R) -> R {
        let value = self.value.try_borrow().unwrap_or_else(|_| {
            panic!(
                "entity {:?} cannot be read while it is mutably borrowed",
                self.id
            )
        });
        read(&value)
    }

    /// Mutate the value and notify every currently observing window exactly once.
    ///
    /// The callback runs synchronously on the application thread. Notifications are collected in
    /// the current event context and coalesced before any redraw is requested.
    pub fn update<R>(
        &self,
        cx: &mut EventContext,
        update: impl FnOnce(&mut T, &mut EventContext) -> R,
    ) -> R {
        let result = {
            let mut value = self.value.try_borrow_mut().unwrap_or_else(|_| {
                panic!(
                    "entity {:?} cannot be updated while it is already borrowed",
                    self.id
                )
            });
            update(&mut value, cx)
        };
        cx.notify(self);
        result
    }

    /// Create a non-owning reference suitable for callbacks and cyclic state graphs.
    pub fn downgrade(&self) -> WeakEntity<T> {
        WeakEntity {
            id: self.id,
            value: Rc::downgrade(&self.value),
        }
    }
}

impl<T: 'static> Entity<T> {
    /// Emit one typed event to every retained subscriber after the current callback releases its
    /// application-state borrow.
    ///
    /// Events preserve emission order and are not coalesced. `false` reports that the current
    /// callback reached its hard queue limit and the event was not retained.
    pub fn emit<E: Any>(&self, cx: &mut EventContext, event: E) -> bool
    where
        T: EventEmitter<E>,
    {
        cx.emit(self, event)
    }
}

impl<T> Clone for Entity<T> {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            value: self.value.clone(),
        }
    }
}

impl<T> fmt::Debug for Entity<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Entity")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

impl<T> PartialEq for Entity<T> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl<T> Eq for Entity<T> {}

/// A non-owning reference to an [`Entity`].
pub struct WeakEntity<T> {
    id: EntityId,
    value: Weak<RefCell<T>>,
}

impl<T> WeakEntity<T> {
    pub fn id(&self) -> EntityId {
        self.id
    }

    pub fn upgrade(&self) -> Option<Entity<T>> {
        Some(Entity {
            id: self.id,
            value: self.value.upgrade()?,
        })
    }

    pub fn is_alive(&self) -> bool {
        self.value.strong_count() != 0
    }
}

impl<T> Clone for WeakEntity<T> {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            value: self.value.clone(),
        }
    }
}

impl<T> fmt::Debug for WeakEntity<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WeakEntity")
            .field("id", &self.id)
            .field("alive", &self.is_alive())
            .finish()
    }
}

impl<T> PartialEq for WeakEntity<T> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl<T> Eq for WeakEntity<T> {}

#[cfg(test)]
mod tests {
    use super::*;

    struct Publisher;

    #[derive(Debug, Eq, PartialEq)]
    struct Published(u16);

    impl EventEmitter<Published> for Publisher {}

    #[test]
    fn clones_share_state_and_updates_notify_once() {
        let entity = Entity::new(3_u32);
        let clone = entity.clone();
        let mut cx = EventContext::default();

        entity.update(&mut cx, |value, cx| {
            *value += 4;
            cx.notify(&clone);
        });

        assert_eq!(clone.read(|value| *value), 7);
        assert_eq!(cx.entity_notifications, [entity.id()]);
        assert!(!cx.notify_all_entities);
    }

    #[test]
    fn weak_entities_do_not_extend_lifetime() {
        let entity = Entity::new(String::from("shared"));
        let weak = entity.downgrade();
        assert_eq!(weak.upgrade().unwrap().read(Clone::clone), "shared");
        drop(entity);
        assert!(!weak.is_alive());
        assert!(weak.upgrade().is_none());
    }

    #[test]
    fn notification_overflow_falls_back_without_growing_the_queue() {
        let mut cx = EventContext::default();
        for _ in 0..=MAX_ENTITY_NOTIFICATIONS_PER_EVENT {
            cx.notify(&Entity::new(()));
        }

        assert!(cx.notify_all_entities);
        assert!(cx.entity_notifications.is_empty());
    }

    #[test]
    fn typed_events_preserve_order_and_keep_the_source_alive_through_delivery() {
        let entity = Entity::new(Publisher);
        let weak = entity.downgrade();
        let mut cx = EventContext::default();

        assert!(entity.emit(&mut cx, Published(7)));
        assert!(cx.emit(&entity, Published(11)));
        drop(entity);

        assert!(weak.is_alive());
        assert_eq!(cx.entity_events.len(), 2);
        assert_eq!(cx.entity_events[0].event_type, TypeId::of::<Published>());
        assert_eq!(
            cx.entity_events[0].value.downcast_ref::<Published>(),
            Some(&Published(7))
        );
        assert_eq!(
            cx.entity_events[1].value.downcast_ref::<Published>(),
            Some(&Published(11))
        );

        drop(cx);
        assert!(!weak.is_alive());
    }

    #[test]
    fn typed_event_count_is_hard_bounded_per_callback() {
        let entity = Entity::new(Publisher);
        let mut cx = EventContext::default();

        for value in 0..MAX_ENTITY_EVENTS_PER_CALLBACK {
            assert!(entity.emit(&mut cx, Published(value as u16)));
        }
        assert!(!entity.emit(&mut cx, Published(u16::MAX)));
        assert_eq!(cx.entity_events.len(), MAX_ENTITY_EVENTS_PER_CALLBACK);
    }

    #[test]
    fn subscription_drop_cancels_while_detach_preserves_delivery() {
        let (subscription, state) = Subscription::new();
        assert!(state.is_active());
        drop(subscription);
        assert!(!state.is_active());

        let (subscription, detached_state) = Subscription::new();
        subscription.detach();
        assert!(detached_state.is_active());
    }
}
