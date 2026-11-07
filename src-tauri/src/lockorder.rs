//! The app's shared locks, and the one order they are taken in.
//!
//! Three locks guard most of the app's state: the task queue, the database and the running turns.
//! A thread may hold more than one only in this order:
//!
//! ```text
//! queue  ->  runs  ->  database
//! ```
//!
//! So: take the queue first if you need it, then the runs map, then the database. Never take the
//! runs map while holding the database, and never take a lock you already hold (a std `Mutex` is
//! not reentrant: the second `lock()` on the same thread waits for itself forever). The usual fix
//! is to copy out what you need, let the guard go, then take the next lock.
//!
//! Better still, hold one at a time: copy out, let go, take the next. And hold none of them across
//! a process, a network call, a pipe or terminal write, or `app.emit` where it can be avoided.
//!
//! Debug builds (tests, the e2e run) check the order on every `lock()` and panic with both lock
//! names instead of hanging, so a wrong order fails a test rather than freezing the app. Release
//! builds skip the check.
//!
//! A panic while one of these is held doesn't poison it for everyone after: `lock()` hands the
//! value over anyway. Before, one panic made every later database call panic too, and the
//! window's requests never got an answer.

use std::ops::{Deref, DerefMut};
use std::sync::{LockResult, Mutex, MutexGuard};

/// A type that lives behind one of the ordered locks: its place in the order and its name.
pub trait Ranked {
    const RANK: u8;
    const NAME: &'static str;
}

impl Ranked for crate::queue::Queue {
    const RANK: u8 = 10;
    const NAME: &'static str = "queue";
}

impl Ranked for crate::store::Store {
    const RANK: u8 = 30;
    const NAME: &'static str = "database";
}

impl Ranked for crate::Runs {
    const RANK: u8 = 20;
    const NAME: &'static str = "runs";
}

/// A `Mutex` that knows its place in the lock order.
pub struct Ordered<T: Ranked> {
    inner: Mutex<T>,
}

impl<T: Ranked + Default> Default for Ordered<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

#[cfg(debug_assertions)]
thread_local! {
    /// The ordered locks this thread holds right now, in the order it took them.
    static HELD: std::cell::RefCell<Vec<(u8, &'static str)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Panics when taking a lock of `rank` now would break the order (or take a held lock again).
#[cfg(debug_assertions)]
fn check(rank: u8, name: &'static str) {
    HELD.with(|h| {
        if let Some(&(_, held)) = h.borrow().iter().find(|(r, _)| *r >= rank) {
            let why = if held == name {
                format!("lock order: this thread already holds the {name} lock and asked for it again, which waits forever")
            } else {
                format!("lock order: taking the {name} lock while holding the {held} lock (the order is queue, runs, database)")
            };
            panic!("{why}. Copy out what you need and let the first guard go before taking the next.");
        }
    });
}

impl<T: Ranked> Ordered<T> {
    pub fn new(value: T) -> Self {
        Self { inner: Mutex::new(value) }
    }

    /// Nobody holds it right now (tests: "the lock is let go again").
    pub fn is_free(&self) -> bool {
        self.inner.try_lock().is_ok()
    }

    /// `Mutex::lock`, with the order checked first in debug builds. Never fails: a lock a panicking
    /// thread held is handed over as it is (the `LockResult` keeps `.lock().unwrap()` call sites).
    pub fn lock(&self) -> LockResult<Guard<'_, T>> {
        #[cfg(debug_assertions)]
        check(T::RANK, T::NAME);
        match self.inner.lock() {
            Ok(g) => Ok(Guard::held(g)),
            Err(p) => Ok(Guard::held(p.into_inner())),
        }
    }
}

/// The held lock. Derefs to the value like a `MutexGuard`.
pub struct Guard<'a, T: Ranked> {
    g: MutexGuard<'a, T>,
}

impl<'a, T: Ranked> Guard<'a, T> {
    fn held(g: MutexGuard<'a, T>) -> Self {
        #[cfg(debug_assertions)]
        HELD.with(|h| h.borrow_mut().push((T::RANK, T::NAME)));
        Guard { g }
    }
}

impl<T: Ranked> Drop for Guard<'_, T> {
    fn drop(&mut self) {
        #[cfg(debug_assertions)]
        HELD.with(|h| {
            let mut h = h.borrow_mut();
            if let Some(i) = h.iter().rposition(|(r, _)| *r == T::RANK) {
                h.remove(i);
            }
        });
    }
}

impl<T: Ranked> Deref for Guard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.g
    }
}

impl<T: Ranked> DerefMut for Guard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.g
    }
}

#[cfg(all(test, debug_assertions))]
mod tests {
    use super::*;
    use crate::store::Store;

    #[test]
    fn the_order_is_allowed() {
        let q: Ordered<crate::queue::Queue> = Default::default();
        let db = Ordered::new(Store::in_memory().unwrap());
        let runs: Ordered<crate::Runs> = Default::default();
        let _a = q.lock().unwrap();
        let _b = runs.lock().unwrap();
        let _c = db.lock().unwrap();
    }

    #[test]
    fn a_lock_is_free_again_after_its_guard_goes() {
        let db = Ordered::new(Store::in_memory().unwrap());
        drop(db.lock().unwrap());
        let _again = db.lock().unwrap();
    }

    #[test]
    #[should_panic(expected = "already holds the database lock")]
    fn taking_the_database_twice_on_one_thread_panics_instead_of_hanging() {
        // The v0.22.2 freeze: `st.db().workspace(..).and_then(|w| st.db().repo(..))` in one statement.
        let db = Ordered::new(Store::in_memory().unwrap());
        let _first = db.lock().unwrap();
        let _second = db.lock().unwrap();
    }

    #[test]
    #[should_panic(expected = "taking the runs lock while holding the database lock")]
    fn the_runs_map_under_the_database_panics() {
        // v0.22.2's snapshot did this while autopilot did the reverse: two threads, each waiting
        // for the other's lock.
        let db = Ordered::new(Store::in_memory().unwrap());
        let runs: Ordered<crate::Runs> = Default::default();
        let _d = db.lock().unwrap();
        let _r = runs.lock().unwrap();
    }

    #[test]
    fn a_panic_while_holding_the_database_does_not_break_it_for_everyone_after() {
        let db = std::sync::Arc::new(Ordered::new(Store::in_memory().unwrap()));
        let d2 = db.clone();
        let _ = std::thread::spawn(move || {
            let _g = d2.lock().unwrap();
            panic!("a bug while the database is held");
        })
        .join();
        assert!(db.lock().unwrap().setting("x").is_ok(), "still usable after the panic");
    }
}
