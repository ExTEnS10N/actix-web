//! Payload stream
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::{Rc, Weak};

use bytes::Bytes;
use futures::task::current as current_task;
use futures::task::Task;
use futures::{Async, Poll, Stream};

use crate::error::PayloadError;

/// max buffer size 32k
pub(crate) const MAX_BUFFER_SIZE: usize = 32_768;

#[derive(Debug, PartialEq)]
pub(crate) enum PayloadStatus {
    Read,
    Pause,
    Dropped,
}

/// Buffered stream of bytes chunks
///
/// Payload stores chunks in a vector. First chunk can be received with
/// `.readany()` method. Payload stream is not thread safe. Payload does not
/// notify current task when new data is available.
///
/// Payload stream can be used as `Response` body stream.
#[derive(Debug)]
pub struct Payload {
    inner: Rc<RefCell<Inner>>,
}

impl Payload {
    /// Create payload stream.
    ///
    /// This method construct two objects responsible for bytes stream
    /// generation.
    ///
    /// * `PayloadSender` - *Sender* side of the stream
    ///
    /// * `Payload` - *Receiver* side of the stream
    pub fn create(eof: bool) -> (PayloadSender, Payload) {
        let shared = Rc::new(RefCell::new(Inner::new(eof)));

        (
            PayloadSender {
                inner: Rc::downgrade(&shared),
            },
            Payload { inner: shared },
        )
    }

    /// Create empty payload
    #[doc(hidden)]
    pub fn empty() -> Payload {
        Payload {
            inner: Rc::new(RefCell::new(Inner::new(true))),
        }
    }

    /// Length of the data in this payload
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.inner.borrow().len()
    }

    /// Is payload empty
    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.inner.borrow().len() == 0
    }

    /// Put unused data back to payload
    #[inline]
    pub fn unread_data(&mut self, data: Bytes) {
        self.inner.borrow_mut().unread_data(data);
    }

    #[inline]
    /// Set read buffer capacity
    ///
    /// Default buffer capacity is 32Kb.
    pub fn set_read_buffer_capacity(&mut self, cap: usize) {
        self.inner.borrow_mut().capacity = cap;
    }
}

impl Stream for Payload {
    type Item = Bytes;
    type Error = PayloadError;

    #[inline]
    fn poll(&mut self) -> Poll<Option<Bytes>, PayloadError> {
        self.inner.borrow_mut().readany()
    }
}

impl Clone for Payload {
    fn clone(&self) -> Payload {
        Payload {
            inner: Rc::clone(&self.inner),
        }
    }
}

/// Payload writer interface.
pub(crate) trait PayloadWriter {
    /// Set stream error.
    fn set_error(&mut self, err: PayloadError);

    /// Write eof into a stream which closes reading side of a stream.
    fn feed_eof(&mut self);

    /// Feed bytes into a payload stream
    fn feed_data(&mut self, data: Bytes);

    /// Need read data
    fn need_read(&self) -> PayloadStatus;
}

/// Sender part of the payload stream
pub struct PayloadSender {
    inner: Weak<RefCell<Inner>>,
}

impl PayloadWriter for PayloadSender {
    #[inline]
    fn set_error(&mut self, err: PayloadError) {
        if let Some(shared) = self.inner.upgrade() {
            shared.borrow_mut().set_error(err)
        }
    }

    #[inline]
    fn feed_eof(&mut self) {
        if let Some(shared) = self.inner.upgrade() {
            shared.borrow_mut().feed_eof()
        }
    }

    #[inline]
    fn feed_data(&mut self, data: Bytes) {
        if let Some(shared) = self.inner.upgrade() {
            shared.borrow_mut().feed_data(data)
        }
    }

    #[inline]
    fn need_read(&self) -> PayloadStatus {
        // we check need_read only if Payload (other side) is alive,
        // otherwise always return true (consume payload)
        if let Some(shared) = self.inner.upgrade() {
            if shared.borrow().need_read {
                PayloadStatus::Read
            } else {
                #[cfg(not(test))]
                {
                    if shared.borrow_mut().io_task.is_none() {
                        shared.borrow_mut().io_task = Some(current_task());
                    }
                }
                PayloadStatus::Pause
            }
        } else {
            PayloadStatus::Dropped
        }
    }
}

#[derive(Debug)]
struct Inner {
    len: usize,
    eof: bool,
    err: Option<PayloadError>,
    need_read: bool,
    items: VecDeque<Bytes>,
    capacity: usize,
    task: Option<Task>,
    io_task: Option<Task>,
}

impl Inner {
    fn new(eof: bool) -> Self {
        Inner {
            eof,
            len: 0,
            err: None,
            items: VecDeque::new(),
            need_read: true,
            capacity: MAX_BUFFER_SIZE,
            task: None,
            io_task: None,
        }
    }

    #[inline]
    fn set_error(&mut self, err: PayloadError) {
        self.err = Some(err);
    }

    #[inline]
    fn feed_eof(&mut self) {
        self.eof = true;
    }

    #[inline]
    fn feed_data(&mut self, data: Bytes) {
        self.len += data.len();
        self.items.push_back(data);
        self.need_read = self.len < self.capacity;
        if let Some(task) = self.task.take() {
            task.notify()
        }
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.len
    }

    fn readany(&mut self) -> Poll<Option<Bytes>, PayloadError> {
        if let Some(data) = self.items.pop_front() {
            self.len -= data.len();
            self.need_read = self.len < self.capacity;

            if self.need_read && self.task.is_none() && !self.eof {
                self.task = Some(current_task());
            }
            if let Some(task) = self.io_task.take() {
                task.notify()
            }
            Ok(Async::Ready(Some(data)))
        } else if let Some(err) = self.err.take() {
            Err(err)
        } else if self.eof {
            Ok(Async::Ready(None))
        } else {
            self.need_read = true;
            #[cfg(not(test))]
            {
                if self.task.is_none() {
                    self.task = Some(current_task());
                }
                if let Some(task) = self.io_task.take() {
                    task.notify()
                }
            }
            Ok(Async::NotReady)
        }
    }

    fn unread_data(&mut self, data: Bytes) {
        self.len += data.len();
        self.items.push_front(data);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_rt::Runtime;
    use futures::future::{lazy, result};

    #[test]
    fn test_unread_data() {
        Runtime::new()
            .unwrap()
            .block_on(lazy(|| {
                let (_, mut payload) = Payload::create(false);

                payload.unread_data(Bytes::from("data"));
                assert!(!payload.is_empty());
                assert_eq!(payload.len(), 4);

                assert_eq!(
                    Async::Ready(Some(Bytes::from("data"))),
                    payload.poll().ok().unwrap()
                );

                let res: Result<(), ()> = Ok(());
                result(res)
            }))
            .unwrap();
    }
}