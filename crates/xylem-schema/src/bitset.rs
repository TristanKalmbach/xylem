//! A small fixed-width bit set used for automaton state sets.

/// Fixed-capacity bit set. All sets belonging to one automaton share a width.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Default)]
pub(crate) struct BitSet {
    words: Vec<u64>,
}

impl BitSet {
    pub(crate) fn new(bits: usize) -> Self {
        Self {
            words: vec![0; bits.div_ceil(64).max(1)],
        }
    }

    pub(crate) fn insert(&mut self, bit: usize) {
        self.words[bit / 64] |= 1 << (bit % 64);
    }

    pub(crate) fn contains(&self, bit: usize) -> bool {
        self.words
            .get(bit / 64)
            .is_some_and(|w| w & (1 << (bit % 64)) != 0)
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.words.iter().all(|&w| w == 0)
    }

    pub(crate) fn union_with(&mut self, other: &BitSet) {
        for (a, b) in self.words.iter_mut().zip(&other.words) {
            *a |= *b;
        }
    }

    pub(crate) fn intersect_with(&mut self, other: &BitSet) {
        for (a, b) in self.words.iter_mut().zip(&other.words) {
            *a &= *b;
        }
    }

    pub(crate) fn intersects(&self, other: &BitSet) -> bool {
        self.words.iter().zip(&other.words).any(|(a, b)| a & b != 0)
    }

    /// Iterates set bits in ascending order.
    pub(crate) fn iter(&self) -> impl Iterator<Item = usize> + '_ {
        self.words.iter().enumerate().flat_map(|(i, &w)| {
            let mut w = w;
            std::iter::from_fn(move || {
                if w == 0 {
                    return None;
                }
                let bit = w.trailing_zeros() as usize;
                w &= w - 1;
                Some(i * 64 + bit)
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_operations() {
        let mut a = BitSet::new(130);
        a.insert(1);
        a.insert(129);
        let mut b = BitSet::new(130);
        b.insert(129);
        b.insert(64);
        assert!(a.intersects(&b));
        let mut c = a.clone();
        c.intersect_with(&b);
        assert_eq!(c.iter().collect::<Vec<_>>(), vec![129]);
        a.union_with(&b);
        assert_eq!(a.iter().collect::<Vec<_>>(), vec![1, 64, 129]);
        assert!(!a.is_empty());
        assert!(BitSet::new(3).is_empty());
        assert!(!a.contains(2));
    }
}
