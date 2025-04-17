use criterion::{Criterion, black_box, criterion_group, criterion_main};
use rand::Rng;

const NUM_ARCHETYPES: usize = 7;
const COMPONENTS_PER_ARCHETYPE: usize = 1024;

#[derive(Debug, Clone, Copy)]
struct Component(f32);

// Flat Iterator (baseline)

fn flat_slice_iter(data: &[Component]) -> f32 {
    let mut sum = 0.0;
    for c in data {
        sum += c.0;
    }
    sum
}

// FlattenSlices without prefetch

pub struct FlattenSlices<'a, T> {
    slices: [&'a [T]; NUM_ARCHETYPES],
    front: (usize, usize),
}

impl<'a, T> FlattenSlices<'a, T> {
    pub fn new(slices: [&'a [T]; NUM_ARCHETYPES]) -> Self {
        Self {
            slices,
            front: (0, 0),
        }
    }
}

impl<'a, T> Iterator for FlattenSlices<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        while self.front.0 < self.slices.len() {
            let (slice_idx, elem_idx) = self.front;
            let slice = self.slices[slice_idx];

            if elem_idx < slice.len() {
                self.front.1 += 1;
                if self.front.1 >= slice.len() {
                    self.front.0 += 1;
                    self.front.1 = 0;
                }
                return Some(&slice[elem_idx]);
            } else {
                self.front.0 += 1;
                self.front.1 = 0;
            }
        }
        None
    }
}

// With prefetch

pub struct FlattenSlicesPrefetch<'a, T> {
    slices: [&'a [T]; NUM_ARCHETYPES],
    front: (usize, usize),
}

impl<'a, T> FlattenSlicesPrefetch<'a, T> {
    pub fn new(slices: [&'a [T]; NUM_ARCHETYPES]) -> Self {
        Self {
            slices,
            front: (0, 0),
        }
    }
}

impl<'a, T> Iterator for FlattenSlicesPrefetch<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        const PREFETCH_THRESHOLD: usize = 4;

        while self.front.0 < self.slices.len() {
            let (slice_idx, elem_idx) = self.front;
            let slice = self.slices[slice_idx];

            if elem_idx < slice.len() {
                self.front.1 += 1;
                if self.front.1 >= slice.len() {
                    self.front.0 += 1;
                    self.front.1 = 0;
                }

                // Prefetch the next slice start
                if slice.len() - elem_idx <= PREFETCH_THRESHOLD {
                    let next_idx = slice_idx + 1;
                    if next_idx < self.slices.len() {
                        let next = self.slices[next_idx];
                        if !next.is_empty() {
                            #[cfg(all(target_arch = "x86_64", target_feature = "sse"))]
                            unsafe {
                                const STRATEGY: i32 = core::arch::x86_64::_MM_HINT_T0;
                                core::arch::x86_64::_mm_prefetch::<STRATEGY>(
                                    next.as_ptr() as *const i8
                                );
                            }
                        }
                    }
                }

                return Some(&slice[elem_idx]);
            } else {
                self.front.0 += 1;
                self.front.1 = 0;
            }
        }

        None
    }
}

fn benchmark_flatten_slices(c: &mut Criterion) {
    let mut group = c.benchmark_group("flatten_slices");

    // Generate data
    let mut rng = rand::rng();
    let archetypes: [[Component; COMPONENTS_PER_ARCHETYPE]; NUM_ARCHETYPES] =
        std::array::from_fn(|_| std::array::from_fn(|_| Component(rng.random())));

    let slice_refs: [&[Component]; NUM_ARCHETYPES] =
        std::array::from_fn::<&[Component], NUM_ARCHETYPES, _>(|i| &archetypes[i]);

    let flat_ref: Vec<Component> = slice_refs.iter().flat_map(|s| *s).copied().collect();

    group.bench_function("flat &[T] baseline", |b| {
        b.iter(|| black_box(flat_slice_iter(&flat_ref)))
    });

    group.bench_function("FlattenSlices (no prefetch)", |b| {
        b.iter(|| {
            let iter = FlattenSlices::new(slice_refs);
            let sum: f32 = iter.map(|c| c.0).sum();
            black_box(sum)
        });
    });

    group.bench_function("FlattenSlices (with prefetch)", |b| {
        b.iter(|| {
            let iter = FlattenSlicesPrefetch::new(slice_refs);
            let sum: f32 = iter.map(|c| c.0).sum();
            black_box(sum)
        });
    });

    group.finish();
}

criterion_group!(benches, benchmark_flatten_slices);
criterion_main!(benches);
