use crate::Rect;

const MAX_NODE_ENTRIES: usize = 12;
const SPLIT_ENTRY_COUNT: usize = MAX_NODE_ENTRIES + 1;
const MIN_NODE_ENTRIES: usize = MAX_NODE_ENTRIES / 2;
const UNASSIGNED_GROUP: u8 = u8::MAX;

#[derive(Clone, Copy, Debug)]
struct LeafEntry {
    bounds: Rect,
    order: u32,
}

impl LeafEntry {
    const EMPTY: Self = Self {
        bounds: Rect::ZERO,
        order: 0,
    };
}

#[derive(Clone, Debug)]
enum NodeKind {
    Leaf {
        entries: [LeafEntry; MAX_NODE_ENTRIES],
        len: usize,
    },
    Branch {
        children: [usize; MAX_NODE_ENTRIES],
        len: usize,
    },
}

#[derive(Clone, Debug)]
struct Node {
    parent: Option<usize>,
    bounds: Rect,
    max_order: u32,
    kind: NodeKind,
}

impl Node {
    fn leaf(parent: Option<usize>) -> Self {
        Self {
            parent,
            bounds: Rect::ZERO,
            max_order: 0,
            kind: NodeKind::Leaf {
                entries: [LeafEntry::EMPTY; MAX_NODE_ENTRIES],
                len: 0,
            },
        }
    }

    fn branch(parent: Option<usize>) -> Self {
        Self {
            parent,
            bounds: Rect::ZERO,
            max_order: 0,
            kind: NodeKind::Branch {
                children: [usize::MAX; MAX_NODE_ENTRIES],
                len: 0,
            },
        }
    }
}

/// Append-only overlap index used to derive a safe renderer draw order.
///
/// The tree is rebuilt for each display list, but [`Self::clear`] retains every allocation. An
/// inserted rectangle receives one more than the greatest order of any earlier rectangle it
/// intersects. Rectangles at the same order are therefore guaranteed not to overlap and may be
/// reordered by primitive kind or texture for batching.
#[derive(Clone, Debug, Default)]
pub(crate) struct BoundsOrderTree {
    root: Option<usize>,
    nodes: Vec<Node>,
    query_stack: Vec<usize>,
}

impl BoundsOrderTree {
    pub(crate) fn allocated_bytes(&self) -> usize {
        self.nodes.capacity() * size_of::<Node>() + self.query_stack.capacity() * size_of::<usize>()
    }

    pub(crate) fn with_capacity(primitives: usize) -> Self {
        let leaf_nodes = primitives.div_ceil(MAX_NODE_ENTRIES);
        Self {
            root: None,
            nodes: Vec::with_capacity(leaf_nodes.saturating_add(2)),
            query_stack: Vec::with_capacity(16),
        }
    }

    pub(crate) fn clear(&mut self) {
        self.root = None;
        self.nodes.clear();
        self.query_stack.clear();
    }

    /// Inserts finite, non-empty bounds and returns their minimum safe draw order.
    ///
    /// Invalid bounds are ignored and receive order zero. Scene construction filters those
    /// primitives before calling this method; keeping this guard here makes the spatial index
    /// robust for low-level callers as well.
    pub(crate) fn insert(&mut self, bounds: Rect) -> u32 {
        if !valid_bounds(bounds) {
            return 0;
        }

        let order = self
            .max_intersecting_order(bounds)
            .map_or(0, |order| order.saturating_add(1));
        let entry = LeafEntry { bounds, order };

        let Some(_) = self.root else {
            let mut root = Node::leaf(None);
            if let NodeKind::Leaf { entries, len } = &mut root.kind {
                entries[0] = entry;
                *len = 1;
            }
            root.bounds = bounds;
            root.max_order = order;
            self.nodes.push(root);
            self.root = Some(0);
            return order;
        };

        let leaf = self.choose_leaf(bounds);
        let has_room = matches!(
            &self.nodes[leaf].kind,
            NodeKind::Leaf { len, .. } if *len < MAX_NODE_ENTRIES
        );
        if has_room {
            if let NodeKind::Leaf { entries, len } = &mut self.nodes[leaf].kind {
                entries[*len] = entry;
                *len += 1;
            }
            self.refit_upward(leaf);
        } else {
            self.split_leaf(leaf, entry);
        }
        order
    }

    fn max_intersecting_order(&mut self, bounds: Rect) -> Option<u32> {
        self.query_stack.clear();
        self.query_stack.extend(self.root);
        let mut best = None;

        while let Some(index) = self.query_stack.pop() {
            let node = &self.nodes[index];
            if !node.bounds.intersects(bounds) || best.is_some_and(|best| node.max_order <= best) {
                continue;
            }
            match &node.kind {
                NodeKind::Leaf { entries, len } => {
                    for entry in &entries[..*len] {
                        if entry.bounds.intersects(bounds)
                            && best.is_none_or(|best| entry.order > best)
                        {
                            best = Some(entry.order);
                        }
                    }
                }
                NodeKind::Branch { children, len } => {
                    for child_index in &children[..*len] {
                        let child = &self.nodes[*child_index];
                        if child.bounds.intersects(bounds)
                            && best.is_none_or(|best| child.max_order > best)
                        {
                            self.query_stack.push(*child_index);
                        }
                    }
                }
            }
        }
        best
    }

    fn choose_leaf(&self, bounds: Rect) -> usize {
        let mut index = self.root.expect("a non-empty tree has a root");
        loop {
            let NodeKind::Branch { children, len } = &self.nodes[index].kind else {
                return index;
            };
            index = children[..*len]
                .iter()
                .copied()
                .min_by(|left, right| {
                    let left_bounds = self.nodes[*left].bounds;
                    let right_bounds = self.nodes[*right].bounds;
                    enlargement(left_bounds, bounds)
                        .total_cmp(&enlargement(right_bounds, bounds))
                        .then_with(|| area(left_bounds).total_cmp(&area(right_bounds)))
                })
                .expect("branch nodes are never empty");
        }
    }

    fn split_leaf(&mut self, index: usize, incoming: LeafEntry) {
        let (parent, mut entries, len) = match &self.nodes[index].kind {
            NodeKind::Leaf { entries, len } => {
                let mut expanded = [LeafEntry::EMPTY; SPLIT_ENTRY_COUNT];
                expanded[..*len].copy_from_slice(&entries[..*len]);
                expanded[*len] = incoming;
                (self.nodes[index].parent, expanded, *len + 1)
            }
            NodeKind::Branch { .. } => unreachable!("only leaf nodes accept leaf entries"),
        };
        let mut bounds = [Rect::ZERO; SPLIT_ENTRY_COUNT];
        for (destination, entry) in bounds.iter_mut().zip(entries.iter()).take(len) {
            *destination = entry.bounds;
        }
        let groups = partition(&bounds, len);

        self.nodes[index] = Node::leaf(parent);
        let sibling = self.nodes.len();
        self.nodes.push(Node::leaf(parent));
        for entry_index in 0..len {
            let destination = if groups[entry_index] == 0 {
                index
            } else {
                sibling
            };
            let entry = entries[entry_index];
            if let NodeKind::Leaf { entries, len } = &mut self.nodes[destination].kind {
                entries[*len] = entry;
                *len += 1;
            }
        }
        // Avoid retaining accidental references if this code is changed to a non-Copy entry.
        entries.fill(LeafEntry::EMPTY);
        self.recalculate(index);
        self.recalculate(sibling);
        self.attach_split(index, sibling);
    }

    fn split_branch(&mut self, index: usize, incoming_child: usize) {
        let (parent, children, len) = match &self.nodes[index].kind {
            NodeKind::Branch { children, len } => {
                let mut expanded = [usize::MAX; SPLIT_ENTRY_COUNT];
                expanded[..*len].copy_from_slice(&children[..*len]);
                expanded[*len] = incoming_child;
                (self.nodes[index].parent, expanded, *len + 1)
            }
            NodeKind::Leaf { .. } => unreachable!("only branch nodes accept child nodes"),
        };
        let mut bounds = [Rect::ZERO; SPLIT_ENTRY_COUNT];
        for entry_index in 0..len {
            bounds[entry_index] = self.nodes[children[entry_index]].bounds;
        }
        let groups = partition(&bounds, len);

        self.nodes[index] = Node::branch(parent);
        let sibling = self.nodes.len();
        self.nodes.push(Node::branch(parent));
        for entry_index in 0..len {
            let destination = if groups[entry_index] == 0 {
                index
            } else {
                sibling
            };
            let child = children[entry_index];
            if let NodeKind::Branch { children, len } = &mut self.nodes[destination].kind {
                children[*len] = child;
                *len += 1;
            }
            self.nodes[child].parent = Some(destination);
        }
        self.recalculate(index);
        self.recalculate(sibling);
        self.attach_split(index, sibling);
    }

    fn attach_split(&mut self, original: usize, sibling: usize) {
        let Some(parent) = self.nodes[original].parent else {
            let root = self.nodes.len();
            let mut node = Node::branch(None);
            if let NodeKind::Branch { children, len } = &mut node.kind {
                children[0] = original;
                children[1] = sibling;
                *len = 2;
            }
            self.nodes[original].parent = Some(root);
            self.nodes[sibling].parent = Some(root);
            self.nodes.push(node);
            self.recalculate(root);
            self.root = Some(root);
            return;
        };

        let has_room = matches!(
            &self.nodes[parent].kind,
            NodeKind::Branch { len, .. } if *len < MAX_NODE_ENTRIES
        );
        if has_room {
            if let NodeKind::Branch { children, len } = &mut self.nodes[parent].kind {
                children[*len] = sibling;
                *len += 1;
            }
            self.nodes[sibling].parent = Some(parent);
            self.refit_upward(parent);
        } else {
            self.split_branch(parent, sibling);
        }
    }

    fn refit_upward(&mut self, mut index: usize) {
        loop {
            self.recalculate(index);
            let Some(parent) = self.nodes[index].parent else {
                break;
            };
            index = parent;
        }
    }

    fn recalculate(&mut self, index: usize) {
        let (bounds, max_order) = match &self.nodes[index].kind {
            NodeKind::Leaf { entries, len } => {
                debug_assert!(*len > 0);
                let mut bounds = entries[0].bounds;
                let mut max_order = entries[0].order;
                for entry in &entries[1..*len] {
                    bounds = union(bounds, entry.bounds);
                    max_order = max_order.max(entry.order);
                }
                (bounds, max_order)
            }
            NodeKind::Branch { children, len } => {
                debug_assert!(*len > 0);
                let mut bounds = self.nodes[children[0]].bounds;
                let mut max_order = self.nodes[children[0]].max_order;
                for child in &children[1..*len] {
                    bounds = union(bounds, self.nodes[*child].bounds);
                    max_order = max_order.max(self.nodes[*child].max_order);
                }
                (bounds, max_order)
            }
        };
        self.nodes[index].bounds = bounds;
        self.nodes[index].max_order = max_order;
    }
}

fn partition<const N: usize>(bounds: &[Rect; N], len: usize) -> [u8; N] {
    debug_assert!(len >= 2);
    let mut first_seed = 0;
    let mut second_seed = 1;
    let mut greatest_waste = f64::NEG_INFINITY;
    for first in 0..len - 1 {
        for second in first + 1..len {
            let waste = area(union(bounds[first], bounds[second]))
                - area(bounds[first])
                - area(bounds[second]);
            if waste > greatest_waste {
                greatest_waste = waste;
                first_seed = first;
                second_seed = second;
            }
        }
    }

    let mut groups = [UNASSIGNED_GROUP; N];
    groups[first_seed] = 0;
    groups[second_seed] = 1;
    let mut group_bounds = [bounds[first_seed], bounds[second_seed]];
    let mut group_lengths = [1_usize, 1_usize];
    let mut remaining = len - 2;

    while remaining > 0 {
        if group_lengths[0] + remaining == MIN_NODE_ENTRIES {
            assign_remaining(
                &mut groups,
                bounds,
                len,
                0,
                &mut group_bounds,
                &mut group_lengths,
            );
            break;
        }
        if group_lengths[1] + remaining == MIN_NODE_ENTRIES {
            assign_remaining(
                &mut groups,
                bounds,
                len,
                1,
                &mut group_bounds,
                &mut group_lengths,
            );
            break;
        }

        let next = (0..len)
            .filter(|index| groups[*index] == UNASSIGNED_GROUP)
            .max_by(|left, right| {
                let left_difference = (enlargement(group_bounds[0], bounds[*left])
                    - enlargement(group_bounds[1], bounds[*left]))
                .abs();
                let right_difference = (enlargement(group_bounds[0], bounds[*right])
                    - enlargement(group_bounds[1], bounds[*right]))
                .abs();
                left_difference.total_cmp(&right_difference)
            })
            .expect("remaining entries contain an unassigned item");
        let first_enlargement = enlargement(group_bounds[0], bounds[next]);
        let second_enlargement = enlargement(group_bounds[1], bounds[next]);
        let group = if first_enlargement < second_enlargement {
            0
        } else if second_enlargement < first_enlargement {
            1
        } else {
            let first_area = area(group_bounds[0]);
            let second_area = area(group_bounds[1]);
            if first_area < second_area {
                0
            } else if second_area < first_area {
                1
            } else if group_lengths[0] <= group_lengths[1] {
                0
            } else {
                1
            }
        };
        groups[next] = group as u8;
        group_bounds[group] = union(group_bounds[group], bounds[next]);
        group_lengths[group] += 1;
        remaining -= 1;
    }

    debug_assert!(groups[..len].iter().all(|group| *group <= 1));
    debug_assert!(
        group_lengths
            .iter()
            .all(|length| *length >= MIN_NODE_ENTRIES)
    );
    groups
}

fn assign_remaining<const N: usize>(
    groups: &mut [u8; N],
    bounds: &[Rect; N],
    len: usize,
    group: usize,
    group_bounds: &mut [Rect; 2],
    group_lengths: &mut [usize; 2],
) {
    for index in 0..len {
        if groups[index] == UNASSIGNED_GROUP {
            groups[index] = group as u8;
            group_bounds[group] = union(group_bounds[group], bounds[index]);
            group_lengths[group] += 1;
        }
    }
}

pub(crate) fn valid_bounds(bounds: Rect) -> bool {
    !bounds.is_empty()
        && bounds.x.is_finite()
        && bounds.y.is_finite()
        && bounds.width.is_finite()
        && bounds.height.is_finite()
        && bounds.right().is_finite()
        && bounds.bottom().is_finite()
}

fn union(first: Rect, second: Rect) -> Rect {
    let left = first.x.min(second.x);
    let top = first.y.min(second.y);
    let right = first.right().max(second.right());
    let bottom = first.bottom().max(second.bottom());
    Rect::new(left, top, right - left, bottom - top)
}

fn area(bounds: Rect) -> f64 {
    f64::from(bounds.width) * f64::from(bounds.height)
}

fn enlargement(existing: Rect, incoming: Rect) -> f64 {
    area(union(existing, incoming)) - area(existing)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disjoint_bounds_share_an_order_and_overlaps_advance() {
        let mut tree = BoundsOrderTree::default();
        assert_eq!(tree.insert(Rect::new(0.0, 0.0, 10.0, 10.0)), 0);
        assert_eq!(tree.insert(Rect::new(20.0, 0.0, 10.0, 10.0)), 0);
        assert_eq!(tree.insert(Rect::new(5.0, 0.0, 20.0, 10.0)), 1);
        assert_eq!(tree.insert(Rect::new(8.0, 0.0, 2.0, 10.0)), 2);
        assert_eq!(tree.insert(Rect::new(40.0, 0.0, 2.0, 2.0)), 0);
    }

    #[test]
    fn touching_edges_do_not_force_another_draw_order() {
        let mut tree = BoundsOrderTree::default();
        assert_eq!(tree.insert(Rect::new(0.0, 0.0, 10.0, 10.0)), 0);
        assert_eq!(tree.insert(Rect::new(10.0, 0.0, 10.0, 10.0)), 0);
    }

    #[test]
    fn invalid_bounds_are_ignored() {
        let mut tree = BoundsOrderTree::default();
        assert_eq!(tree.insert(Rect::new(f32::NAN, 0.0, 10.0, 10.0)), 0);
        assert!(tree.root.is_none());
        assert_eq!(tree.insert(Rect::new(0.0, 0.0, 0.0, 10.0)), 0);
        assert!(tree.root.is_none());
    }

    #[test]
    fn rtree_orders_match_a_linear_reference_across_splits() {
        let mut tree = BoundsOrderTree::with_capacity(800);
        let mut reference = Vec::<(Rect, u32)>::new();
        let mut state = 0x8d26_4ad1_5f3b_91e7_u64;

        for _ in 0..800 {
            let x = random_unit(&mut state) * 900.0 - 200.0;
            let y = random_unit(&mut state) * 700.0 - 100.0;
            let width = 1.0 + random_unit(&mut state) * 150.0;
            let height = 1.0 + random_unit(&mut state) * 120.0;
            let bounds = Rect::new(x, y, width, height);
            let expected = reference
                .iter()
                .filter(|(previous, _)| previous.intersects(bounds))
                .map(|(_, order)| *order)
                .max()
                .map_or(0, |order| order.saturating_add(1));
            let actual = tree.insert(bounds);
            assert_eq!(actual, expected, "mismatched order for {bounds:?}");
            reference.push((bounds, expected));
        }
    }

    #[test]
    fn clear_retains_tree_and_query_allocations() {
        let mut tree = BoundsOrderTree::with_capacity(128);
        for index in 0..128 {
            tree.insert(Rect::new(index as f32, 0.0, 8.0, 8.0));
        }
        let node_capacity = tree.nodes.capacity();
        let stack_capacity = tree.query_stack.capacity();
        tree.clear();
        assert!(tree.root.is_none());
        assert!(tree.nodes.is_empty());
        assert_eq!(tree.nodes.capacity(), node_capacity);
        assert_eq!(tree.query_stack.capacity(), stack_capacity);
    }

    fn random_unit(state: &mut u64) -> f32 {
        *state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((*state >> 40) as u32) as f32 / (u32::MAX >> 8) as f32
    }
}
