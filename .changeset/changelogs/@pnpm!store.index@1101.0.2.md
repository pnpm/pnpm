## 1101.0.2

### Patch Changes

- An install that adds nothing to the store no longer changes the modification time of the store directory. CI caches that check modification times, such as GitLab's, no longer re-upload an unchanged store [#16929](https://github.com/pnpm/pnpm/issues/16929).
