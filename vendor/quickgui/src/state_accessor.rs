//! Per-instance retained-state accessors for the unstyled component layer.
//!
//! Every unstyled component that retains interaction state reaches it through an accessor from
//! the owning view. The original entry points took a non-capturing `fn(&mut V) -> &mut State`
//! pointer, which is enough for an application that declares each control as a distinct field but
//! not for a host that renders many declared controls through one view — a hosted JavaScript tree,
//! a repeated row, or a collection built from data all need the accessor to carry which instance
//! it addresses.
//!
//! [`StateAccessor`] is that accessor: one reference-counted closure, cloned into the component's
//! listeners exactly like the `fn` pointer was. Because it is a single erased type rather than a
//! generic parameter, adding it does not multiply the component code by the number of accessor
//! shapes, and the number of listeners each component registers is unchanged, so a view that
//! declares many instances still registers a bounded, per-instance-constant listener set.
//!
//! The `fn` pointer entry points remain: they are thin wrappers that build a `StateAccessor` from
//! the pointer, so existing applications, examples, and tests keep compiling unchanged.

use std::fmt;
use std::rc::Rc;

/// A cloneable accessor from a view to one component instance's retained state.
///
/// Construct it with [`StateAccessor::new`] for a per-instance closure, or with `From` for a plain
/// `fn` pointer.
///
/// ```
/// use quickgui::StateAccessor;
///
/// struct Row {
///     value: u32,
/// }
/// struct Demo {
///     rows: Vec<Row>,
/// }
///
/// let first = StateAccessor::new(|view: &mut Demo| &mut view.rows[0]);
/// let second = StateAccessor::new(|view: &mut Demo| &mut view.rows[1]);
/// let mut demo = Demo {
///     rows: vec![Row { value: 0 }, Row { value: 0 }],
/// };
/// first.get(&mut demo).value = 7;
/// second.get(&mut demo).value = 9;
/// assert_eq!(demo.rows[0].value, 7);
/// assert_eq!(demo.rows[1].value, 9);
/// ```
pub struct StateAccessor<V: 'static, S: 'static> {
    access: Rc<dyn Fn(&mut V) -> &mut S>,
}

impl<V: 'static, S: 'static> StateAccessor<V, S> {
    /// Build an accessor from a closure that may capture which instance it addresses.
    ///
    /// The closure must resolve to state owned by the view; it is called while the component is
    /// handling an event, never on an idle frame.
    pub fn new(access: impl Fn(&mut V) -> &mut S + 'static) -> Self {
        Self {
            access: Rc::new(access),
        }
    }

    /// Resolve this instance's retained state inside the owning view.
    pub fn get<'a>(&self, view: &'a mut V) -> &'a mut S {
        (self.access)(view)
    }
}

impl<V: 'static, S: 'static> Clone for StateAccessor<V, S> {
    fn clone(&self) -> Self {
        Self {
            access: Rc::clone(&self.access),
        }
    }
}

impl<V: 'static, S: 'static> fmt::Debug for StateAccessor<V, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StateAccessor").finish_non_exhaustive()
    }
}

impl<V: 'static, S: 'static> From<fn(&mut V) -> &mut S> for StateAccessor<V, S> {
    fn from(access: fn(&mut V) -> &mut S) -> Self {
        Self::new(access)
    }
}

#[cfg(test)]
mod tests {
    use super::StateAccessor;

    struct Demo {
        left: u32,
        right: u32,
    }

    #[test]
    fn fn_pointer_accessors_convert() {
        let accessor: StateAccessor<Demo, u32> =
            StateAccessor::from((|view: &mut Demo| &mut view.left) as fn(&mut Demo) -> &mut u32);
        let mut demo = Demo { left: 1, right: 2 };
        *accessor.get(&mut demo) = 5;
        assert_eq!(demo.left, 5);
        assert_eq!(demo.right, 2);
    }

    #[test]
    fn cloned_accessors_address_the_same_instance() {
        let accessor = StateAccessor::new(|view: &mut Demo| &mut view.right);
        let clone = accessor.clone();
        let mut demo = Demo { left: 1, right: 2 };
        *accessor.get(&mut demo) += 1;
        *clone.get(&mut demo) += 1;
        assert_eq!(demo.right, 4);
        assert_eq!(demo.left, 1);
        assert!(format!("{accessor:?}").contains("StateAccessor"));
    }

    #[test]
    fn captured_indices_keep_instances_independent() {
        struct Rows {
            rows: Vec<u32>,
        }
        let accessors: Vec<StateAccessor<Rows, u32>> = (0..3)
            .map(|index| StateAccessor::new(move |view: &mut Rows| &mut view.rows[index]))
            .collect();
        let mut rows = Rows { rows: vec![0; 3] };
        for (index, accessor) in accessors.iter().enumerate() {
            *accessor.get(&mut rows) = index as u32 + 10;
        }
        assert_eq!(rows.rows, vec![10, 11, 12]);
    }
}
