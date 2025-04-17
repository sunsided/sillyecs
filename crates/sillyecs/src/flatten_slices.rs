use std::borrow::Cow;
use std::iter::FusedIterator;

/// An iterator over a slice of slices.
///
/// Presents the inner slices as one contiguous set of data.
#[derive(Debug)]
pub struct FlattenSlices<'a, T> {
    slices: Cow<'a, [&'a [T]]>,
    front: (usize, usize), // (slice index, element index)
}

impl<'a, T> FlattenSlices<'a, T> {
    pub fn new<const N: usize>(slices: [&'a [T]; N]) -> Self {
        let slices = Cow::Owned(slices.into());
        Self {
            slices,
            front: (0, 0),
        }
    }

    pub fn reset(&mut self) {
        self.front = (0, 0);
    }
}

#[cfg(all(target_arch = "x86_64", target_feature = "sse", feature = "prefetch"))]
pub(crate) const PREFETCH_THRESHOLD: usize = 4;

impl<'a, T> Iterator for FlattenSlices<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        while self.front.0 < self.slices.len() {
            let (slice_idx, elem_idx) = self.front;
            let slice = &self.slices[slice_idx];

            if elem_idx < slice.len() {
                let item = &slice[elem_idx];
                self.front.1 += 1;

                if self.front.1 >= slice.len() {
                    self.front.0 += 1;
                    self.front.1 = 0;
                }

                // Prefetch next slice if we're near the end of the current one
                #[cfg(all(target_arch = "x86_64", target_feature = "sse", feature = "prefetch"))]
                if slice.len() - elem_idx <= PREFETCH_THRESHOLD {
                    let next_idx = slice_idx + 1;
                    if next_idx < self.slices.len() {
                        let next_slice = &self.slices[next_idx];
                        if !next_slice.is_empty() {
                            unsafe {
                                const STRATEGY: i32 = core::arch::x86_64::_MM_HINT_T0;
                                core::arch::x86_64::_mm_prefetch::<STRATEGY>(
                                    next_slice.as_ptr() as *const i8
                                );
                            }
                        }
                    }
                }

                return Some(item);
            }

            self.front.0 += 1;
            self.front.1 = 0;
        }

        None
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let mut count = 0;
        for i in self.front.0..self.slices.len() {
            let slice = &self.slices[i];
            let start = if i == self.front.0 { self.front.1 } else { 0 };
            count += slice.len().saturating_sub(start);
        }
        (count, Some(count))
    }
}

impl<'a, T> ExactSizeIterator for FlattenSlices<'a, T> {}
impl<'a, T> FusedIterator for FlattenSlices<'a, T> {}

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
}
