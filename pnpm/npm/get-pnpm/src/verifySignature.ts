import { createVerify } from 'node:crypto'

import { NPM_SIGNING_KEYS } from './npmSigningKeys.js'

export interface SigningKey {
  keyid: string
  key: string
  expires: string | null
}

export interface PackageSignature {
  keyid: string
  sig: string
}

/**
 * Checks the registry's signature over a package's identity and checksum.
 *
 * The registry serves both the tarball and the checksum, so a checksum taken
 * from it proves nothing on its own. This is what makes it worth anything: the
 * signature is made with a key the registry publishes but the download host
 * cannot mint, and the trusted copy of that key ships inside this package.
 *
 * @throws if the package is unsigned, signed with a key that isn't trusted or
 * has expired, or the signature does not verify.
 */
export function verifyRegistrySignature (
  opts: {
    name: string
    version: string
    integrity: string
    signatures?: PackageSignature[]
    keys?: readonly SigningKey[]
    now?: Date
  }
): void {
  const pkg = `${opts.name}@${opts.version}`
  const signatures = opts.signatures ?? []
  if (signatures.length === 0) {
    throw new Error(`${pkg} carries no npm registry signature, so it cannot be verified.`)
  }

  // Pick the signature by key rather than by position. A package can carry
  // several, in no guaranteed order, and across a rotation one of them can be
  // from a key this installer does not pin — which is not a reason to refuse a
  // package that another, pinned key also signed.
  const keys = opts.keys ?? NPM_SIGNING_KEYS
  const match = signatures
    .map((signature) => ({ signature, key: keys.find(({ keyid }) => keyid === signature.keyid) }))
    .find((candidate) => candidate.key != null)
  if (match?.key == null) {
    throw new Error(`${pkg} is signed with an unexpected npm key (${signatures.map(({ keyid }) => keyid).join(', ')}).

If npm has rotated its signing key, this installer needs updating.
Until then, install pnpm another way: https://pnpm.io/installation`)
  }
  const { signature, key } = match

  if (key.expires != null) {
    const expires = new Date(key.expires).getTime()
    // An unreadable date must not read as "never expires".
    if (Number.isNaN(expires)) {
      throw new Error(`${pkg} is signed with an npm key whose expiry date cannot be read (${key.expires}).`)
    }
    if (expires < (opts.now ?? new Date()).getTime()) {
      throw new Error(`${pkg} is signed with an npm key that expired on ${key.expires}.`)
    }
  }

  // Registry signatures cover the package identity and its content hash.
  const message = `${pkg}:${opts.integrity}`
  const publicKey = `-----BEGIN PUBLIC KEY-----\n${key.key}\n-----END PUBLIC KEY-----`
  if (!createVerify('SHA256').update(message).verify(publicKey, signature.sig, 'base64')) {
    throw new Error(`The npm registry signature for ${pkg} is not valid. Refusing to install.`)
  }
}
