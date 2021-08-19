use atomic::Atomic;
use std::{
    cell::RefCell,
    mem::{ManuallyDrop, MaybeUninit},
    ops::{Deref, DerefMut},
    sync::{atomic::Ordering, Arc},
};

// Present this enum as a single u16 for the purposes of atomic operations
// That way both of the fields must be updated simultaneously
#[repr(C, align(2))]
#[derive(Clone, Copy)]
struct BucketInfo {
    acquired: u8,
    canonical: u8,
}

static_assertions::const_assert!(Atomic::<BucketInfo>::is_lock_free());

/// an unsplit MontyHall
pub struct MontyHall<T> {
    buckets: [RefCell<MaybeUninit<T>>; 3],
    bucket_info: Atomic<BucketInfo>,
}

/// A reference to the element stored in the MontyHall at a certain point in time
pub struct MontyHallHandle<'a, T> {
    acquired: usize,
    monte_hall: &'a MontyHall<T>,
}

impl<'a, T> Deref for MontyHallHandle<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.monte_hall.buckets[self.acquired].borrow().as_ptr() }
    }
}

impl<'a, T> DerefMut for MontyHallHandle<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe {
            &mut *self.monte_hall.buckets[self.acquired]
                .borrow_mut()
                .as_mut_ptr()
        }
    }
}

impl<'a, T> Drop for MontyHallHandle<'a, T> {
    fn drop(&mut self) {
        // Take ownership of the value, and drop it if we turned out to be holding a non-canonical reference
        let value = ManuallyDrop::new(unsafe {
            self.monte_hall.buckets[self.acquired]
                .replace(MaybeUninit::uninit())
                .assume_init()
        });

        let old_info = self
            .monte_hall
            .bucket_info
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |bucket_info| {
                Some(BucketInfo {
                    acquired: u8::MAX,
                    ..bucket_info
                })
            })
            .or(Err(()))
            .unwrap();

        // We were holding onto a non-canonical handle, so drop it
        if old_info.acquired != old_info.canonical {
            println!("Dropped non-canonical value {}", old_info.acquired);
            ManuallyDrop::into_inner(value);
        }

        // Otherwise, the existing value continues to live
    }
}

impl<T> MontyHall<T> {
    /// Create a new MontyHall containing some value
    pub fn new(value: T) -> Self {
        MontyHall {
            buckets: [
                RefCell::new(MaybeUninit::new(value)),
                RefCell::new(MaybeUninit::uninit()),
                RefCell::new(MaybeUninit::uninit()),
            ],
            bucket_info: Atomic::new(BucketInfo {
                acquired: u8::MAX,
                canonical: 0,
            }),
        }
    }

    /// Create a new MontyHall containing the default value
    pub fn with_default() -> Self
    where
        T: Default,
    {
        Self::new(Default::default())
    }

    /// Split the MontyHall into its sender and receiver halves
    pub fn split(self) -> (MontyHallWrite<T>, MontyHallRead<T>) {
        let arc = Arc::new(self);

        (MontyHallWrite(arc.clone()), MontyHallRead(arc))
    }

    /// Load the value from the MontyHall
    /// SAFETY: you must be the only reader
    pub(self) unsafe fn load(&self) -> MontyHallHandle<'_, T> {
        let BucketInfo { canonical, .. } = self
            .bucket_info
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |bucket_info| {
                Some(BucketInfo {
                    acquired: bucket_info.canonical,
                    ..bucket_info
                })
            })
            .or(Err(()))
            .unwrap();

        MontyHallHandle {
            acquired: canonical as usize,
            monte_hall: self,
        }
    }

    /// Store a value into the MontyHall
    /// SAFETY: you must be the only writer
    pub(self) unsafe fn store(&self, value: T) {
        let write_bucket = self.write_bucket();

        self.buckets[write_bucket].replace(MaybeUninit::new(value));

        let BucketInfo {
            acquired,
            canonical,
        } = self
            .bucket_info
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, move |bucket_info| {
                Some(BucketInfo {
                    canonical: write_bucket as u8,
                    ..bucket_info
                })
            })
            .or(Err(()))
            .unwrap();

        println!(
            "Replaced {} {} with {} {}",
            acquired, canonical, acquired, write_bucket
        );

        // If we did not have an active handle to the old canonical element, drop it
        if acquired != canonical {
            println!("Dropped old value {}", canonical);

            self.buckets[canonical as usize]
                .replace(MaybeUninit::uninit())
                .assume_init();
        }
    }

    /// Get the bucket we can always write to
    /// Unsafe because it must only be invoked from the writer
    unsafe fn write_bucket(&self) -> usize {
        let BucketInfo {
            acquired,
            canonical,
        } = self.bucket_info.load(Ordering::Relaxed);
        let mut mask = 0b111u32;

        if acquired < 3 {
            mask &= !(1 << acquired as usize);
        }

        mask &= !(1 << canonical as usize);

        // Obtain the index of the lowest free bucket
        mask.trailing_zeros() as usize
    }
}

impl<T> Drop for MontyHall<T> {
    fn drop(&mut self) {
        // It's impossible to end up here with a non-u8::MAX value for acquired, so we only need to drop canonical
        let BucketInfo { canonical, .. } = self.bucket_info.load(Ordering::Relaxed);

        unsafe {
            self.buckets[canonical as usize]
                .replace(MaybeUninit::uninit())
                .assume_init();
        }
    }
}

/// The "sending" half of a MontyHall
pub struct MontyHallWrite<T>(Arc<MontyHall<T>>);

impl<T> MontyHallWrite<T> {
    /// Store a value into the MontyHall.
    pub fn store(&mut self, value: T) {
        unsafe {
            self.0.store(value);
        }
    }
}

/// The "receiving" half of a MontyHall
pub struct MontyHallRead<T>(Arc<MontyHall<T>>);

impl<T> MontyHallRead<T> {
    /// Load the value in the MontyHall.
    pub fn load(&mut self) -> MontyHallHandle<'_, T> {
        unsafe { self.0.load() }
    }
}
