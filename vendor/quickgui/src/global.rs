use std::{
    any::{Any, TypeId, type_name},
    cell::{Ref, RefCell, RefMut},
    collections::HashMap,
    fmt,
    rc::Rc,
};

/// Maximum distinct typed values retained by one QuickGUI application.
///
/// This is a count bound around application-owned values. Individual value sizes remain under the
/// application's control.
pub const MAX_APPLICATION_GLOBALS: usize = 1_024;

/// Maximum distinct global types one retained window may observe during a view declaration.
pub const MAX_OBSERVED_GLOBALS_PER_WINDOW: usize = 1_024;

/// Maximum RAII global-change subscriptions retained by one window.
pub const MAX_GLOBAL_SUBSCRIPTIONS_PER_WINDOW: usize = 1_024;

/// Maximum distinct global changes retained from one event callback.
///
/// Crossing this bound conservatively invalidates and notifies all global observers instead of
/// dropping a change or allowing an unbounded command buffer.
pub const MAX_GLOBAL_NOTIFICATIONS_PER_EVENT: usize = 256;

/// Maximum distinct global types queued across one deferred application effect cycle.
pub const MAX_PENDING_GLOBAL_NOTIFICATIONS: usize = MAX_APPLICATION_GLOBALS;

/// Maximum global observer callbacks invoked during one deferred effect turn.
pub const MAX_GLOBAL_OBSERVER_DELIVERIES_PER_TURN: usize = 65_536;

/// Marker trait for a type stored once in application-global state.
///
/// Globals are main-thread-owned and need not implement `Send` or `Sync`. Use a private wrapper type
/// when an application wants to restrict which modules can access a value.
pub trait Global: 'static {}

#[derive(Clone, Default)]
pub(crate) struct GlobalStore {
    values: Rc<RefCell<HashMap<TypeId, Box<dyn Any>>>>,
}

impl GlobalStore {
    pub(crate) fn has<G: Global>(&self) -> bool {
        self.values
            .try_borrow()
            .unwrap_or_else(|_| {
                panic!(
                    "application globals cannot be inspected while another global is mutably borrowed"
                )
            })
            .contains_key(&TypeId::of::<G>())
    }

    #[track_caller]
    pub(crate) fn get<G: Global>(&self) -> Ref<'_, G> {
        Ref::map(
            self.values.try_borrow().unwrap_or_else(|_| {
                panic!(
                    "global {} cannot be read while application globals are mutably borrowed",
                    type_name::<G>()
                )
            }),
            |values| {
                values
                    .get(&TypeId::of::<G>())
                    .and_then(|value| value.downcast_ref::<G>())
                    .unwrap_or_else(|| panic!("no global of type {} exists", type_name::<G>()))
            },
        )
    }

    pub(crate) fn try_get<G: Global>(&self) -> Option<Ref<'_, G>> {
        Ref::filter_map(
            self.values.try_borrow().unwrap_or_else(|_| {
                panic!(
                    "global {} cannot be read while application globals are mutably borrowed",
                    type_name::<G>()
                )
            }),
            |values| {
                values
                    .get(&TypeId::of::<G>())
                    .and_then(|value| value.downcast_ref::<G>())
            },
        )
        .ok()
    }

    #[track_caller]
    pub(crate) fn get_mut<G: Global>(&self) -> RefMut<'_, G> {
        RefMut::map(
            self.values.try_borrow_mut().unwrap_or_else(|_| {
                panic!(
                    "global {} cannot be mutated while application globals are borrowed",
                    type_name::<G>()
                )
            }),
            |values| {
                values
                    .get_mut(&TypeId::of::<G>())
                    .and_then(|value| value.downcast_mut::<G>())
                    .unwrap_or_else(|| panic!("no global of type {} exists", type_name::<G>()))
            },
        )
    }

    pub(crate) fn default_mut<G: Global + Default>(&self) -> RefMut<'_, G> {
        let values = self.values.try_borrow_mut().unwrap_or_else(|_| {
            panic!(
                "global {} cannot be initialized while application globals are borrowed",
                type_name::<G>()
            )
        });
        RefMut::map(values, |values| {
            if !values.contains_key(&TypeId::of::<G>()) {
                assert!(
                    values.len() < MAX_APPLICATION_GLOBALS,
                    "an application cannot retain more than {MAX_APPLICATION_GLOBALS} global types"
                );
                values.insert(TypeId::of::<G>(), Box::<G>::default());
            }
            values
                .get_mut(&TypeId::of::<G>())
                .and_then(|value| value.downcast_mut::<G>())
                .expect("a global is always stored under its concrete TypeId")
        })
    }

    pub(crate) fn set<G: Global>(&self, global: G) {
        let mut values = self.values.try_borrow_mut().unwrap_or_else(|_| {
            panic!(
                "global {} cannot be assigned while application globals are borrowed",
                type_name::<G>()
            )
        });
        assert!(
            values.contains_key(&TypeId::of::<G>()) || values.len() < MAX_APPLICATION_GLOBALS,
            "an application cannot retain more than {MAX_APPLICATION_GLOBALS} global types"
        );
        values.insert(TypeId::of::<G>(), Box::new(global));
    }

    #[track_caller]
    pub(crate) fn remove<G: Global>(&self) -> G {
        let value = self
            .values
            .try_borrow_mut()
            .unwrap_or_else(|_| {
                panic!(
                    "global {} cannot be removed while application globals are borrowed",
                    type_name::<G>()
                )
            })
            .remove(&TypeId::of::<G>())
            .unwrap_or_else(|| panic!("no global of type {} exists", type_name::<G>()));
        *value
            .downcast::<G>()
            .expect("a global is always stored under its concrete TypeId")
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.values.borrow().len()
    }
}

impl fmt::Debug for GlobalStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GlobalStore")
            .field(
                "len",
                &self.values.try_borrow().map_or(0, |values| values.len()),
            )
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Default, Eq, PartialEq)]
    struct Theme {
        dark: bool,
    }

    impl Global for Theme {}

    #[derive(Debug, Eq, PartialEq)]
    struct Locale(&'static str);

    impl Global for Locale {}

    #[test]
    fn globals_are_exactly_typed_replaceable_and_removable() {
        let globals = GlobalStore::default();
        assert!(!globals.has::<Theme>());
        assert!(globals.try_get::<Theme>().is_none());

        globals.set(Theme { dark: true });
        globals.set(Locale("en-SG"));
        assert_eq!(globals.len(), 2);
        assert!(globals.get::<Theme>().dark);
        assert_eq!(globals.get::<Locale>().0, "en-SG");

        globals.set(Theme { dark: false });
        assert_eq!(globals.len(), 2);
        assert!(!globals.get::<Theme>().dark);
        assert_eq!(globals.remove::<Locale>(), Locale("en-SG"));
        assert!(!globals.has::<Locale>());
    }

    #[test]
    fn default_global_is_inserted_once_and_mutated_in_place() {
        let globals = GlobalStore::default();
        globals.default_mut::<Theme>().dark = true;
        assert!(globals.default_mut::<Theme>().dark);
        assert_eq!(globals.len(), 1);
    }

    #[test]
    #[should_panic(expected = "cannot be read while application globals are mutably borrowed")]
    fn overlapping_mutable_and_shared_global_borrows_fail_loudly() {
        let globals = GlobalStore::default();
        globals.set(Theme::default());
        let _mutable = globals.get_mut::<Theme>();
        let _shared = globals.get::<Theme>();
    }
}
