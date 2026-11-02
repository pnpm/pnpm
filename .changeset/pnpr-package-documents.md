---
"pacquet": minor
"@pnpm/pnpr": minor
---

pnpm now reads pnpr's own package document from a registry declared with `serverType: pnpr`. A large document leaves out the manifests of the versions a resolution rarely picks, and pnpm fetches the manifest of such a version only when it picks one. These documents are cached in `v12/metadata-pnpr`, and the `pnpm cache` commands cover them.

pnpm warns when a registry that is not declared `serverType: pnpr` turns out to be pnpr. A `minimumReleaseAge` longer than 30 days fails with `ERR_PNPM_UNSUPPORTED_MINIMUM_RELEASE_AGE` for a registry declared `serverType: pnpr`.

pnpr serves the new document to clients that ask for `application/vnd.pnpr.install-v2+json`, and each version's release record at `<name>/<version>+r<revision>`. npm's documents served by pnpr no longer carry `dist.pnpr`, `dist.revision`, or `dist.revisions`. The new `packageDocuments` setting configures the document's split threshold and publish-time window.
