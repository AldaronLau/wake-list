#![no_std]

extern crate alloc;

use alloc::boxed::Box;
use core::{
    cell::UnsafeCell,
    num::NonZeroUsize,
    ptr,
    sync::atomic::{
        AtomicPtr, AtomicUsize, AtomicBool,
        Ordering::{Relaxed, SeqCst},
    },
    task::Waker,
};

// `AtomicMaybeWaker` requires https://github.com/rust-lang/rust/issues/87021
/*

struct AtomicMaybeWaker {
    vtable: AtomicPtr<RawWakerVTable>,
    data: *const (),
}

*/

pub struct WakeHandle(usize);

/// A `WakeList` stores append-only atomic linked lists of wakers and garbage
pub struct WakeList {
    // List of wakers (eventually will be able to use `AtomicMaybeWaker`)
    wakers: AtomicLinkedList<(AtomicBool, UnsafeCell<Option<Waker>>)>,
    // List of garbage
    garbage: AtomicLinkedList<AtomicUsize>,
    // Next handle ID / length of wakers list
    size: AtomicUsize,
}

impl WakeList {
    pub fn new() -> Self {
        let wakers = AtomicLinkedList::new();
        let garbage = AtomicLinkedList::new();
        let size = AtomicUsize::new(0);

        Self {
            wakers, garbage, size,
        }
    }

    /// Register a waker
    pub fn register(&self, waker: impl Into<Option<Waker>>) -> WakeHandle {
        let waker = waker.into();

        // Check garbage for reuse
        let garbage = 'garbage: {
            for garbage in self.garbage.iter() {
                if let Some(wh) = NonZeroUsize::new(garbage.fetch_and(0, Relaxed)) {
                    break 'garbage Some(wh);
                }
            }
            None
        };

        let handle = if let Some(index) = garbage {
            // Replace existing
            let index = usize::from(index) - 1;
            let mut size_a = self.size.load(SeqCst);
            loop {
                let size_b = size_a;
                let wakey = self.wakers.iter().skip(size_a - index).next().unwrap();
                size_a = self.size.load(SeqCst);
                if size_a == size_b {
                    unsafe { *wakey.1.get() = waker.into() };
                    break;
                }
            }
            index
        } else {
            // If no garbage exists, push new pair
            self.wakers.push((AtomicBool::new(false), waker.into()));
            self.size.fetch_add(1, Relaxed)
        };

        WakeHandle(handle)
    }

    pub fn reregister(&self, handle: WakeHandle, waker: Waker) {

    }

    pub fn unregister(&self, handle: WakeHandle) {

    }

    pub fn wake_one(&self) {

    }
}

struct Node<T> {
    next: AtomicPtr<Node<T>>,
    data: T,
}

struct AtomicLinkedList<T> {
    root: AtomicPtr<Node<T>>,
}

impl<T> AtomicLinkedList<T> {
    fn new() -> Self {
        let root = AtomicPtr::new(ptr::null_mut());

        Self { root }
    }

    fn push(&self, data: T) {
        let mut ptr = self.root.load(Relaxed);
        // Create node
        let next = AtomicPtr::new(ptr);
        let node = Box::into_raw(Box::new(Node { next, data }));
        // Push node
        while let Err(other) =
            self.root.compare_exchange(ptr, node, SeqCst, Relaxed)
        {
            ptr = other;
            unsafe { (*node).next.store(ptr, Relaxed) };
        }
    }

    fn iter(&self) -> AtomicLinkedListIter<'_, T> {
        AtomicLinkedListIter {
            _all: self,
            next: self.root.load(Relaxed),
        }
    }
}

struct AtomicLinkedListIter<'a, T> {
    _all: &'a AtomicLinkedList<T>,
    next: *mut Node<T>,
}

impl<'a, T> Iterator for AtomicLinkedListIter<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<&'a T> {
        if self.next.is_null() {
            return None;
        }

        let ret: *const T = unsafe { &(*self.next).data };

        // Advance
        self.next = unsafe { (*self.next).next.load(Relaxed) };

        Some(unsafe { &*ret })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple() {
        let list = AtomicLinkedList::new();
        assert!(list.iter().next().is_none());

        list.push(42);
        assert_eq!(
            alloc::vec![42],
            list.iter().cloned().collect::<alloc::vec::Vec<u32>>()
        );
    }
}
