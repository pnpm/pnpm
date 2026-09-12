use super::{
    Arc, Footprint, Identity, MetadataCacheScope, Mutex, PrivateAccessDescriptor, RouteClass,
    RouteContext, UpstreamRouteHook, fmt,
};

/// The [`UpstreamRouteHook`] pnpr installs on a resolve's
/// [`AuthHeaders`](pnpm_network::AuthHeaders). Every metadata/tarball
/// fetch routes through [`UpstreamRouteHook::authorize`], which classifies
/// the route, records it into the shared [`Footprint`], and returns the
/// pnpr-managed credential (never a client-forwarded one).
pub struct RouteHook {
    pub(super) context: Arc<RouteContext>,
    pub(super) identity: Identity,
    pub(super) footprint: Arc<Mutex<Footprint>>,
    /// HMAC secret keying the per-descriptor metadata namespace
    /// ([`MetadataCacheScope::Private`]); the same server secret the
    /// resolution cache keys private footprints with.
    pub(super) secret: Arc<[u8]>,
}

impl fmt::Debug for RouteHook {
    /// Redacts `Self::secret` — the descriptor-HMAC key must never reach a
    /// log line or panic dump, or the private namespace becomes correlatable.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RouteHook")
            .field("context", &self.context)
            .field("identity", &self.identity)
            .field("footprint", &self.footprint)
            .field("secret", &"<redacted>")
            .finish()
    }
}

impl RouteHook {
    #[must_use]
    pub fn new(
        context: Arc<RouteContext>,
        identity: Identity,
        footprint: Arc<Mutex<Footprint>>,
        secret: Arc<[u8]>,
    ) -> Self {
        Self { context, identity, footprint, secret }
    }
}

impl UpstreamRouteHook for RouteHook {
    fn authorize(&self, url: &str, package: Option<&str>) -> Option<String> {
        match self.context.classify(&self.identity, url, package) {
            RouteClass::Public => None,
            RouteClass::Hosted { policy_id } => {
                self.record(PrivateAccessDescriptor::Hosted { policy_id });
                // Hosted packages are served by pnpr itself; no upstream
                // credential is involved.
                None
            }
            RouteClass::Proxied { alias, credential_digest } => {
                let authorization = self
                    .context
                    .aliases
                    .iter()
                    .find(|candidate| candidate.name == alias)
                    .map(|candidate| candidate.authorization.clone());
                // Package-qualified only when the upstream's rules explicitly
                // refine this name, so replay re-checks the refinement.
                let package = self.context.alias_package_qualifier(&alias, package);
                self.record(PrivateAccessDescriptor::Alias { alias, credential_digest, package });
                authorization
            }
        }
    }

    fn allows_fetch(&self, url: &str) -> bool {
        self.context.allows_registry(url)
    }

    fn metadata_scope(&self, url: &str, package: Option<&str>) -> MetadataCacheScope {
        // Read-only classification — this must not record into the
        // footprint (`authorize` already does, at the real fetch point).
        match self.context.classify(&self.identity, url, package) {
            RouteClass::Public => MetadataCacheScope::Public,
            RouteClass::Hosted { policy_id } => MetadataCacheScope::Private {
                descriptor_id: PrivateAccessDescriptor::Hosted { policy_id }
                    .digest_id(&self.secret),
            },
            RouteClass::Proxied { alias, credential_digest } => MetadataCacheScope::Private {
                descriptor_id: {
                    let package = self.context.alias_package_qualifier(&alias, package);
                    PrivateAccessDescriptor::Alias { alias, credential_digest, package }
                        .digest_id(&self.secret)
                },
            },
        }
    }
}

impl RouteHook {
    pub(super) fn record(&self, descriptor: PrivateAccessDescriptor) {
        self.footprint.lock().expect("footprint poisoned").add(descriptor);
    }
}
