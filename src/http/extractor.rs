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
