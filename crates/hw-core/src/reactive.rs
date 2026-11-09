//! Reactive values: [`Signal`], [`computed`] with automatic dependency
//! tracking, and [`poll`].
//!
//! Everything lives on the GTK main thread (`Rc<RefCell>`). Other threads
//! push values through a [`SignalSender`], which forwards them over a channel
//! drained by the GLib main loop.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::{Rc, Weak};
use std::time::Duration;

type Callback<T> = Rc<dyn Fn(&T)>;

struct Inner<T> {
    /// Behind an `Rc` so readers can drop the borrow before calling out,
    /// letting callbacks set this same signal.
    value: RefCell<Rc<T>>,
    /// Bumped on every write; lets `notify` stop delivering a stale value.
    version: Cell<u64>,
    subs: RefCell<Vec<(u64, Callback<T>)>>,
    next_id: Cell<u64>,
    /// Things whose lifetime is tied to this signal: dependency
    /// subscriptions of a computed, poll timers, …
    keep: RefCell<Vec<Box<dyn Any>>>,
}

/// A value that notifies subscribers when it changes.
///
/// Cloning is cheap and yields a handle to the same value.
pub struct Signal<T>(Rc<Inner<T>>);

impl<T> Clone for Signal<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T: fmt::Debug> fmt::Debug for Signal<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Signal")
            .field(&**self.0.value.borrow())
            .finish()
    }
}

/// Type-erased view of a signal, used for dependency tracking.
trait Source {
    fn id(&self) -> *const ();
    fn subscribe_any(self: Rc<Self>, f: Rc<dyn Fn()>) -> Subscription;
}

impl<T: 'static> Source for Inner<T> {
    fn id(&self) -> *const () {
        self as *const Self as *const ()
    }

    fn subscribe_any(self: Rc<Self>, f: Rc<dyn Fn()>) -> Subscription {
        Signal(self).subscribe(move |_| f())
    }
}

thread_local! {
    /// Stack of dependency sets being collected by running computeds.
    static TRACKING: RefCell<Vec<Vec<Rc<dyn Source>>>> = const { RefCell::new(Vec::new()) };
}

fn track(src: Rc<dyn Source>) {
    TRACKING.with(|t| {
        if let Some(deps) = t.borrow_mut().last_mut()
            && !deps.iter().any(|d| d.id() == src.id())
        {
            deps.push(src);
        }
    });
}

/// Runs `f` and returns the signals it read.
fn collect_deps<R>(f: impl FnOnce() -> R) -> (R, Vec<Rc<dyn Source>>) {
    TRACKING.with(|t| t.borrow_mut().push(Vec::new()));
    // Pop even if `f` panics so the stack stays balanced.
    struct Pop;
    impl Drop for Pop {
        fn drop(&mut self) {
            TRACKING.with(|t| t.borrow_mut().pop());
        }
    }
    let pop = Pop;
    let r = f();
    let deps = TRACKING.with(|t| t.borrow_mut().last_mut().map(std::mem::take));
    drop(pop);
    (r, deps.unwrap_or_default())
}

/// Runs `f` without registering the signals it reads as dependencies of the
/// surrounding computed.
pub fn untracked<R>(f: impl FnOnce() -> R) -> R {
    collect_deps(f).0
}

impl<T: 'static> Signal<T> {
    pub fn new(value: T) -> Self {
        Self(Rc::new(Inner {
            value: RefCell::new(Rc::new(value)),
            version: Cell::new(0),
            subs: RefCell::new(Vec::new()),
            next_id: Cell::new(0),
            keep: RefCell::new(Vec::new()),
        }))
    }

    /// Borrows the current value. Registers a dependency when called inside
    /// a computed.
    pub fn with<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        track(self.0.clone());
        let value = self.0.value.borrow().clone();
        f(&value)
    }

    /// Calls `f` with the new value on every change. The subscription ends
    /// when the returned guard is dropped.
    #[must_use = "dropping the Subscription unsubscribes immediately"]
    pub fn subscribe(&self, f: impl Fn(&T) + 'static) -> Subscription {
        let id = self.0.next_id.get();
        self.0.next_id.set(id + 1);
        self.0.subs.borrow_mut().push((id, Rc::new(f)));
        let weak: Weak<Inner<T>> = Rc::downgrade(&self.0);
        Subscription(Some(Box::new(move || {
            if let Some(inner) = weak.upgrade() {
                inner.subs.borrow_mut().retain(|(i, _)| *i != id);
            }
        })))
    }

    /// Replaces the value and notifies subscribers unconditionally.
    pub fn replace(&self, value: T) {
        *self.0.value.borrow_mut() = Rc::new(value);
        self.bump();
        self.notify();
    }

    fn bump(&self) {
        self.0.version.set(self.0.version.get() + 1);
    }

    /// Notifies subscribers with the current value.
    pub fn notify(&self) {
        // Snapshot so callbacks may (un)subscribe or set other signals.
        let subs: Vec<Callback<T>> = self
            .0
            .subs
            .borrow()
            .iter()
            .map(|(_, f)| f.clone())
            .collect();
        if subs.is_empty() {
            return;
        }
        let value = self.0.value.borrow().clone();
        let version = self.0.version.get();
        for f in subs {
            // A callback set a newer value, and its own notify delivered it.
            if self.0.version.get() != version {
                break;
            }
            f(&value);
        }
    }

    /// Ties `thing` to the lifetime of this signal.
    pub fn keep_alive(&self, thing: impl Any) {
        self.0.keep.borrow_mut().push(Box::new(thing));
    }

    pub fn subscriber_count(&self) -> usize {
        self.0.subs.borrow().len()
    }

    pub fn ptr_eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }

    pub fn downgrade(&self) -> WeakSignal<T> {
        WeakSignal(Rc::downgrade(&self.0))
    }
}

impl<T: Clone + 'static> Signal<T> {
    /// Returns a copy of the current value (tracked).
    pub fn get(&self) -> T {
        self.with(T::clone)
    }

    /// Returns a copy of the current value without tracking.
    pub fn peek(&self) -> T {
        T::clone(&self.0.value.borrow())
    }

    /// Mutates the value in place and notifies subscribers. Clones the value
    /// first only if a callback is still holding the old one.
    pub fn update(&self, f: impl FnOnce(&mut T)) {
        f(Rc::make_mut(&mut self.0.value.borrow_mut()));
        self.bump();
        self.notify();
    }

    /// Derives a new signal; shorthand for `computed(|| f(&self.get()))`.
    pub fn map<U: Clone + PartialEq + 'static>(&self, f: impl Fn(&T) -> U + 'static) -> Signal<U> {
        let this = self.clone();
        computed(move || this.with(&f))
    }
}

impl<T: PartialEq + 'static> Signal<T> {
    /// Sets the value, notifying subscribers only if it changed.
    pub fn set(&self, value: T) {
        if **self.0.value.borrow() == value {
            return;
        }
        self.replace(value);
    }
}

impl<T: PartialEq + Send + 'static> Signal<T> {
    /// Returns a `Send` handle that sets this signal from any thread. Values
    /// are delivered on the thread-default GLib main context of the caller
    /// (the GTK main thread). Must be called on the main thread.
    pub fn sender(&self) -> SignalSender<T> {
        let (tx, rx) = async_channel::unbounded::<T>();
        let weak = self.downgrade();
        glib::spawn_future_local(async move {
            while let Ok(v) = rx.recv().await {
                match weak.upgrade() {
                    Some(s) => s.set(v),
                    None => break,
                }
            }
        });
        SignalSender(tx)
    }
}

/// Weak handle to a [`Signal`].
pub struct WeakSignal<T>(Weak<Inner<T>>);

impl<T> Clone for WeakSignal<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T> WeakSignal<T> {
    pub fn upgrade(&self) -> Option<Signal<T>> {
        self.0.upgrade().map(Signal)
    }
}

/// Thread-safe sender created by [`Signal::sender`].
pub struct SignalSender<T>(async_channel::Sender<T>);

impl<T> Clone for SignalSender<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T> SignalSender<T> {
    /// Queues `value`; returns `false` once the signal is gone.
    pub fn send(&self, value: T) -> bool {
        self.0.send_blocking(value).is_ok()
    }
}

/// Guard returned by [`Signal::subscribe`]; unsubscribes on drop.
#[must_use = "dropping the Subscription unsubscribes immediately"]
pub struct Subscription(Option<Box<dyn FnOnce()>>);

impl Subscription {
    /// A subscription that does nothing.
    pub fn none() -> Self {
        Self(None)
    }

    /// Keeps the subscription alive for as long as the signal exists.
    pub fn forget(mut self) {
        self.0 = None;
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        if let Some(f) = self.0.take() {
            f();
        }
    }
}

/// Creates a signal whose value is `f()`, recomputed whenever a signal read
/// by `f` changes. Dependencies are re-collected on every run.
pub fn computed<T: Clone + PartialEq + 'static>(f: impl Fn() -> T + 'static) -> Signal<T> {
    let (value, deps) = collect_deps(&f);
    let signal = Signal::new(value);
    let state = Rc::new(ComputedState {
        f: Box::new(f),
        target: signal.downgrade(),
        deps: RefCell::new(Vec::new()),
    });
    state.subscribe_deps(deps);
    signal.keep_alive(state);
    signal
}

struct ComputedState<T> {
    f: Box<dyn Fn() -> T>,
    target: WeakSignal<T>,
    deps: RefCell<Vec<Subscription>>,
}

impl<T: Clone + PartialEq + 'static> ComputedState<T> {
    fn subscribe_deps(self: &Rc<Self>, deps: Vec<Rc<dyn Source>>) {
        let weak = Rc::downgrade(self);
        let subs = deps
            .into_iter()
            .map(|d| {
                let weak = weak.clone();
                d.subscribe_any(Rc::new(move || {
                    if let Some(state) = weak.upgrade() {
                        state.rerun();
                    }
                }))
            })
            .collect();
        // Old subscriptions drop here.
        *self.deps.borrow_mut() = subs;
    }

    fn rerun(self: &Rc<Self>) {
        let Some(target) = self.target.upgrade() else {
            return;
        };
        let (value, deps) = collect_deps(&self.f);
        self.subscribe_deps(deps);
        target.set(value);
    }
}

/// Creates a signal set to `f()` now and then every `interval`. The timer
/// stops when the signal is dropped. Requires a running GLib main loop.
pub fn poll<T: PartialEq + 'static>(interval: Duration, f: impl Fn() -> T + 'static) -> Signal<T> {
    let signal = Signal::new(untracked(&f));
    let weak = signal.downgrade();
    let id = glib::timeout_add_local(interval, move || match weak.upgrade() {
        Some(s) => {
            s.set(untracked(&f));
            glib::ControlFlow::Continue
        }
        None => glib::ControlFlow::Break,
    });
    signal.keep_alive(SourceGuard(Some(id)));
    signal
}

/// Removes a GLib source when dropped.
pub struct SourceGuard(pub Option<glib::SourceId>);

impl Drop for SourceGuard {
    fn drop(&mut self) {
        if let Some(id) = self.0.take() {
            remove_source(id);
        }
    }
}

/// Removes a source from the thread-default main context, ignoring sources
/// that already finished. (`SourceId::remove` only looks at the global
/// default context and panics on unknown ids.)
pub fn remove_source(id: glib::SourceId) {
    if let Some(src) = glib::MainContext::ref_thread_default().find_source_by_id(&id) {
        src.destroy();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_notifies_only_on_change() {
        let s = Signal::new(1);
        let hits = Rc::new(Cell::new(0));
        let h = hits.clone();
        let _sub = s.subscribe(move |_| h.set(h.get() + 1));
        s.set(1);
        s.set(2);
        s.set(2);
        assert_eq!(hits.get(), 1);
        assert_eq!(s.get(), 2);
    }

    #[test]
    fn dropping_subscription_unsubscribes() {
        let s = Signal::new(0);
        let sub = s.subscribe(|_| {});
        assert_eq!(s.subscriber_count(), 1);
        drop(sub);
        assert_eq!(s.subscriber_count(), 0);
    }

    #[test]
    fn computed_tracks_dependencies() {
        let a = Signal::new(1);
        let b = Signal::new(10);
        let (a2, b2) = (a.clone(), b.clone());
        let sum = computed(move || a2.get() + b2.get());
        assert_eq!(sum.get(), 11);
        a.set(2);
        assert_eq!(sum.get(), 12);
        b.set(20);
        assert_eq!(sum.get(), 22);
    }

    #[test]
    fn computed_recollects_dynamic_deps() {
        let cond = Signal::new(true);
        let a = Signal::new("a");
        let b = Signal::new("b");
        let (c2, a2, b2) = (cond.clone(), a.clone(), b.clone());
        let pick = computed(move || if c2.get() { a2.get() } else { b2.get() });
        assert_eq!(pick.get(), "a");
        assert_eq!(b.subscriber_count(), 0);
        cond.set(false);
        assert_eq!(pick.get(), "b");
        assert_eq!(a.subscriber_count(), 0);
        assert_eq!(b.subscriber_count(), 1);
        b.set("B");
        assert_eq!(pick.get(), "B");
    }

    #[test]
    fn chained_map() {
        let a = Signal::new(2);
        let doubled = a.map(|v| v * 2);
        let text = doubled.map(|v| format!("{v}"));
        a.set(5);
        assert_eq!(text.get(), "10");
    }

    #[test]
    fn dropped_computed_releases_deps() {
        let a = Signal::new(1);
        let a2 = a.clone();
        let c = computed(move || a2.get());
        assert_eq!(a.subscriber_count(), 1);
        drop(c);
        assert_eq!(a.subscriber_count(), 0);
    }

    #[test]
    fn untracked_reads_are_not_deps() {
        let a = Signal::new(1);
        let a2 = a.clone();
        let _c = computed(move || untracked(|| a2.get()));
        assert_eq!(a.subscriber_count(), 0);
    }

    #[test]
    fn subscriber_may_set_other_signal() {
        let a = Signal::new(0);
        let b = Signal::new(0);
        let b2 = b.clone();
        let _s = a.subscribe(move |v| b2.set(*v * 3));
        a.set(4);
        assert_eq!(b.get(), 12);
    }

    #[test]
    fn subscriber_may_set_own_signal() {
        let s = Signal::new(0);
        let s2 = s.clone();
        let _sub = s.subscribe(move |v| {
            if *v > 10 {
                s2.set(10)
            }
        });
        s.set(20);
        assert_eq!(s.peek(), 10);
    }

    #[test]
    fn with_may_set_own_signal() {
        let s = Signal::new(1);
        s.with(|v| s.set(*v + 1));
        assert_eq!(s.peek(), 2);
        s.update(|v| *v += 1);
        assert_eq!(s.peek(), 3);
    }

    #[test]
    fn later_subscribers_see_only_final_value() {
        let s = Signal::new(0);
        let s2 = s.clone();
        let _clamp = s.subscribe(move |v| {
            if *v > 10 {
                s2.set(10)
            }
        });
        let seen = Rc::new(RefCell::new(Vec::new()));
        let seen2 = seen.clone();
        let _log = s.subscribe(move |v| seen2.borrow_mut().push(*v));
        s.set(20);
        assert_eq!(*seen.borrow(), [10]);
    }

    #[test]
    fn sender_delivers_from_thread() {
        let ctx = glib::MainContext::new();
        ctx.with_thread_default(|| {
            let s = Signal::new(0);
            let tx = s.sender();
            std::thread::spawn(move || {
                tx.send(42);
            })
            .join()
            .unwrap();
            let deadline = std::time::Instant::now() + Duration::from_secs(2);
            while s.peek() != 42 && std::time::Instant::now() < deadline {
                ctx.iteration(false);
            }
            assert_eq!(s.peek(), 42);
        })
        .unwrap();
    }
}
