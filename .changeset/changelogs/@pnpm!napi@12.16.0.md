## 12.16.0

### Patch Changes

- A dependency symlink that pnpm cannot point at the wanted target now fails the install with an error that names the link. This happens when the filesystem still serves the old link after pnpm removed it, as a `node_modules` served by a virtual filesystem does. pnpm used to remove the link again without end and crashed with a segmentation fault once the thread ran out of stack.
