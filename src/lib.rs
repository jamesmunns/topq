#![cfg_attr(not(test), no_std)]

use core::mem::MaybeUninit;
use core::slice;
use generic_array::{ArrayLength, GenericArray};
pub use generic_array::typenum::consts;

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
    queue: MaybeUninit<GenericArray<TopqItem<D, P, T>, N>>,
    timer: T,
    used: usize,
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
            queue: MaybeUninit::uninit(),
            timer,
            used: 0,
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
        let start_ptr = self.queue.as_mut_ptr().cast::<TopqItem<D, P, T>>();

        // Find the insertion place
        let result_idx = {
            let mut_slice = unsafe { core::slice::from_raw_parts_mut(start_ptr, self.used) };
            mut_slice.binary_search_by(|ti| new_item.prio.cmp(&ti.prio))
        };

        match result_idx {
            Ok(idx) => {
                unsafe {
                    // We have an exact priority match. Drop the old item and
                    // replace it with the new.
                    core::ptr::drop_in_place(start_ptr.add(idx));
                    core::ptr::write(start_ptr.add(idx), new_item);
                }
            }
            Err(idx) if idx == N::to_usize() => {
                // Nothing to do, off the end
            }
            Err(idx) if idx == self.used => {
                // Off the used end, but not off the total end
                unsafe {
                    core::ptr::write(start_ptr.add(idx), new_item);
                    self.used += 1;
                }
            }
            Err(idx) => {
                if self.used == N::to_usize() {
                    // Drop the last item, we're about to bump it
                    self.used -= 1;
                    unsafe {
                        core::ptr::drop_in_place(start_ptr.add(self.used));
                    }
                }
                unsafe {
                    let posn = start_ptr.add(idx);
                    // scootch over the array
                    core::ptr::copy(posn, posn.add(1), self.used - idx);
                    // Put the new item where it goes
                    core::ptr::write(posn, new_item);
                }
                self.used += 1;
            }
        }
    }

    /// Remove any expired items from the priority queue
    ///
    /// See the module level documentation for when it is necessary to call this function
    pub fn prune(&mut self) {
        let start_ptr = self.queue.as_mut_ptr().cast::<TopqItem<D, P, T>>();
        let now = self.timer.now();

        let mut good = 0;

        for idx in 0..self.used {
            unsafe {
                // For each used item...
                let idx_ptr = start_ptr.add(idx);
                let good_ptr = start_ptr.add(good);

                // Is the current item good?
                let idx_good = (*idx_ptr).valid_at_time(&now);

                if idx_good {
                    // No need to copy if we are already here
                    if good != idx {
                        // Drop the destination item
                        core::ptr::drop_in_place(good_ptr);

                        // Move from source to destination
                        core::ptr::copy_nonoverlapping(idx_ptr, good_ptr, 1);
                    }

                    // Move to the next good position
                    good += 1;
                } else {
                    // This item is bad, drop it
                    core::ptr::drop_in_place(idx_ptr);
                }
            }
        }

        self.used = good;
    }

    /// Obtain the highest priority and currently valid data, if any
    ///
    /// This is typically used when you ONLY need the current value, and not
    /// the remaining validity time or the priority of the currently valid data
    pub fn get_data(&self) -> Option<&D> {
        self.get_item().map(|i| &i.item)
    }

    /// Obtain the highest priority and currently valid topq item, if any
    ///
    /// This is typically used when you need the current value, AND ALSO need
    /// the remaining validity time or the priority of the currently valid data
    pub fn get_item(&self) -> Option<&TopqItem<D, P, T>> {
        let start_ptr = self.queue.as_ptr().cast::<TopqItem<D, P, T>>();
        let slice = unsafe { core::slice::from_raw_parts(start_ptr, self.used) };

        let now = self.timer.now();
        slice.iter().find(|item| item.valid_at_time(&now))
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
        let start_ptr = self.queue.as_ptr().cast::<TopqItem<D, P, T>>();
        let slice = unsafe { core::slice::from_raw_parts(start_ptr, self.used) };
        slice.iter()
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
        let start_ptr = self.queue.as_mut_ptr().cast::<TopqItem<D, P, T>>();
        let slice_mut = unsafe { core::slice::from_raw_parts_mut(start_ptr, self.used) };
        slice_mut.iter_mut()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use core::sync::atomic::{AtomicU32, Ordering::SeqCst};
    use generic_array::typenum::consts::*;

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

        q.prune();

        q.into_iter().for_each(|t| {
            println!("{:?}", t);
        });

        TIMER.store(21, SeqCst);
        assert_eq!(q.get_data(), Some(&11));

        TIMER.store(26, SeqCst);
        assert_eq!(q.get_data(), Some(&10));

        TIMER.store(31, SeqCst);
        assert_eq!(q.get_data(), None);

        q.prune();

        q.into_iter().for_each(|t| {
            println!("{:?}", t);
        });
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
