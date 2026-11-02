---
"pacquet": minor
"@pnpm/pnpr": minor
---

pnpm downloads less metadata from a registry declared with `serverType: pnpr`. It asks such a registry for pnpr's own package document. A large document leaves out the manifests of versions that are rarely picked. pnpm fetches such a manifest only when it picks that version. In a benchmark that resolves 787 packages, the package documents shrank from 30.3 MB to 2.5 MB gzipped. pnpm caches these documents in `v12/metadata-pnpr`, and the `pnpm cache` commands cover them.

pnpm warns when a registry that is not declared `serverType: pnpr` turns out to be pnpr. For a registry declared `serverType: pnpr`, a `minimumReleaseAge` longer than 30 days fails with `ERR_PNPM_UNSUPPORTED_MINIMUM_RELEASE_AGE`.

pnpr serves its own package document to clients that ask for `application/vnd.pnpr.install-v2+json`. It serves each version's release record at `<name>/<version>+r<revision>`. npm's documents served by pnpr no longer carry `dist.pnpr`, `dist.revision`, or `dist.revisions`. The new `packageDocuments` setting configures the split threshold and the publish-time window.
