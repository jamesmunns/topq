#![cfg_attr(not(test), no_std)]

use core::slice;
use heapless::{ArrayLength, Vec};

/// A trait that represents a (probably rolling) timer of arbitrary
/// precision.
pub trait Timer {
    /// The type that is used to represent a time/offset
    type Time: Ord;

    /// The number of ticks per second. e.g. if using a 32.786kHz
    /// timer, this would be `32_768`
    const TICKS_PER_SECOND: u32;

    /// Get the current time
    fn now(&self) -> Self::Time;

    /// Add a timestamp plus an offset. When this type is expected to
    /// be rolling (e.g. when using a `u32` with a 32.768kHz clock), the
    /// addition should be done by wrapping
    fn wrapping_add(time: &Self::Time, offset: &Self::Time) -> Self::Time;
}

/// A "Timeout Priority Queue"
///
/// This is generic over four parameters:
///
/// * D: The data type held by the priority queue
/// * P: The priority type. Must implement `Ord`
/// * T: The timer type. Must implement `topq::Timer`
/// * N: The number of priority levels that can be kept at once
///
/// ## Note
///
/// `Topq` CAN handle timers that rollover periodically, however `Topq::purge()` MUST be called
/// AT LEAST twice per rollover time period.
///
/// For example, when using a 32.768kHz clock source and a `u32` time value, the timer will
/// roll over every 1.51 days or so. In this case, you MUST call `purge` to remove stale values
/// at least every 0.75 days or so.
pub struct Topq<D, P, T, N>
where
    D: 'static,
    P: Ord,
    T: Timer,
    N: ArrayLength<TopqItem<D, P, T>>,
{
    queue: Vec<TopqItem<D, P, T>, N>,
    timer: T,
}

impl<D, P, T, N> Topq<D, P, T, N>
where
    D: 'static,
    P: Ord,
    T: Timer,
    N: ArrayLength<TopqItem<D, P, T>>,
{
    /// Create an empty Topq with the given timer
    pub fn new(timer: T) -> Self {
        Self {
            queue: Vec::new(),
            timer,
        }
    }

    /// Get the current time based on the internal counter
    pub fn now(&self) -> T::Time {
        self.timer.now()
    }

    /// Insert a datapoint into the priority queue
    ///
    /// If the queue already contains an item with the same priority, the old
    /// data and timeout will be replaced. If the queue does not contain an item
    /// at this priority, it will be inserted if there is room or it is a higher
    /// priority than the existing items
    pub fn insert(&mut self, item: D, prio: P, valid_for: T::Time) {
        let now = self.timer.now();
        let exp = T::wrapping_add(&now, &valid_for);

        let new_item = TopqItem {
            item,
            prio,
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

    /// Remove any expired items from the priority queue
    ///
    /// See the module level documentation for when it is necessary to call this function
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

    /// Obtain the highest priority and currently valid data, if any
    ///
    /// This is typically used when you ONLY need the current value, and not
    /// the remaining validity time or the priority of the currently valid data
    pub fn get_data(&self) -> Option<&D> {
        self.get_item()
            .map(|i| &i.item)
    }

    /// Obtain the highest priority and currently valid topq item, if any
    ///
    /// This is typically used when you need the current value, AND ALSO need
    /// the remaining validity time or the priority of the currently valid data
    pub fn get_item(&self) -> Option<&TopqItem<D, P, T>> {
        let now = self.timer.now();
        self.queue
            .iter()
            .find(|item| item.valid_at_time(&now))
    }
}


#[derive(Debug)]
pub struct TopqItem<D, P, T>
where
    D: 'static,
    P: Ord,
    T: Timer,
{
    pub item: D,
    pub prio: P,
    pub start_time: T::Time,
    pub expiry_time: T::Time,
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


impl<'a, D, P, T, N> IntoIterator for &'a Topq<D, P, T, N>
where
    D: 'static,
    P: Ord,
    T: Timer,
    N: ArrayLength<TopqItem<D, P, T>>,
{
    type Item = &'a TopqItem<D, P, T>;
    type IntoIter = slice::Iter<'a, TopqItem<D, P, T>>;

    fn into_iter(self) -> Self::IntoIter {
        self.queue.iter()
    }
}

impl<'a, D, P, T, N> IntoIterator for &'a mut Topq<D, P, T, N>
where
    D: 'static,
    P: Ord,
    T: Timer,
    N: ArrayLength<TopqItem<D, P, T>>,
{
    type Item = &'a mut TopqItem<D, P, T>;
    type IntoIter = slice::IterMut<'a, TopqItem<D, P, T>>;

    fn into_iter(self) -> Self::IntoIter {
        self.queue.iter_mut()
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
        assert_eq!(q.get_data(), Some(&10));

        q.insert(11, Medium, 25);
        assert_eq!(q.get_data(), Some(&11));

        q.insert(12, High, 20);
        assert_eq!(q.get_data(), Some(&12));

        TIMER.store(20, SeqCst);
        assert_eq!(q.get_data(), Some(&12));

        TIMER.store(21, SeqCst);
        assert_eq!(q.get_data(), Some(&11));

        TIMER.store(26, SeqCst);
        assert_eq!(q.get_data(), Some(&10));

        TIMER.store(31, SeqCst);
        assert_eq!(q.get_data(), None);
    }

    #[test]
    fn expiry() {
        static TIMER: AtomicU32 = AtomicU32::new(0);
        let timer = FakeTimer(&TIMER);
        let mut q: Topq<u32, u8, FakeTimer, U4> = Topq::new(timer);

        q.insert(10, 3, 30);
        assert_eq!(q.get_data(), Some(&10));

        q.insert(11, 4, 25);
        assert_eq!(q.get_data(), Some(&11));

        q.insert(12, 5, 20);
        assert_eq!(q.get_data(), Some(&12));

        q.insert(13, 6, 15);
        assert_eq!(q.get_data(), Some(&13));

        TIMER.store(15, SeqCst);
        assert_eq!(q.get_data(), Some(&13));

        TIMER.store(16, SeqCst);
        assert_eq!(q.get_data(), Some(&12));

        TIMER.store(21, SeqCst);
        assert_eq!(q.get_data(), Some(&11));

        TIMER.store(26, SeqCst);
        assert_eq!(q.get_data(), Some(&10));

        TIMER.store(31, SeqCst);
        assert_eq!(q.get_data(), None);
    }

    #[test]
    fn out_of_order() {
        static TIMER: AtomicU32 = AtomicU32::new(0);
        let timer = FakeTimer(&TIMER);
        let mut q: Topq<u32, u8, FakeTimer, U4> = Topq::new(timer);

        q.insert(10, 3, 30);
        assert_eq!(q.get_data(), Some(&10));

        q.insert(13, 6, 15);
        assert_eq!(q.get_data(), Some(&13));

        q.insert(12, 5, 20);
        assert_eq!(q.get_data(), Some(&13));

        q.insert(11, 4, 25);
        assert_eq!(q.get_data(), Some(&13));

        q.into_iter().for_each(|t| {
            println!("{:?}", t);
        });

        TIMER.store(15, SeqCst);
        assert_eq!(q.get_data(), Some(&13));

        TIMER.store(16, SeqCst);
        assert_eq!(q.get_data(), Some(&12));

        TIMER.store(21, SeqCst);
        assert_eq!(q.get_data(), Some(&11));

        TIMER.store(26, SeqCst);
        assert_eq!(q.get_data(), Some(&10));

        TIMER.store(31, SeqCst);
        assert_eq!(q.get_data(), None);
    }

    #[test]
    fn rollover() {
        static TIMER: AtomicU32 = AtomicU32::new(0);
        let timer = FakeTimer(&TIMER);
        let mut q: Topq<u32, u8, FakeTimer, U4> = Topq::new(timer);

        TIMER.store(0xFFFF_FFF0, SeqCst);
        q.insert(10, 3, 32);
        assert_eq!(q.get_data(), Some(&10));

        TIMER.store(0xFFFF_FFF8, SeqCst);
        assert_eq!(q.get_data(), Some(&10));

        TIMER.store(0xFFFF_FFFF, SeqCst);
        assert_eq!(q.get_data(), Some(&10));

        TIMER.store(0x0000_0000, SeqCst);
        assert_eq!(q.get_data(), Some(&10));

        TIMER.store(0x0000_0010, SeqCst);
        assert_eq!(q.get_data(), Some(&10));

        TIMER.store(0x0000_0011, SeqCst);
        assert_eq!(q.get_data(), None);
    }
}
