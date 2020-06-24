# Timeout Priority Queue

This is a data structure that allows you to have multiple versions of data, with a timeout and priority level associated with each value.

## Example

```rust
// Create a timer instance. It must implement the `topq::Timer` trait.
let timer = SomeTimer;

// This is a priority queue that:
//  * Uses `u32` as the data item
//  * Uses a `u8` for the priority level
//  * Uses `SomeTimer` for the timer instance
//  * Supports 4 priority levels at once
let mut q: Topq<u32, u8, SomeTimer, U4> = Topq::new(timer);

// We start at time "0"

// Insert the value "10", at priority level "3", valid for
// "30" time units (the unit depends on the timer)
q.insert(10, 3, 30);

// Get the top priority item
assert_eq!(q.get(), Some(&10));

// Insert the value "11", at priority level "4", valid for
// "25" time units (the unit depends on the timer)
q.insert(11, 4, 25);

// Get the top priority item - it is now 11
assert_eq!(q.get(), Some(&11));

// Insert the value "12", at priority level "5", valid for
// "20" time units (the unit depends on the timer)
q.insert(12, 5, 20);

// Get the top priority item - it is now 12
assert_eq!(q.get(), Some(&12));

// Insert the value "13", at priority level "6", valid for
// "15" time units (the unit depends on the timer)
q.insert(13, 6, 15);
assert_eq!(q.get(), Some(&13));

// Fast forward to time "15". 13 is still valid here
assert_eq!(q.get(), Some(&13));

// Fast forward to time "16". 13 has expired, so 12 is
// now the highest priority + valid data
assert_eq!(q.get(), Some(&12));

// Fast forward to time "21". 12 has expired, so 11 is
// now the highest priority + valid data
assert_eq!(q.get(), Some(&11));

// Fast forward to time "26". 11 has expired, so 10 is
// now the highest priority + valid data
assert_eq!(q.get(), Some(&10));

// Fast forward to time "31". All items have expired,
// so there is no available data
assert_eq!(q.get(), None);
```
