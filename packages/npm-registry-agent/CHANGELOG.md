# @pnpm/npm-registry-agent

## 6.1.0

### Minor Changes

- d5730ba81: The ca and cert options may accept an array of string.

## 6.0.0

### Major Changes

- 542014839: Node.js 12 is not supported.

## 5.0.2

### Patch Changes

- 3c7e5eced: Proxy URLs with special characters in credentials should work.

## 5.0.1

### Patch Changes

- 6c50af201: Update socks-proxy-agent to v6.
- 0beffc2a0: `http-proxy-agent` and `https-proxy-agent` update to v5.

## 5.0.0

### Major Changes

- eeff424bd: strictSSL option renamed to strictSsl.

## 4.0.0

### Major Changes

- 97b986fbc: Node.js 10 support is dropped. At least Node.js 12.17 is required for the package to work.

## 3.1.2

### Patch Changes

- dd12cf6ec: Creating a proper URL for socks proxy.

## 3.1.1

### Patch Changes

- dc5a0a102: Increase the default maximum number of connections to use per origin to 50 (from 15).

## 3.1.0

### Minor Changes

- 46128b5b0: Export AgentOptions type.

## 3.0.1

### Patch Changes

- 7b98d16c8: Update lru-cache to v6

## 3.0.0

### Major Changes

- c3796a71d: Not reading the env variables anymore. The env variables are read by @pnpm/config.

## 2.0.4
