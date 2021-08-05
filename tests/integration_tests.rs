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

