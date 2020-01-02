use futures::future::Either;
use futures::task::{Context, Poll};
use futures::Stream;
use futures_util::stream::StreamExt;
use pin_project::pin_project;
use std::pin::Pin;

#[pin_project]
pub struct NextEither<A, B>
where
    A: Stream + std::marker::Unpin,
    B: Stream + std::marker::Unpin,
{
    left: A,
    right: B,
}

impl<A, B> NextEither<A, B>
where
    A: Stream + std::marker::Unpin,
    B: Stream + std::marker::Unpin,
{
    fn new(left: A, right: B) -> Self {
        Self { left, right }
    }

    /// Releases the streams
    pub fn release(self) -> (A, B) {
        (self.left, self.right)
    }
}

impl<A, B> Stream for NextEither<A, B>
where
    A: Stream + std::marker::Unpin,
    B: Stream + std::marker::Unpin,
{
    type Item = (Vec<A::Item>, Vec<B::Item>);

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut left_results = Vec::new();
        let mut right_results = Vec::new();

        // todo either require that these are fused (we need to check that framed streams can be
        // fused and then produce values after receiving more bytes

        for i in 0..100u32 {
            match self.left.poll_next_unpin(cx) {
                Poll::Ready(Some(output)) => left_results.push(output),
                _ => break,
            };
        }

        for i in 0..100u32 {
            match self.right.poll_next_unpin(cx) {
                Poll::Ready(Some(output)) => right_results.push(output),
                _ => break,
            };
        }

        if left_results.is_empty() && right_results.is_empty() {
            Poll::Pending
        } else {
            Poll::Ready(Some((left_results, right_results)))
        }
    }
}

pub fn next_either<A, B>(left: A, right: B) -> NextEither<A, B>
where
    A: Stream + std::marker::Unpin,
    B: Stream + std::marker::Unpin,
{
    NextEither::new(left, right)
}
