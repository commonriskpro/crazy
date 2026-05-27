use super::{
    FetchRequest, FetchResponse, PublishRequest, PublishResponse, SearchRequest, SearchResponse,
    VerifyRequest, VerifyResponse,
};

/// Trait defining the remote registry client interface.
///
/// Implementors provide the transport (HTTP, gRPC, in-process mock, etc.).
/// This trait is synchronous and returns owned values; async wrappers can be
/// layered on top.
pub trait RegistryClient {
    /// Error type returned by all registry operations.
    type Error: std::fmt::Debug;

    /// Publish a signed package to the registry.
    fn publish(&self, request: PublishRequest) -> Result<PublishResponse, Self::Error>;

    /// Fetch a specific package version from the registry.
    fn fetch(&self, request: FetchRequest) -> Result<FetchResponse, Self::Error>;

    /// Search for packages matching a query.
    fn search(&self, request: SearchRequest) -> Result<SearchResponse, Self::Error>;

    /// Verify a package's integrity and advisory status.
    fn verify(&self, request: VerifyRequest) -> Result<VerifyResponse, Self::Error>;
}
