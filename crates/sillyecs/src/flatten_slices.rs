use std::borrow::Cow;

/// An iterator over a slice of slices.
///
/// Presents the inner slices as one contiguous set of data.
#[derive(Debug)]
pub struct FlattenSlices<'a, T> {
    slices: Cow<'a, [&'a [T]]>,
    front: (usize, usize),
    back: (usize, usize),
}

impl<'a, T> FlattenSlices<'a, T> {
    pub fn new<const N: usize>(slices: [&'a [T]; N]) -> Self {
        let slices: Cow<'_, [&'a [T]]> = Cow::Owned(slices.into());
        let mut back = (0, 0);
        for (i, s) in slices.iter().enumerate().rev() {
            if !s.is_empty() {
                back = (i, s.len());
                break;
            }
        }

        Self {
            slices,
            front: (0, 0),
            back,
        }
    }

    pub fn reset(&mut self) {
        self.front = (0, 0);
        self.back = Self::compute_back(&self.slices);
    }

    fn compute_back(slices: &[&'a [T]]) -> (usize, usize) {
        for (i, s) in slices.iter().enumerate().rev() {
            if !s.is_empty() {
                return (i, s.len());
            }
        }
        (0, 0)
    }
}

impl<'a, T> core::iter::Iterator for FlattenSlices<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        while self.front < self.back {
            let (slice_idx, elem_idx) = self.front;
            let slice = &self.slices[slice_idx];
            if elem_idx < slice.len() {
                let item = &slice[elem_idx];
                self.front.1 += 1;
                if self.front.1 >= slice.len() {
                    self.front.0 += 1;
                    self.front.1 = 0;
                }
                return Some(item);
            } else {
                self.front.0 += 1;
                self.front.1 = 0;
            }
        }
        None
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let mut count = 0;
        for i in self.front.0..=self.back.0 {
            let slice = &self.slices[i];
            let start = if i == self.front.0 { self.front.1 } else { 0 };
            let end = if i == self.back.0 {
                self.back.1
            } else {
                slice.len()
            };
            if end > start {
                count += end - start;
            }
        }
        (count, Some(count))
    }
}

impl<'a, T> core::iter::DoubleEndedIterator for FlattenSlices<'a, T> {
    fn next_back(&mut self) -> Option<Self::Item> {
        while self.front < self.back {
            if self.back.1 > 0 {
                self.back.1 -= 1;
            } else {
                if self.back.0 == 0 {
                    return None;
                }
                self.back.0 -= 1;
                self.back.1 = self.slices[self.back.0].len();
                if self.back.1 == 0 {
                    continue;
                }
                self.back.1 -= 1;
            }
            return Some(&self.slices[self.back.0][self.back.1]);
        }
        None
    }
}

impl<'a, T> core::iter::ExactSizeIterator for FlattenSlices<'a, T> {}
impl<'a, T> core::iter::FusedIterator for FlattenSlices<'a, T> {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_forward() {
        let s1 = &[1, 2][..];
        let s2 = &[3][..];
        let s3 = &[][..];
        let s4 = &[4, 5, 6][..];

        let iter = FlattenSlices::new([s1, s2, s3, s4]);

        let size = iter.len();
        assert_eq!(size, 6);
        assert_eq!(iter.size_hint(), (6, Some(6)));

        assert_eq!(iter.copied().collect::<Vec<i32>>(), &[1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn test_reverse() {
        let s1 = &[1, 2][..];
        let s2 = &[3][..];
        let s3 = &[][..];
        let s4 = &[4, 5, 6][..];

        let iter = FlattenSlices::new([s1, s2, s3, s4]);
        assert_eq!(
            iter.rev().copied().collect::<Vec<i32>>(),
            &[6, 5, 4, 3, 2, 1]
        );
    }
}
