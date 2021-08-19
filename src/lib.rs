//! A simple, lock-free, single-producer single-consumer channel that can hold a single item.
//! Unlike crates like [triple_buffer][https://crates.io/crates/triple_buffer], the content is moved between ends of the channel instead of borrowed.

use std::{
    cell::Cell,
    mem::MaybeUninit,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

pub mod triple;

/// An unsplit trapdoor.
pub struct Trapdoor<T> {
    populated: AtomicBool,
    data: Cell<MaybeUninit<T>>,
}

impl<T> Trapdoor<T> {
    /// Construct an empty trapdoor.
    ///
    /// # Examples
    ///
    /// ```
    /// # use trapdoor::Trapdoor;
    /// let trapdoor: Trapdoor<()> = Trapdoor::new();
    /// ```
    pub fn new() -> Self {
        Trapdoor {
            populated: AtomicBool::new(false),
            data: Cell::new(MaybeUninit::uninit()),
        }
    }

    /// Construct a trapdoor pre-populated with a value.
    ///
    /// # Examples
    ///
    /// ```
    /// # use trapdoor::Trapdoor;
    /// let trapdoor = Trapdoor::with_value(());
    /// ```
    pub fn with_value(value: T) -> Self {
        Trapdoor {
            populated: AtomicBool::new(true),
            data: Cell::new(MaybeUninit::new(value)),
        }
    }

    /// Construct a trapdoor pre-populated with the default value.
    ///
    /// # Examples
    ///
    /// ```
    /// # use trapdoor::Trapdoor;
    /// let trapdoor: Trapdoor<u32> = Trapdoor::with_default();
    /// ```
    pub fn with_default() -> Self where T: Default {
        Self::with_value(Default::default())
    }

    /// Split the trapdoor into the sender and receiver halves.
    ///
    /// # Examples
    ///
    /// If `T` implements Send, the two trapdoor halves will as well.
    /// ```
    /// # use std::thread;
    /// # use trapdoor::Trapdoor;
    /// let (tx, mut rx) = Trapdoor::with_value(()).split();
    /// thread::spawn(move || {
    ///     rx.take();
    /// });
    /// ```
    /// If `T` does not implement Send, the halves will only be usable within the thread they were created from.
    /// ```compile_fail
    /// # use std::thread;
    /// # use trapdoor::Trapdoor;
    /// let (tx, mut rx) = Trapdoor::with_value(Rc::new(())).split();
    /// thread::spawn(move || {
    ///     rx.take();
    /// });
    /// ```
    pub fn split(self) -> (TrapdoorWrite<T>, TrapdoorRead<T>) {
        let arc = Arc::new(self);

        (TrapdoorWrite(arc.clone()), TrapdoorRead(arc))
    }

    /// Attempt to store a value into the trapdoor
    /// Fails and returns the value if the trapdoor is occupied
    /// SAFETY: you must be the only writer
    pub(self) unsafe fn try_store(&self, value: T) -> Result<(), T> {
        if self.populated.load(Ordering::Acquire) {
            Err(value)
        } else {
            self.data.set(MaybeUninit::new(value));
            self.populated.store(true, Ordering::Release);

            Ok(())
        }
    }

    /// Attempt to take a value from the trapdoor
    /// Returns None if the trapdoor is unoccupied
    /// SAFETY: you must be the only reader
    pub(self) unsafe fn try_take(&self) -> Option<T> {
        if self.populated.load(Ordering::Acquire) {
            // Safe - `data` validity follows `populated`
            let value = self.data.replace(MaybeUninit::uninit()).assume_init();

            self.populated.store(false, Ordering::Release);

            Some(value)
        } else {
            None
        }
    }

    pub(self) unsafe fn try_peek(&self) -> Option<&T> {
        if self.populated.load(Ordering::Acquire) {
            Some(&*(*self.data.as_ptr()).as_ptr())
        } else {
            None
        }
    }
}

/// Cell isn't Sync because of interior mutability,
/// but we guarantee no concurrent mutation.
unsafe impl<T> Sync for Trapdoor<T> where T: Send {}

impl<T> Drop for Trapdoor<T> {
    fn drop(&mut self) {
        // Safe - at this point, we are the only reader
        unsafe {
            // Take (and then drop) the value if available
            self.try_take();
        }
    }
}

/// The "sending" half of a trapdoor.
pub struct TrapdoorWrite<T>(Arc<Trapdoor<T>>);

impl<T> TrapdoorWrite<T> {
    /// Stores a value into the trapdoor.
    /// # Panics
    /// If the trapdoor already contains a value, this function will panic.
    /// If this is not desired, use the [try_store][Self::try_store] function instead.
    /// # Examples
    /// ```should_panic
    /// # use trapdoor::Trapdoor;
    /// let (mut tx, rx) = Trapdoor::new().split();
    /// tx.store(123);
    /// // The following call will panic, as the trapdoor is already occupied
    /// tx.store(123);
    /// ```
    pub fn store(&mut self, value: T) {
        if let Err(_) = self.try_store(value) {
            panic!("Trapdoor is already occupied");
        }
    }

    /// Attempts to store a value into the trapdoor.
    ///
    /// If the trapdoor is occupied, `value` will be returned as the error.
    /// # Examples
    /// ```
    /// # use trapdoor::Trapdoor;
    /// let (mut tx, rx) = Trapdoor::new().split();
    /// println!("{:?}", tx.try_store(123)); // Ok(())
    /// println!("{:?}", tx.try_store(456)); // Err(456)
    /// ```
    pub fn try_store(&mut self, value: T) -> Result<(), T> {
        // Only one TrapdoorWrite may exist for any TrapdoorInner, so we're the only writer
        unsafe { self.0.try_store(value) }
    }
}

pub struct TrapdoorRead<T>(Arc<Trapdoor<T>>);

impl<T> TrapdoorRead<T> {
    /// Take the value from the trapdoor.
    /// # Panics
    /// If the trapdoor does not contain a value, this function will panic.
    /// If this is not desired, use the [try_take][Self::try_take] function instead.
    /// # Examples
    /// ```
    /// # use trapdoor::Trapdoor;
    /// let (mut tx, mut rx) = Trapdoor::new().split();
    /// tx.store(123);
    /// assert_eq!(123, rx.take());
    /// ```
    /// ```should_panic
    /// # use trapdoor::Trapdoor;
    /// let (mut tx, mut rx) = Trapdoor::new().split();
    /// // The following call will panic, as the trapdoor is empty
    /// let _: u32 = rx.take();
    /// ```
    pub fn take(&mut self) -> T {
        self.try_take().expect("Trapdoor is not occupied")
    }

    /// Attempts to take the value from the trapdoor. Returns `None` if the trapdoor is empty.
    pub fn try_take(&mut self) -> Option<T> {
        // Only one TrapdoorRead may exist for any TrapdoorInner, so we're the only reader
        unsafe { self.0.try_take() }
    }

    pub fn peek(&self) -> &T {
        self.try_peek().expect("Trapdoor is not occupied")
    }

    pub fn try_peek(&self) -> Option<&T> {
        unsafe { self.0.try_peek() }
    }
}
