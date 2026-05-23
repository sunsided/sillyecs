//! Minimal `izip!` macro used by code generated through `sillyecs-build`.
//!
//! Generated systems iterate component slices with a flat
//! zip-and-destructure expression. To keep the runtime crate
//! dependency-free we ship a small in-tree `izip!` instead of pulling in
//! `itertools`. The macro is structurally equivalent to the
//! `itertools::izip!` macro: an N-iterator zip whose item type is a flat
//! N-tuple `(a, b, c, ...)` rather than a right-nested
//! `(a, (b, (c, ...)))`.
//!
//! Implementation notes:
//! - The 1-argument and 2-argument arms produce a plain `IntoIterator`
//!   and `Iterator::zip` respectively, so the generated code stays as
//!   close as possible to a hand-written `xs.iter().zip(ys.iter())`.
//! - The 3+-argument arm chains `.zip(...)` calls and finishes with a
//!   `.map(...)` that flattens the right-nested tuple back into a flat
//!   tuple. The internal `@closure` arm builds that flattening closure
//!   by walking the remaining argument list.

/// Zip an arbitrary number of iterators into a single iterator yielding a
/// flat tuple of their items.
///
/// Mirrors the surface API of `itertools::izip!`. Each argument is
/// fed through [`IntoIterator::into_iter`], so slices, arrays, and
/// adapter iterators are all accepted.
///
/// # Examples
///
/// ```
/// use sillyecs::izip;
///
/// let xs = [1, 2, 3];
/// let ys = [10, 20, 30];
/// let mut zs = [100, 200, 300];
///
/// for (x, y, z) in izip!(xs.iter(), ys.iter(), zs.iter_mut()) {
///     *z += x + y;
/// }
/// assert_eq!(zs, [111, 222, 333]);
/// ```
#[macro_export]
macro_rules! izip {
    // Build the flattening closure: walk the remaining iterators and grow
    // the destructuring pattern / flat tuple in lockstep.
    (@closure $p:pat => $tup:expr) => {
        |$p| $tup
    };
    (@closure $p:pat => ( $($tup:tt)* ) , $_iter:expr $( , $tail:expr )*) => {
        $crate::izip!(@closure ($p, b) => ( $($tup)*, b ) $( , $tail )*)
    };

    // Single iterator: just turn it into an iterator.
    ($first:expr $(,)?) => {
        ::core::iter::IntoIterator::into_iter($first)
    };

    // Exactly two iterators: a plain `.zip()`, no flattening needed.
    ($first:expr, $second:expr $(,)?) => {
        $crate::izip!($first).zip($second)
    };

    // Three or more: chain `.zip(...)` calls and flatten the result
    // tuple back into a flat tuple via `.map(...)`.
    ($first:expr $( , $rest:expr )* $(,)?) => {
        $crate::izip!($first)
            $( .zip($rest) )*
            .map($crate::izip!(@closure a => (a) $( , $rest )*))
    };
}

#[cfg(test)]
mod tests {
    #[test]
    fn izip_single() {
        let xs = [1, 2, 3];
        let collected: Vec<&i32> = izip!(xs.iter()).collect();
        assert_eq!(collected, vec![&1, &2, &3]);
    }

    #[test]
    fn izip_two() {
        let xs = [1, 2, 3];
        let ys = [10, 20, 30];
        let collected: Vec<(&i32, &i32)> = izip!(xs.iter(), ys.iter()).collect();
        assert_eq!(collected, vec![(&1, &10), (&2, &20), (&3, &30)]);
    }

    #[test]
    fn izip_three_flat_tuple() {
        let xs = [1, 2];
        let ys = [10, 20];
        let zs = [100, 200];
        let collected: Vec<(&i32, &i32, &i32)> = izip!(xs.iter(), ys.iter(), zs.iter()).collect();
        assert_eq!(collected, vec![(&1, &10, &100), (&2, &20, &200)]);
    }

    #[test]
    fn izip_four_with_mut() {
        let xs = [1, 2];
        let ys = [10, 20];
        let zs = [100, 200];
        let mut ws = [0, 0];
        for (x, y, z, w) in izip!(xs.iter(), ys.iter(), zs.iter(), ws.iter_mut()) {
            *w = x + y + z;
        }
        assert_eq!(ws, [111, 222]);
    }

    #[test]
    fn izip_stops_at_shortest() {
        let xs = [1, 2, 3, 4];
        let ys = [10, 20];
        let collected: Vec<(&i32, &i32)> = izip!(xs.iter(), ys.iter()).collect();
        assert_eq!(collected, vec![(&1, &10), (&2, &20)]);
    }
}
