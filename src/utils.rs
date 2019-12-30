use pin_project::pin_project;
use futures::Stream;
use futures::future::Either;
use futures::task::{Context, Poll};
use std::pin::Pin;

#[pin_project]
pub struct NextEither<A, B>
where
    A: Stream,
    B: Stream,
{
    #[pin]
    left: A,
    #[pin]
    right: B,
}

impl<A, B> NextEither<A, B>
where
    A: Stream,
    B: Stream,
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
    A: Stream,
    B: Stream,
{
    type Item = Either<A::Item, B::Item>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut projection = self.project();

        // todo either require that these are fused (we need to check that framed streams can be
        // fused and then produce values after receiving more bytes
        match projection.left.poll_next(cx) {
            Poll::Ready(Some(output)) => return Poll::Ready(Some(Either::Left(output))),
            _ => {}
        };

        match projection.right.poll_next(cx) {
            Poll::Ready(Some(output)) => return Poll::Ready(Some(Either::Right(output))),
            _ => {}
        };

        Poll::Pending
    }
}

pub fn next_either<A, B>(left: A, right: B) -> NextEither<A, B>
where
    A: Stream,
    B: Stream,
{
    NextEither::new(left, right)
}
