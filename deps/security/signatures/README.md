# @pnpm/deps.security.signatures

> Verify package signatures from npm registries

[![npm version](https://img.shields.io/npm/v/@pnpm/deps.security.signatures.svg)](https://npmx.dev/package/@pnpm/deps.security.signatures)

## Installation

```sh
pnpm add @pnpm/deps.security.signatures
```

## Signature Verification

`verifySignatures()` verifies ECDSA registry signatures for installed package versions. It fetches public keys from each package's registry at `/-/npm/v1/keys`, fetches full package metadata, and verifies each signature over `${name}@${version}:${integrity}`.

Registries that do not expose signing keys are skipped. Sigstore provenance attestations are not yet verified by this package; they are tracked as future scope.

## License

MIT
