extern crate trapdoor;
use trapdoor::Trapdoor;

struct DropObserver<'a>(&'a mut bool);

impl<'a> Drop for DropObserver<'a> {
    fn drop(&mut self) {
        *self.0 = true;
    }
}

#[test]
/// Ensures that the value contained in a Trapdoor is dropped when the two halves are
fn test_drop() {
    let (mut tx, rx) = Trapdoor::new().split();
    let mut did_drop = false;

    tx.store(DropObserver(&mut did_drop));
    std::mem::drop(tx);
    std::mem::drop(rx);

    assert!(did_drop);
}

#[test]
/// Ensures that the trapdoor properly moves values instead of dropping/copying them
fn test_move() {
    let (mut tx, mut rx) = Trapdoor::new().split();
    let mut did_drop = false;

    // We move the DropObserver into the trapdoor
    tx.store(DropObserver(&mut did_drop));
    // Take it from the trapdoor and forget it
    std::mem::forget(rx.take());

    // Drop the two ends
    std::mem::drop(tx);
    std::mem::drop(rx);

    // If the value was properly moved, this flag will remain unset as it was never dropped
    assert!(!did_drop);
}

#[test]
/// Ensures that a matched store/take sequence works
fn test_store_take() {
    // Some unique object, doesn't matter which
    let now = std::time::SystemTime::now();

    let (mut tx, mut rx) = Trapdoor::new().split();

    tx.store(now);
    match tx.try_store(now) {
        Ok(_) => panic!("Double store"),
        _ => {}
    }

    assert_eq!(rx.take(), now);
    match rx.try_take() {
        Some(_) => panic!("Double take"),
        _ => {}
    }
}

mod triple {
    use trapdoor::triple::*;

    use crate::DropObserver;

    #[test]
    fn test_store_load() {
        let (mut tx, mut rx) = MontyHall::new(1).split();

        let handle = rx.load();
        assert_eq!(*handle, 1);
        tx.store(2);
        assert_eq!(*handle, 1);

        std::mem::drop(handle);

        let handle = rx.load();
        assert_eq!(*handle, 2);
        tx.store(3);
        tx.store(4);
        assert_eq!(*handle, 2);

        std::mem::drop(handle);

        let handle = rx.load();
        assert_eq!(*handle, 4);
        tx.store(5);
        tx.store(6);
        assert_eq!(*handle, 4);

        std::mem::drop(handle);
        assert_eq!(*rx.load(), 6);
    }

    #[test]
    fn test_drop() {
        let mut did_drop = false;
        let (tx, rx) = MontyHall::new(DropObserver(&mut did_drop)).split();

        std::mem::drop(tx);
        std::mem::drop(rx);

        assert!(did_drop);
    }
}
