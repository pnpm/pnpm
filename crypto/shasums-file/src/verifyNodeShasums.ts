import { PnpmError } from '@pnpm/error'
import type { FetchFromRegistry } from '@pnpm/fetching.types'
import * as openpgp from 'openpgp'

import { NODE_RELEASE_KEYS } from './nodeReleaseKeys.js'

export interface ArmoredKey { armoredKey: string }

let bundledKeysPromise: Promise<openpgp.Key[]> | undefined

async function loadTrustedKeys (trustedKeys: readonly ArmoredKey[]): Promise<openpgp.Key[]> {
  if (trustedKeys === NODE_RELEASE_KEYS) {
    bundledKeysPromise ??= readKeys(NODE_RELEASE_KEYS)
    return bundledKeysPromise
  }
  return readKeys(trustedKeys)
}

async function readKeys (trustedKeys: readonly ArmoredKey[]): Promise<openpgp.Key[]> {
  return Promise.all(trustedKeys.map(({ armoredKey }) => openpgp.readKey({ armoredKey })))
}

/**
 * Fetches a Node.js release's `SHASUMS256.txt` and verifies its detached
 * OpenPGP signature (`SHASUMS256.txt.sig`) against the Node.js release team's
 * embedded public keys before returning its content.
 *
 * The download mirror is repository-configurable (`node-mirror:<channel>`), so
 * the SHASUMS file — and the integrity hashes it carries — cannot be trusted on
 * their own. Verifying the signature against keys embedded in pnpm anchors the
 * download to the real Node.js release team: a mirror serving a tampered binary
 * with a matching SHASUMS cannot also produce a valid signature.
 *
 * Throws when the signature is missing or does not verify.
 */
export async function fetchVerifiedNodeShasums (
  fetch: FetchFromRegistry,
  shasumsUrl: string,
  trustedKeys: readonly ArmoredKey[] = NODE_RELEASE_KEYS
): Promise<string> {
  const [shasumsBytes, signatureBytes] = await Promise.all([
    fetchBytes(fetch, shasumsUrl, 'SHASUMS256.txt'),
    fetchBytes(fetch, `${shasumsUrl}.sig`, 'SHASUMS256.txt.sig'),
  ])

  const [message, signature, verificationKeys] = await Promise.all([
    openpgp.createMessage({ binary: shasumsBytes }),
    openpgp.readSignature({ binarySignature: signatureBytes }),
    loadTrustedKeys(trustedKeys),
  ])

  let verificationResult: Awaited<ReturnType<typeof openpgp.verify>>
  try {
    verificationResult = await openpgp.verify({ message, signature, verificationKeys })
  } catch (err: unknown) {
    throw new PnpmError(
      'NODE_SHASUMS_SIGNATURE_INVALID',
      `The OpenPGP signature of ${shasumsUrl} could not be verified against the Node.js release keys: ${String(err)}`
    )
  }

  const verified = await anySignatureVerifies(verificationResult.signatures)
  if (!verified) {
    throw new PnpmError(
      'NODE_SHASUMS_SIGNATURE_INVALID',
      `The OpenPGP signature of ${shasumsUrl} does not match any trusted Node.js release key. ` +
      'The downloaded Node.js runtime cannot be verified as a genuine release.'
    )
  }

  return Buffer.from(shasumsBytes).toString('utf8')
}

async function fetchBytes (fetch: FetchFromRegistry, url: string, what: string): Promise<Uint8Array> {
  const res = await fetch(url)
  if (!res.ok) {
    throw new PnpmError(
      'NODE_SHASUMS_FETCH_FAIL',
      `Failed to fetch ${what} (${url}) to verify the Node.js download (status: ${res.status})`
    )
  }
  return new Uint8Array(await res.arrayBuffer())
}

async function anySignatureVerifies (
  signatures: Awaited<ReturnType<typeof openpgp.verify>>['signatures']
): Promise<boolean> {
  // `signature.verified` rejects for a signature made by a key we do not trust,
  // so settle them all and accept if any resolved to `true`.
  const results = await Promise.allSettled(signatures.map((sig) => sig.verified))
  return results.some((result) => result.status === 'fulfilled' && result.value === true)
}
