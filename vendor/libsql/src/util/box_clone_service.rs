// Copied from https://docs.rs/tower/latest/tower/util/struct.BoxCloneService.html
// This is because in the tower version the trait object only implements `Send` which
// means we can't call clients from context that need `Sync` like an `async fn` that needs
// to be `Send` (must be sync as well to impl Send).

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use tower::Service;
use tower::ServiceExt;

type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

pub struct BoxCloneService<T, U, E>(
    Box<
        dyn CloneService<T, Response = U, Error = E, Future = BoxFuture<Result<U, E>>>
            + Send
            + Sync,
    >,
);

impl<T, U, E> BoxCloneService<T, U, E> {
    /// Create a new `BoxCloneService`.
    pub fn new<S>(inner: S) -> Self
    where
        S: Service<T, Response = U, Error = E> + Clone + Sync + Send + 'static,
        S::Future: Send + 'static,
    {
        let inner = inner.map_future(|f| Box::pin(f) as _);
        BoxCloneService(Box::new(inner))
    }
}

impl<T, U, E> Service<T> for BoxCloneService<T, U, E> {
    type Response = U;
    type Error = E;
    type Future = BoxFuture<Result<U, E>>;

    #[inline]
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), E>> {
        self.0.poll_ready(cx)
    }

    #[inline]
    fn call(&mut self, request: T) -> Self::Future {
        self.0.call(request)
    }
}

impl<T, U, E> Clone for BoxCloneService<T, U, E> {
    fn clone(&self) -> Self {
        Self(self.0.clone_box())
    }
}

trait CloneService<R>: Service<R> {
    fn clone_box(
        &self,
    ) -> Box<
        dyn CloneService<R, Response = Self::Response, Error = Self::Error, Future = Self::Future>
            + Send
            + Sync,
    >;
}

impl<R, T> CloneService<R> for T
where
    T: Service<R> + Send + Sync + Clone + 'static,
{
    fn clone_box(
        &self,
    ) -> Box<
        dyn CloneService<R, Response = T::Response, Error = T::Error, Future = T::Future>
            + Send
            + Sync,
    > {
        Box::new(self.clone())
    }
}

impl<T, U, E> fmt::Debug for BoxCloneService<T, U, E> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.debug_struct("BoxCloneService").finish()
    }
}
