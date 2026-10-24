/// A set of dirty entity indices within one archetype.
///
/// A sparse set: `dense` holds the member indices and `sparse[i]` the position of index `i` in
/// `dense`. Marking, unmarking, membership and the swap/swap-remove rewiring that mirrors the
/// archetype's own column operations are all O(1); clearing is O(len). [`sorted`](Self::sorted)
/// sorts the members in place without allocating, so the per-run dispatch of a dirty system costs
/// no heap traffic once the buffers have grown.
#[derive(Debug, Default, Clone)]
pub struct DirtySet {
    dense: Vec<usize>,
    sparse: Vec<usize>,
    sorted: bool,
}

const ABSENT: usize = usize::MAX;

impl DirtySet {
    /// Marks `index` as dirty. Returns `true` if it was not dirty before.
    pub fn mark(&mut self, index: usize) -> bool {
        if self.contains(index) {
            return false;
        }
        if self.sparse.len() <= index {
            self.sparse.resize(index + 1, ABSENT);
        }
        if self.dense.last().is_some_and(|&last| last > index) {
            self.sorted = false;
        }
        self.sparse[index] = self.dense.len();
        self.dense.push(index);
        true
    }

    /// Marks every index in `0..len` as dirty, replacing the previous contents.
    pub fn mark_all(&mut self, len: usize) {
        self.clear();
        self.dense.extend(0..len);
        if self.sparse.len() < len {
            self.sparse.resize(len, ABSENT);
        }
        self.sparse[..len].copy_from_slice(&self.dense);
        self.sorted = true;
    }

    /// Unmarks `index`. Returns `true` if it was dirty.
    pub fn unmark(&mut self, index: usize) -> bool {
        let Some(pos) = self.position(index) else {
            return false;
        };
        self.sparse[index] = ABSENT;
        self.dense.swap_remove(pos);
        if let Some(&moved) = self.dense.get(pos) {
            self.sparse[moved] = pos;
            self.sorted = false;
        }
        true
    }

    /// Whether `index` is dirty.
    #[inline]
    pub fn contains(&self, index: usize) -> bool {
        self.position(index).is_some()
    }

    /// The number of dirty indices.
    #[inline]
    pub fn len(&self) -> usize {
        self.dense.len()
    }

    /// Whether no index is dirty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.dense.is_empty()
    }

    /// Unmarks every index.
    pub fn clear(&mut self) {
        for &index in &self.dense {
            self.sparse[index] = ABSENT;
        }
        self.dense.clear();
        self.sorted = true;
    }

    /// The dirty indices in ascending order. Sorts in place if needed.
    pub fn sorted(&mut self) -> &[usize] {
        if !self.sorted {
            self.dense.sort_unstable();
            for (pos, &index) in self.dense.iter().enumerate() {
                self.sparse[index] = pos;
            }
            self.sorted = true;
        }
        &self.dense
    }

    /// Mirrors swapping the entities at `first` and `second`: each index keeps the dirty state
    /// of the entity that moves into it.
    pub fn swap(&mut self, first: usize, second: usize) {
        match (self.position(first), self.position(second)) {
            (Some(_), Some(_)) | (None, None) => {}
            (Some(pos), None) => self.relabel(pos, first, second),
            (None, Some(pos)) => self.relabel(pos, second, first),
        }
    }

    /// Mirrors `Vec::swap_remove(index)` on columns that were `len` long before the call: the
    /// entity at `index` is gone and the former last entity now lives at `index`.
    pub fn swap_remove(&mut self, index: usize, len: usize) {
        self.unmark(index);
        let last = len - 1;
        if last != index {
            if let Some(pos) = self.position(last) {
                self.relabel(pos, last, index);
            }
        }
    }

    fn position(&self, index: usize) -> Option<usize> {
        match self.sparse.get(index) {
            Some(&pos) if pos != ABSENT => Some(pos),
            _ => None,
        }
    }

    /// Replaces member `from` (stored at `pos`) by the non-member `to`.
    fn relabel(&mut self, pos: usize, from: usize, to: usize) {
        if self.sparse.len() <= to {
            self.sparse.resize(to + 1, ABSENT);
        }
        self.sparse[from] = ABSENT;
        self.sparse[to] = pos;
        self.dense[pos] = to;
        self.sorted = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn members(set: &mut DirtySet) -> Vec<usize> {
        set.sorted().to_vec()
    }

    #[test]
    fn mark_unmark_contains() {
        let mut set = DirtySet::default();
        assert!(set.mark(5));
        assert!(!set.mark(5));
        assert!(set.mark(1));
        assert!(set.contains(5) && set.contains(1) && !set.contains(0) && !set.contains(99));
        assert_eq!(members(&mut set), vec![1, 5]);
        assert!(set.unmark(1));
        assert!(!set.unmark(1));
        assert_eq!(members(&mut set), vec![5]);
        assert_eq!(set.len(), 1);
    }

    #[test]
    fn sorted_keeps_membership_consistent() {
        let mut set = DirtySet::default();
        for i in [9, 3, 7, 1] {
            set.mark(i);
        }
        assert_eq!(set.sorted(), &[1, 3, 7, 9]);
        // Positions were rebuilt by the sort; unmarking must still find the right slot.
        assert!(set.unmark(3));
        assert_eq!(members(&mut set), vec![1, 7, 9]);
    }

    #[test]
    fn clear_and_mark_all() {
        let mut set = DirtySet::default();
        set.mark(2);
        set.mark_all(4);
        assert_eq!(members(&mut set), vec![0, 1, 2, 3]);
        set.clear();
        assert!(set.is_empty() && !set.contains(2));
        set.mark(1);
        assert_eq!(members(&mut set), vec![1]);
    }

    #[test]
    fn swap_moves_dirty_state_with_the_entity() {
        let mut set = DirtySet::default();
        set.mark(0);
        set.swap(0, 3);
        assert_eq!(members(&mut set), vec![3]);
        set.mark(0);
        set.swap(0, 3); // both dirty: unchanged
        assert_eq!(members(&mut set), vec![0, 3]);
        set.swap(1, 2); // neither dirty: unchanged
        assert_eq!(members(&mut set), vec![0, 3]);
    }

    #[test]
    fn swap_remove_matches_vec_semantics() {
        // Model: entity at position i is dirty iff flags[i].
        let cases: &[(&[bool], usize)] = &[
            (&[true, false, true], 0),
            (&[true, false, true], 2),
            (&[false, true, false, true], 1),
            (&[true], 0),
            (&[false, false, true], 0),
        ];
        for &(flags, index) in cases {
            let mut set = DirtySet::default();
            for (i, &dirty) in flags.iter().enumerate() {
                if dirty {
                    set.mark(i);
                }
            }
            let mut model = flags.to_vec();
            model.swap_remove(index);
            set.swap_remove(index, flags.len());

            let expected: Vec<usize> = (0..model.len()).filter(|&i| model[i]).collect();
            assert_eq!(members(&mut set), expected, "flags={flags:?} index={index}");
            assert!(
                !set.contains(model.len()),
                "vacated last slot must not stay dirty"
            );
        }
    }
}
