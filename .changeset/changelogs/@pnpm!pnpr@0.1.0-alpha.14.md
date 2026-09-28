## 0.1.0-alpha.14

pnpr 0.1.0-alpha.14 stops its resolver from connecting to private network addresses, links workspace projects at their `publishConfig.directory`, and resolves Cargo dependencies faster.

### Minor Changes

- The pnpr resolver no longer connects to loopback, private, link-local, or other non-public addresses, such as the `169.254.169.254` cloud metadata endpoint. The check applies to the address of each connection, so an allowlisted registry host that resolves to an internal address is refused too [#12705](https://github.com/pnpm/pnpm/issues/12705).

  Addresses of the `public_url` host stay reachable. List any other non-public network the resolver must reach, such as the one of an internal upstream registry, under `routes.allowedPrivateNetworks`.

  pnpr now checks transitive git and tarball dependencies against the fetch allowlist before it contacts their host. With git 2.37 or later, git fetches over `http(s)` connect only to the addresses pnpr checked.

### Patch Changes

- Installing through a pnpr server now links a workspace project at the directory its `publishConfig.directory` names. Before, the project root was linked. An install through a server that does not forward the setting fails with `ERR_PNPM_PNPR_PUBLISH_DIRECTORY_MISMATCH`, and the server rejects a `publishConfig.directory` that points outside its project [#14460](https://github.com/pnpm/pnpm/issues/14460).

- Cargo dependency resolution across sparse-index registries is faster. pnpr removes expired Cargo sparse-index cache files automatically [#14611](https://github.com/pnpm/pnpm/issues/14611).
