## 1100.2.3

### Patch Changes

- Cached metadata for a package published within `minimumReleaseAge` is now revalidated with its ETag, so the npm registry can answer `304 Not Modified`. Before, the next install that checked the cache downloaded the whole document again.
