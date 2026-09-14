## 1100.1.9

### Patch Changes

- GitHub Actions updates now stop if an action reference changes while its versions are being resolved. Unrelated workflow edits are preserved.

  GitHub Actions homepage links no longer expose server credentials. GitHub server URLs now require HTTPS, with HTTP allowed only for loopback hosts.

- Updated dependencies:
  - @pnpm/resolving.git-resolver@1100.1.21
