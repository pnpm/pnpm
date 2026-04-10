# @pnpm/fetching-types

## 1001.0.0

### Major Changes

- 491a84f: This package is now pure ESM.
- 7d2fd48: Node.js v18, 19, 20, and 21 support discontinued.
- 6c480a4: Replace node-fetch with undici as the HTTP client [#10537](https://github.com/pnpm/pnpm/pull/10537).

  - Use undici's native `fetch()` with dispatcher-based connection management
  - Support HTTP, HTTPS, SOCKS4, and SOCKS5 proxies
  - Cache dispatchers via LRU cache keyed by connection parameters
  - Handle per-registry client certificates via nerf-dart URL matching
  - Convert test HTTP mocking from nock to undici MockAgent

### Patch Changes

- bb8baa7: Fixed optional dependencies to request full metadata from the registry to get the `libc` field, which is required for proper platform compatibility checks [#9950](https://github.com/pnpm/pnpm/issues/9950).

## 1000.2.0

### Minor Changes

- 1ba2e15: Export type Response.

## 1000.1.0

### Minor Changes

- b0f3c71: The `fetch` function accepts a `method` option now.

## 6.0.0

### Major Changes

- 43cdd87: Node.js v16 support dropped. Use at least Node.js v18.12.

## 5.0.0

### Major Changes

- eceaa8b8b: Node.js 14 support dropped.

## 4.0.0

### Major Changes

- 804de211e: GetCredentials function replaced with GetAuthHeader.

## 3.0.0

### Major Changes

- 542014839: Node.js 12 is not supported.

## 2.2.1

### Patch Changes

- bab172385: The Node.js process should not silently exit on some broken HTTPS requests.

## 2.2.0

### Minor Changes

- eadf0e505: New optional option added: compress.

## 2.1.0

### Minor Changes

- 05baaa6e7: Add new option: timeout.

## 2.0.0

### Major Changes

- 97b986fbc: Node.js 10 support is dropped. At least Node.js 12.17 is required for the package to work.

## 1.0.0

### Major Changes

- 71aeb9a38: Initial version.
