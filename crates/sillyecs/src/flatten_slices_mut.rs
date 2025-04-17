use std::iter::FusedIterator;

/// A mutable iterator over a slice of slices.
///
/// Presents the inner slices as one contiguous set of mutable references.
pub struct FlattenSlicesMut<'a, T> {
    slices: Box<[&'a mut [T]]>,
    front: (usize, usize),
    back: (usize, usize),
}

impl<'a, T> FlattenSlicesMut<'a, T> {
    pub fn new<const N: usize>(slices: [&'a mut [T]; N]) -> Self {
        let slices = Box::new(slices);
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

    fn compute_back(slices: &[&'a mut [T]]) -> (usize, usize) {
        for (i, s) in slices.iter().enumerate().rev() {
            if !s.is_empty() {
                return (i, s.len());
            }
        }
        (0, 0)
    }
}

impl<'a, T> Iterator for FlattenSlicesMut<'a, T> {
    type Item = &'a mut T;

    fn next(&mut self) -> Option<Self::Item> {
        while self.front < self.back {
            let (slice_idx, elem_idx) = self.front;

            if let Some(slice) = self.slices.get_mut(slice_idx) {
                if elem_idx < slice.len() {
                    let item = unsafe {
                        // Safety: we ensure unique access by advancing the front position immediately
                        let item = &mut *(&mut slice[elem_idx] as *mut T);
                        self.front.1 += 1;
                        if self.front.1 >= slice.len() {
                            self.front.0 += 1;
                            self.front.1 = 0;
                        }
                        item
                    };
                    return Some(item);
                } else {
                    self.front.0 += 1;
                    self.front.1 = 0;
                }
            } else {
                break;
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
