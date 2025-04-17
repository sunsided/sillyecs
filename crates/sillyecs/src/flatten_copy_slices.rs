use std::borrow::Cow;
use std::iter::FusedIterator;

/// An iterator over a slice of slices.
///
/// Presents the inner slices as one contiguous set of data.
#[derive(Debug)]
pub struct FlattenCopySlices<'a, T>
where
    T: Copy,
{
    slices: Cow<'a, [&'a [T]]>,
    front: (usize, usize), // (slice index, element index)
}

impl<'a, T> FlattenCopySlices<'a, T>
where
    T: Copy,
{
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

impl<'a, T> Iterator for FlattenCopySlices<'a, T>
where
    T: Copy,
{
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        while self.front.0 < self.slices.len() {
            let (slice_idx, elem_idx) = self.front;
            let slice = &self.slices[slice_idx];

            if elem_idx < slice.len() {
                self.front.1 += 1;

                if self.front.1 >= slice.len() {
                    self.front.0 += 1;
                    self.front.1 = 0;
                }

                return Some(slice[elem_idx]);
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

impl<'a, T> ExactSizeIterator for FlattenCopySlices<'a, T> where T: Copy {}
impl<'a, T> FusedIterator for FlattenCopySlices<'a, T> where T: Copy {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_forward() {
        let s1 = &[1, 2][..];
        let s2 = &[3][..];
        let s3 = &[][..];
        let s4 = &[4, 5, 6][..];

        let iter = FlattenCopySlices::new([s1, s2, s3, s4]);

        let size = iter.len();
        assert_eq!(size, 6);
        assert_eq!(iter.size_hint(), (6, Some(6)));

        assert_eq!(iter.collect::<Vec<i32>>(), &[1, 2, 3, 4, 5, 6]);
    }
}
