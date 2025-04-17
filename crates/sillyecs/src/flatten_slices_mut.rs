use std::iter::FusedIterator;

/// A mutable iterator over a slice of slices.
///
/// Presents the inner slices as one contiguous set of mutable references.
pub struct FlattenSlicesMut<'a, T> {
    slices: Box<[&'a mut [T]]>,
    front: (usize, usize), // (slice index, element index)
}

impl<'a, T> FlattenSlicesMut<'a, T> {
    pub fn new<const N: usize>(slices: [&'a mut [T]; N]) -> Self {
        Self {
            slices: Box::new(slices),
            front: (0, 0),
        }
    }

    pub fn reset(&mut self) {
        self.front = (0, 0);
    }
}

impl<'a, T> Iterator for FlattenSlicesMut<'a, T> {
    type Item = &'a mut T;

    fn next(&mut self) -> Option<Self::Item> {
        const PREFETCH_THRESHOLD: usize = 4;

        while self.front.0 < self.slices.len() {
            let (slice_idx, elem_idx) = self.front;
            let slice = &mut self.slices[slice_idx];

            if elem_idx < slice.len() {
                // SAFETY: We return exactly one &mut reference per item,
                // and update `front` immediately after.
                let item = unsafe {
                    let ptr = slice.as_mut_ptr().add(elem_idx);
                    self.front.1 += 1;

                    if self.front.1 >= slice.len() {
                        self.front.0 += 1;
                        self.front.1 = 0;
                    }

                    // Prefetch next slice's start address if close to switching
                    #[cfg(all(target_arch = "x86_64", target_feature = "sse"))]
                    if slice.len() - elem_idx <= PREFETCH_THRESHOLD {
                        let next_idx = slice_idx + 1;
                        if next_idx < self.slices.len() {
                            let next = &self.slices[next_idx];
                            if !next.is_empty() {
                                #[allow(unused_unsafe)]
                                unsafe {
                                    const STRATEGY: i32 = core::arch::x86_64::_MM_HINT_T0;
                                    core::arch::x86_64::_mm_prefetch::<STRATEGY>(
                                        next.as_ptr() as *const i8
                                    );
                                }
                            }
                        }
                    }

                    &mut *ptr
                };

                return Some(item);
            } else {
                // Skip empty or exhausted slice
                self.front.0 += 1;
                self.front.1 = 0;
            }
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

impl<'a, T> ExactSizeIterator for FlattenSlicesMut<'a, T> {}
impl<'a, T> FusedIterator for FlattenSlicesMut<'a, T> {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_forward() {
        let s1 = &mut [1, 2][..];
        let s2 = &mut [3][..];
        let s3 = &mut [][..];
        let s4 = &mut [4, 5, 6][..];

        let mut iter = FlattenSlicesMut::new([s1, s2, s3, s4]);

        let size = iter.len();
        assert_eq!(size, 6);
        assert_eq!(iter.size_hint(), (6, Some(6)));

        // Mutate an entry
        let first = iter.next().unwrap();
        *first = 10;
        iter.reset();

        assert_eq!(iter.map(|a| *a).collect::<Vec<i32>>(), &[10, 2, 3, 4, 5, 6]);
    }
}
