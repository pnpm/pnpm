use super::{Ecosystem, wildcard_shapes};

/// A static registry-configuration defect. Surfaced by [`crate::Registries::validate`] and by
/// [`crate::PackagePattern::parse`]; the `config` module turns it into an
/// `InvalidConfig` so a bad registry set fails server startup and config reload.
#[derive(Debug, derive_more::Display, Clone, PartialEq, Eq)]
pub enum RegistryConfigError {
    /// A qualified identity conflicts with another entry or its ecosystem.
    #[display(
        "registry identity {registry:?} duplicates a registry or conflicts with its ecosystem or sources"
    )]
    InvalidRegistryIdentity { registry: String },
    /// An unsupported wildcard in a registry pattern. Carries the ecosystem
    /// so the message can name the shapes that ecosystem admits, rather than
    /// sending an operator to one it always refuses.
    #[display("unsupported {ecosystem} registry pattern {pattern:?}: use an exact name, {}, or \
                 `**`",
                wildcard_shapes(*ecosystem))]
    InvalidPattern { pattern: String, ecosystem: Ecosystem },
    /// A wildcard-free registry pattern that is not a well-formed package name,
    /// so it could never match any request.
    #[display(
        "registry pattern {pattern:?} is not a valid package name, so it can never match; \
                 to claim every package in a scope use `@scope/*`"
    )]
    ExactPatternNotAName { pattern: String },
    /// A `@<scope>/*` pattern whose scope no well-formed package name can
    /// carry, so it could never match any request.
    #[display(
        "registry pattern {pattern:?} does not name a valid scope, so it can never match \
                 any package"
    )]
    ScopePatternNotAScope { pattern: String },
    /// A `<namespace>/*` pattern whose namespace no well-formed image
    /// repository name can carry, so it could never match any request.
    #[display(
        "registry pattern {pattern:?} does not name a valid image namespace, so it can \
                 never match any repository"
    )]
    NamespacePatternNotANamespace { pattern: String },
    /// `defaultRegistry` names a registry that does not exist.
    #[display("defaultRegistry {target:?} is not a defined registry")]
    UndefinedDefaultRegistry { target: String },
    /// An ecosystem-specific default has no concrete source serving its protocol.
    #[display(
        "defaultRegistry for {ecosystem} targets {target:?}, which has no source serving {ecosystem}"
    )]
    DefaultRegistryWithoutEcosystem { target: String, ecosystem: Ecosystem },
    /// A router has no sources at all, so it can never serve any package.
    #[display(
        "router {router:?} has no sources, so it can never serve any package; add \
                 sources or remove the registry"
    )]
    EmptyRouter { router: String },
    /// A router lists itself as a source.
    #[display("router {router:?} lists itself as a source")]
    SelfReferentialRouter { router: String },
    /// A router source is not a defined registry.
    #[display("router {router:?} source {source:?} is not a defined registry")]
    UnknownSource { router: String, source: String },
    /// A router source is another router, not a concrete registry.
    #[display(
        "router {router:?} source {source:?} is itself a router; a source must be a \
                 hosted or upstream registry"
    )]
    NonConcreteSource { router: String, source: String },
    /// A router lists the same source more than once.
    #[display("router {router:?} lists source {source:?} more than once")]
    DuplicateSource { router: String, source: String },
    /// A concrete registry declares the same pattern more than once.
    #[display("registry {registry:?} declares pattern {pattern:?} more than once")]
    DuplicatePattern { registry: String, pattern: String },
    /// A router source's claims are fully covered by earlier sources, so it
    /// can never be selected.
    #[display("router {router:?} source #{index} ({source:?}) is unreachable: earlier sources \
                 already claim every package it would serve; list it before the sources that \
                 shadow it, or remove it",
                index = index + 1)]
    UnreachableSource { router: String, index: usize, source: String },
    /// A single pattern of a later source is covered by an earlier source's
    /// pattern, so it can never be selected in this router even though the
    /// rest of its source stays reachable.
    #[display(
        "router {router:?} can never select source {source:?} for its pattern \
                 {pattern:?}: an earlier source's pattern {by:?} already claims every package it \
                 would; reorder the sources or adjust the declared namespaces"
    )]
    ShadowedPattern { router: String, source: String, pattern: String, by: String },
    /// An ecosystem is declared for a name that is not a concrete registry.
    #[display(
        "registry {registry:?} declares ecosystem {ecosystem} but is not a hosted or \
                 upstream registry"
    )]
    EcosystemOnNonConcreteRegistry { registry: String, ecosystem: Ecosystem },
}

impl std::error::Error for RegistryConfigError {}
