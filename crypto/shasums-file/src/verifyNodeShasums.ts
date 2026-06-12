import { PnpmError } from '@pnpm/error'
import type { FetchFromRegistry } from '@pnpm/fetching.types'
import * as openpgp from 'openpgp'

import { NODE_RELEASE_KEYS } from './nodeReleaseKeys.js'

export interface ArmoredKey { armoredKey: string }

let bundledKeyPacketsPromise: Promise<openpgp.AnyKeyPacket[]> | undefined

async function loadSigningKeyPackets (trustedKeys: readonly ArmoredKey[]): Promise<openpgp.AnyKeyPacket[]> {
  if (trustedKeys === NODE_RELEASE_KEYS) {
    bundledKeyPacketsPromise ??= readSigningKeyPackets(NODE_RELEASE_KEYS)
    return bundledKeyPacketsPromise
  }
  return readSigningKeyPackets(trustedKeys)
}

async function readSigningKeyPackets (trustedKeys: readonly ArmoredKey[]): Promise<openpgp.AnyKeyPacket[]> {
  const keys = await Promise.all(trustedKeys.map(({ armoredKey }) => openpgp.readKey({ armoredKey })))
  // A signature may be made by the primary key or any subkey; collect both.
  return keys.flatMap((key) => [key.keyPacket, ...key.subkeys.map((subkey) => subkey.keyPacket)])
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
 * The signature is verified at the packet level (the cryptographic check),
 * deliberately bypassing OpenPGP key-validity-window checks: the trusted keys
 * are pinned (mirrored from `nodejs/release-keys`), and the Node.js release keys
 * are re-certified over time, which would otherwise make signatures on older
 * releases fail to validate against the current key material.
 *
 * Throws when the signature is missing or does not verify against a trusted key.
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

  if (!(await isSignedByTrustedKey(shasumsBytes, signatureBytes, trustedKeys))) {
    throw new PnpmError(
      'NODE_SHASUMS_SIGNATURE_INVALID',
      `The OpenPGP signature of ${shasumsUrl} does not match any trusted Node.js release key. ` +
      'The downloaded Node.js runtime cannot be verified as a genuine release.'
    )
  }

  return Buffer.from(shasumsBytes).toString('utf8')
}

async function isSignedByTrustedKey (
  content: Uint8Array,
  signatureBytes: Uint8Array,
  trustedKeys: readonly ArmoredKey[]
): Promise<boolean> {
  let signature: openpgp.Signature
  let keyPackets: openpgp.AnyKeyPacket[]
  try {
    ;[signature, keyPackets] = await Promise.all([
      openpgp.readSignature({ binarySignature: signatureBytes }),
      loadSigningKeyPackets(trustedKeys),
    ])
  } catch (err: unknown) {
    throw new PnpmError('NODE_SHASUMS_SIGNATURE_INVALID', `Could not read the Node.js SHASUMS signature: ${String(err)}`)
  }
  const message = await openpgp.createMessage({ binary: content })
  const literalDataPacket = message.packets[0]

  const perSignature = await Promise.all(
    signature.packets.map((signaturePacket) =>
      signaturePacketVerifies(signaturePacket, keyPackets, literalDataPacket))
  )
  return perSignature.some(Boolean)
}

async function signaturePacketVerifies (
  signaturePacket: openpgp.SignaturePacket,
  keyPackets: openpgp.AnyKeyPacket[],
  literalDataPacket: object
): Promise<boolean> {
  const issuerKeyID = signaturePacket.issuerKeyID
  if (issuerKeyID == null) return false
  const keyPacket = keyPackets.find((packet) => packet.getKeyID().equals(issuerKeyID))
  if (keyPacket == null) return false
  try {
    // Resolves on a valid signature, rejects otherwise. This is the raw
    // cryptographic check against a pinned key — no web-of-trust / key-expiry
    // evaluation, which is what `openpgp.verify` would (incorrectly here) apply.
    await signaturePacket.verify(keyPacket, signaturePacket.signatureType!, literalDataPacket, signaturePacket.created ?? undefined, true)
    return true
  } catch {
    // Not valid under this key/packet.
    return false
  }
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
