use crate::PublishSummary;

/// Accepted uploads remain published when a later operation fails.
#[derive(Debug)]
pub struct PublishFailure<Error> {
    pub published: Vec<PublishSummary>,
    pub error: Error,
}

impl<Error> From<Error> for PublishFailure<Error> {
    fn from(error: Error) -> Self {
        Self { published: Vec::new(), error }
    }
}
