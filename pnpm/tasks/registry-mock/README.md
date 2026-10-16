# Registry mock storage

The mock stores each fixture generation in a separate directory under
`~/.cache/pnpm-registry/storage`. The directory name is the fixture fingerprint.
Changed or removed fixture packages cannot leave stale metadata in a new
generation. Concurrent generations use separate directories.

`PNPM_REGISTRY_STORAGE` overrides the parent directory, rather than the exact
path passed to `pnpr --storage`. Existing files directly under that parent are
left untouched and are not served.

Repeated launches of the same generation reuse its proxy cache and packages
added by benchmark scenarios. Seeding publishes a completion marker atomically.
An interrupted seed is completed before the mock starts serving. A directory
marked for another generation is rejected.

The benchmark workflow caches the parent directory and its generations.
