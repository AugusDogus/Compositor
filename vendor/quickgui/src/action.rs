use std::{
    any::{Any, TypeId, type_name},
    fmt,
    marker::PhantomData,
    rc::Rc,
};

use crate::{DispatchPhase, ElementId};

/// Maximum typed action listeners attached to one retained element.
pub const MAX_ACTION_LISTENERS_PER_ELEMENT: usize = 32;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ActionListenerKey(pub(crate) u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ActionListenerBinding {
    pub(crate) key: ActionListenerKey,
    pub(crate) action_type: TypeId,
    pub(crate) phase: DispatchPhase,
}

/// A typed command that can be bound independently from the element that handles it.
///
/// Actions stay on the UI thread and only need cheap cloning plus equality. Any
/// `Clone + PartialEq + 'static` value is an action, so payload-bearing commands are as ergonomic
/// as unit structs while keymaps and menus can still distinguish their payloads.
pub trait Action: Clone + PartialEq + 'static {}

impl<T> Action for T where T: Clone + PartialEq + 'static {}

/// A cheaply cloned, type-erased action used by keymaps and deferred dispatch.
#[derive(Clone)]
pub struct AnyAction {
    value: Rc<dyn Any>,
    type_id: TypeId,
    name: &'static str,
    partial_eq: fn(&dyn Any, &dyn Any) -> bool,
}

impl AnyAction {
    pub fn new<A: Action>(action: A) -> Self {
        Self {
            value: Rc::new(action),
            type_id: TypeId::of::<A>(),
            name: type_name::<A>(),
            partial_eq: |left, right| {
                left.downcast_ref::<A>()
                    .zip(right.downcast_ref::<A>())
                    .is_some_and(|(left, right)| left == right)
            },
        }
    }

    pub fn type_id(&self) -> TypeId {
        self.type_id
    }

    pub fn name(&self) -> &'static str {
        self.name
    }

    pub fn downcast_ref<A: Action>(&self) -> Option<&A> {
        self.value.downcast_ref()
    }

    pub fn partial_eq(&self, other: &Self) -> bool {
        self.type_id == other.type_id && (self.partial_eq)(self.as_any(), other.as_any())
    }

    pub(crate) fn as_any(&self) -> &dyn Any {
        self.value.as_ref()
    }
}

impl fmt::Debug for AnyAction {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AnyAction")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

impl PartialEq for AnyAction {
    fn eq(&self, other: &Self) -> bool {
        self.partial_eq(other)
    }
}

/// An opaque typed binding returned by [`crate::ViewContext::action_listener`].
///
/// Attach it with [`crate::Element::on_action`] for focus-to-root bubble dispatch or
/// [`crate::Element::capture_action`] for root-to-focus capture dispatch.
pub struct ActionListener<V, A> {
    pub(crate) id: ElementId,
    pub(crate) key: ActionListenerKey,
    pub(crate) action_type: TypeId,
    pub(crate) marker: PhantomData<fn(&mut V, &A)>,
}

impl<V, A> ActionListener<V, A> {
    pub(crate) fn id(&self) -> ElementId {
        self.id
    }

    pub(crate) fn key(&self) -> ActionListenerKey {
        self.key
    }

    pub(crate) fn action_type(&self) -> TypeId {
        self.action_type
    }
}

impl<V, A> Copy for ActionListener<V, A> {}

impl<V, A> Clone for ActionListener<V, A> {
    fn clone(&self) -> Self {
        *self
    }
}

/// Declare payload-free action types with GPUI-compatible call-site ergonomics.
///
/// ```
/// quickgui::actions!(editor, [Undo, Redo]);
/// let _ = Undo;
/// ```
#[macro_export]
macro_rules! actions {
    ($namespace:ident, [$($name:ident),* $(,)?]) => {
        $(
            #[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
            pub struct $name;
        )*
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug, Eq, PartialEq)]
    struct OpenLine {
        line: usize,
    }

    #[test]
    fn actions_keep_their_concrete_payload_after_erasure() {
        let action = AnyAction::new(OpenLine { line: 42 });
        assert_eq!(action.type_id(), TypeId::of::<OpenLine>());
        assert_eq!(action.downcast_ref::<OpenLine>().unwrap().line, 42);
        assert!(action.downcast_ref::<String>().is_none());
        assert!(action.partial_eq(&AnyAction::new(OpenLine { line: 42 })));
        assert!(!action.partial_eq(&AnyAction::new(OpenLine { line: 7 })));
    }
}
