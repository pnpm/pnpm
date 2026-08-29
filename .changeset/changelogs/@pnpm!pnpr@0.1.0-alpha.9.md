## 0.1.0-alpha.9

### Minor Changes

- Made the signed shared-artifact cache horizontally scalable with S3-compatible storage and an independent top-level `artifacts` feature toggle.

- Generalized the experimental shared-artifact protocol so candidates and signed payloads identify a discriminated subject. Dependency side effects use package and source-integrity subjects, while workspace tasks use project and task subjects.

  This changes shared-artifact request bodies and signed payloads. A pnpr server and its clients have to be on matching versions.

### Patch Changes

- A published build artifact is now immutable: one input key and one set of compatibility constraints admit one artifact, so publishing a different one over it answers `409 Conflict`, the same as a re-published `name@version`. Republishing the identical artifact still succeeds. Artifacts already stored by an earlier version keep their slot, so upgrading a populated registry does not leave them replaceable.

- A published build artifact is now immutable for every consumer it can reach, not only for the exact compatibility constraints it declares. Publishing an artifact that reaches a machine an existing one already reaches answers `409 Conflict`, so a later universal, broader, or higher-floor build can no longer take precedence over the artifact a consumer already receives. Publishing across a platform matrix is unaffected: artifacts for different operating systems, architectures, or Node majors reach no machine in common and coexist as before.

- pnpr reclaims unreferenced shared artifact blobs after ambiguous object-storage write failures once active publications drain.

- Recognize `pnpm install --fix-lockfile`, including filtered installs, and regenerate broken lockfile metadata while preserving compatible locked versions [pnpm/pnpm#14250](https://github.com/pnpm/pnpm/issues/14250).

- pnpr retains shared artifact quota after object storage reports an ambiguous write failure.
