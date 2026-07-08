use super::request::BootRequest;
use crate::Result;

/// Custom request value extractor used by Nest-style controller argument binding.
pub trait RequestExtractor<T>: Send + Sync + 'static {
    fn extract(&self, request: &BootRequest) -> Result<T>;
}

impl<T, F> RequestExtractor<T> for F
where
    F: Fn(&BootRequest) -> Result<T> + Send + Sync + 'static,
{
    fn extract(&self, request: &BootRequest) -> Result<T> {
        self(request)
    }
}

pub fn extract_request_value<T, E>(request: &BootRequest, extractor: E) -> Result<T>
where
    E: RequestExtractor<T>,
{
    extractor.extract(request)
}

/// Transforms a single request value extracted from a path, query, header, or host parameter.
pub trait RequestValuePipe<I, O>: Send + Sync + 'static {
    fn transform(&self, value: I) -> Result<O>;
}

impl<I, O, F> RequestValuePipe<I, O> for F
where
    F: Fn(I) -> Result<O> + Send + Sync + 'static,
{
    fn transform(&self, value: I) -> Result<O> {
        self(value)
    }
}

pub fn transform_request_value<I, O, P>(value: I, pipe: P) -> Result<O>
where
    P: RequestValuePipe<I, O>,
{
    pipe.transform(value)
}
