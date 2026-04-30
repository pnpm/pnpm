# @pnpm/deps.compliance.audit

> Audit a lockfile

[![npm version](https://img.shields.io/npm/v/@pnpm/deps.compliance.audit.svg)](https://npmx.dev/package/@pnpm/deps.compliance.audit)

## Installation

```sh
pnpm add @pnpm/deps.compliance.audit
```

## Signature Verification

`verifySignatures()` verifies ECDSA registry signatures for installed package versions. It fetches public keys from each package's registry at `/-/npm/v1/keys`, fetches full packuments, and verifies each signature over `${name}@${version}:${integrity}`.

Registries that do not expose signing keys are skipped. Sigstore provenance attestations are not verified by this package yet.

## License

MIT
