#![allow(dead_code)]

use pin_project::pin_project;
use std::{marker::PhantomData, sync::Mutex};

use crate::sink::{PollSend, Sink};

pub fn ready<T>() -> impl Sink<Item = T>
where
    T: Clone,
{
    ReadySink::new()
}

pub fn pending<T>() -> impl Sink<Item = T> {
    PendingSink::new()
}

pub fn rejected<T>() -> impl Sink<Item = T> {
    RejectedSink::new()
}

pub fn test_sink<T, I>(iter: I) -> TestSink<I::IntoIter, T>
where
    I: IntoIterator<Item = PollSend<T>>,
    T: Clone,
{
    TestSink::new(iter.into_iter())
}

struct ReadySink<T> {
    _t: PhantomData<T>,
}

impl<T> ReadySink<T> {
    pub fn new() -> Self {
        Self { _t: PhantomData }
    }
}

impl<T> Sink for ReadySink<T> {
    type Item = T;

    fn poll_send(
        self: std::pin::Pin<&Self>,
        _cx: &mut crate::Context<'_>,
        _value: Self::Item,
    ) -> crate::sink::PollSend<Self::Item> {
        PollSend::Ready
    }
}

struct PendingSink<T> {
    _t: PhantomData<T>,
}

impl<T> PendingSink<T> {
    pub fn new() -> Self {
        Self { _t: PhantomData }
    }
}

impl<T> Sink for PendingSink<T> {
    type Item = T;

    fn poll_send(
        self: std::pin::Pin<&Self>,
        _cx: &mut crate::Context<'_>,
        value: Self::Item,
    ) -> PollSend<Self::Item> {
        PollSend::Pending(value)
    }
}
struct RejectedSink<T> {
    _t: PhantomData<T>,
}

impl<T> RejectedSink<T> {
    pub fn new() -> Self {
        Self { _t: PhantomData }
    }
}

impl<T> Sink for RejectedSink<T> {
    type Item = T;

    fn poll_send(
        self: std::pin::Pin<&Self>,
        _cx: &mut crate::Context<'_>,
        value: Self::Item,
    ) -> PollSend<Self::Item> {
        PollSend::Rejected(value)
    }
}
#[pin_project]
pub struct TestSink<I: Iterator, T> {
    iter: Mutex<I>,
    values: Mutex<Vec<T>>,
}

impl<I, T> TestSink<I, T>
where
    I: Iterator,
    T: Clone,
{
    pub fn new(iter: I) -> Self {
        Self {
            iter: Mutex::new(iter),
            values: Mutex::new(Vec::new()),
        }
    }

    pub fn values(&self) -> Vec<T> {
        self.values.lock().unwrap().clone()
    }
}

impl<I, T> Sink for TestSink<I, T>
where
    I: Iterator<Item = PollSend<T>>,
    T: Clone,
{
    type Item = T;

    fn poll_send(
        self: std::pin::Pin<&Self>,
        _cx: &mut crate::Context<'_>,
        value: Self::Item,
    ) -> PollSend<Self::Item> {
        let this = self.project_ref();
        let saved_value = value.clone();
        match this.iter.lock().unwrap().next() {
            Some(poll) => {
                if let PollSend::Ready = poll {
                    this.values.lock().unwrap().push(saved_value);
                }
                poll
            }
            None => PollSend::Rejected(value),
        }
    }
}
