#![cfg_attr(not(test), no_std)]

use core::marker::PhantomData;
use heapless::{ArrayLength, Vec};

pub trait Timer {
    type Time: Ord;
    const TICKS_PER_SECOND: u32;

    fn now(&self) -> Self::Time;
    fn wrapping_add(left: &Self::Time, right: &Self::Time) -> Self::Time;
}

pub struct Topq<D, P, T, N>
where
    D: 'static,
    P: Ord,
    T: Timer,
    N: ArrayLength<TopqItem<D, P, T>>,
{
    pub queue: Vec<TopqItem<D, P, T>, N>,
    timer: T,
}

impl<D, P, T, N> Topq<D, P, T, N>
where
    D: 'static,
    P: Ord,
    T: Timer,
    N: ArrayLength<TopqItem<D, P, T>>,
{
    pub fn new(timer: T) -> Self {
        Self {
            queue: Vec::new(),
            timer,
        }
    }

    pub fn insert(&mut self, item: D, prio: P, valid_for: T::Time) {
        let now = self.timer.now();
        let exp = T::wrapping_add(&now, &valid_for);

        let new_item = TopqItem {
            item,
            prio,
            timer: PhantomData,
            start_time: now,
            expiry_time: exp,
        };

        self.insert_item(new_item);
    }

    fn insert_item(&mut self, new_item: TopqItem<D, P, T>) {
        // Where should we add this?
        // TODO: We can probably do an insertion sort for cheaper than a binary
        // search + unstable sort, especially for small arrays
        match self
            .queue
            .binary_search_by(|ti| new_item.prio.cmp(&ti.prio))
        {
            Ok(idx) => {
                // We have found an exact priority match. Replace.
                self.queue[idx] = new_item;
            }
            Err(idx) => {
                // We have found an insertion position

                // Is the queue already full?
                if self.queue.len() == self.queue.capacity() {
                    self.queue.pop();
                }

                // Add item to the end of the queue
                self.queue.push(new_item).ok();

                // If the insertion position was NOT at the end, sort the queue
                if idx != self.queue.len() {
                    self.queue
                        .sort_unstable_by(|ti_a, ti_b| ti_b.prio.cmp(&ti_a.prio));
                }
            }
        }
    }

    pub fn prune(&mut self) {
        // TODO: Do this without making a second queue (remove items and shift up as needed),
        // or having to re-sort the queue because we popped everything backwards
        // TODO: Probably sort with all invalid going to the back, and then truncate
        // the list

        let now = self.timer.now();

        let mut new = Vec::new();

        while let Some(item) = self.queue.pop() {
            if item.valid_at_time(&now) {
                new.push(item).ok();
            }
        }

        new.sort_unstable_by(|ti_a, ti_b| ti_b.prio.cmp(&ti_a.prio));

        self.queue = new;
    }

    pub fn get(&self) -> Option<&D> {
        let now = self.timer.now();
        self.queue
            .iter()
            .find(|item| item.valid_at_time(&now))
            .map(|i| &i.item)
    }
}

#[derive(Debug)]
pub struct TopqItem<D, P, T>
where
    D: 'static,
    P: Ord,
    T: Timer,
{
    item: D,
    prio: P,
    timer: PhantomData<T>,
    start_time: T::Time,
    expiry_time: T::Time,
}

impl<D, P, T> TopqItem<D, P, T>
where
    D: 'static,
    P: Ord,
    T: Timer,
{
    fn valid_at_time(&self, time: &T::Time) -> bool {
        if self.start_time < self.expiry_time {
            // Not a rollover case
            self.start_time <= *time && *time <= self.expiry_time
        } else {
            // Rollover case
            *time >= self.start_time || *time <= self.expiry_time
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use core::sync::atomic::{AtomicU32, Ordering::SeqCst};
    use heapless::consts::*;

    #[derive(Debug)]
    struct FakeTimer(&'static AtomicU32);

    impl Timer for FakeTimer {
        type Time = u32;
        const TICKS_PER_SECOND: u32 = 1;

        fn now(&self) -> u32 {
            self.0.load(SeqCst)
        }

        fn wrapping_add(a: &u32, b: &u32) -> u32 {
            a.wrapping_add(*b)
        }
    }

    #[derive(Debug, PartialOrd, Ord, Eq, PartialEq)]
    enum PrioEnum {
        Low,
        Medium,
        High,
    }

    #[test]
    fn enum_prio() {
        static TIMER: AtomicU32 = AtomicU32::new(0);
        let timer = FakeTimer(&TIMER);
        let mut q: Topq<u32, PrioEnum, FakeTimer, U4> = Topq::new(timer);
        use PrioEnum::*;

        q.insert(10, Low, 30);
        assert_eq!(q.get(), Some(&10));

        q.insert(11, Medium, 25);
        assert_eq!(q.get(), Some(&11));

        q.insert(12, High, 20);
        assert_eq!(q.get(), Some(&12));

        TIMER.store(20, SeqCst);
        assert_eq!(q.get(), Some(&12));

        TIMER.store(21, SeqCst);
        assert_eq!(q.get(), Some(&11));

        TIMER.store(26, SeqCst);
        assert_eq!(q.get(), Some(&10));

        TIMER.store(31, SeqCst);
        assert_eq!(q.get(), None);
    }

    #[test]
    fn expiry() {
        static TIMER: AtomicU32 = AtomicU32::new(0);
        let timer = FakeTimer(&TIMER);
        let mut q: Topq<u32, u8, FakeTimer, U4> = Topq::new(timer);

        q.insert(10, 3, 30);
        assert_eq!(q.get(), Some(&10));

        q.insert(11, 4, 25);
        assert_eq!(q.get(), Some(&11));

        q.insert(12, 5, 20);
        assert_eq!(q.get(), Some(&12));

        q.insert(13, 6, 15);
        assert_eq!(q.get(), Some(&13));

        TIMER.store(15, SeqCst);
        assert_eq!(q.get(), Some(&13));

        TIMER.store(16, SeqCst);
        assert_eq!(q.get(), Some(&12));

        TIMER.store(21, SeqCst);
        assert_eq!(q.get(), Some(&11));

        TIMER.store(26, SeqCst);
        assert_eq!(q.get(), Some(&10));

        TIMER.store(31, SeqCst);
        assert_eq!(q.get(), None);
    }

    #[test]
    fn out_of_order() {
        static TIMER: AtomicU32 = AtomicU32::new(0);
        let timer = FakeTimer(&TIMER);
        let mut q: Topq<u32, u8, FakeTimer, U4> = Topq::new(timer);

        q.insert(10, 3, 30);
        assert_eq!(q.get(), Some(&10));

        q.insert(13, 6, 15);
        assert_eq!(q.get(), Some(&13));

        q.insert(12, 5, 20);
        assert_eq!(q.get(), Some(&13));

        q.insert(11, 4, 25);
        assert_eq!(q.get(), Some(&13));

        TIMER.store(15, SeqCst);
        assert_eq!(q.get(), Some(&13));

        TIMER.store(16, SeqCst);
        assert_eq!(q.get(), Some(&12));

        TIMER.store(21, SeqCst);
        assert_eq!(q.get(), Some(&11));

        TIMER.store(26, SeqCst);
        assert_eq!(q.get(), Some(&10));

        TIMER.store(31, SeqCst);
        assert_eq!(q.get(), None);
    }

    #[test]
    fn rollover() {
        static TIMER: AtomicU32 = AtomicU32::new(0);
        let timer = FakeTimer(&TIMER);
        let mut q: Topq<u32, u8, FakeTimer, U4> = Topq::new(timer);

        TIMER.store(0xFFFF_FFF0, SeqCst);
        q.insert(10, 3, 32);
        assert_eq!(q.get(), Some(&10));

        TIMER.store(0xFFFF_FFF8, SeqCst);
        assert_eq!(q.get(), Some(&10));

        TIMER.store(0xFFFF_FFFF, SeqCst);
        assert_eq!(q.get(), Some(&10));

        TIMER.store(0x0000_0000, SeqCst);
        assert_eq!(q.get(), Some(&10));

        TIMER.store(0x0000_0010, SeqCst);
        assert_eq!(q.get(), Some(&10));

        TIMER.store(0x0000_0011, SeqCst);
        assert_eq!(q.get(), None);
    }
}
