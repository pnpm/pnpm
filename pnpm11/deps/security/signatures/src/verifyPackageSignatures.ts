import crypto from 'node:crypto'

import type { PackageSignature, RegistryKey, SignaturePackage } from './packument.js'

export interface SignatureIssue extends SignaturePackage {
  integrity?: string
  reason?: string
  resolved?: string
}

export function verifyPackageSignatures (
  pkg: SignaturePackage & {
    integrity: string
    publishedAt?: string
    resolved?: string
    signatures: PackageSignature[]
  },
  keys: RegistryKey[]
): SignatureIssue | undefined {
  const message = `${pkg.name}@${pkg.version}:${pkg.integrity}`
  const publishedTime = pkg.publishedAt ? Date.parse(pkg.publishedAt) : undefined

  const failures: string[] = []
  for (const signature of pkg.signatures) {
    const keyFailure = checkKeyFailure(pkg, signature, keys, publishedTime)
    if (keyFailure != null) {
      failures.push(keyFailure)
      continue
    }

    const key = keys.find(({ keyid }) => keyid === signature.keyid)!
    if (isValidSignature(message, signature.sig, key.key)) {
      return undefined
    }
    failures.push(`${pkg.name}@${pkg.version} has an invalid registry signature with keyid ${signature.keyid}`)
  }
  return toSignatureIssue(pkg, pickMostTellingFailure(pkg, failures))
}

function checkKeyFailure (
  pkg: SignaturePackage,
  signature: PackageSignature,
  keys: RegistryKey[],
  publishedTime?: number
): string | undefined {
  const key = keys.find(({ keyid }) => keyid === signature.keyid)
  if (!key) {
    return `${pkg.name}@${pkg.version} has a registry signature with keyid ${signature.keyid} but no corresponding public key can be found`
  }
  if (key.expires && publishedTime != null && publishedTime >= Date.parse(key.expires)) {
    return `${pkg.name}@${pkg.version} has a registry signature with keyid ${signature.keyid} but the corresponding public key has expired ${key.expires}`
  }
  return undefined
}

function isValidSignature (message: string, sig: string, keyString: string): boolean {
  const pem = `-----BEGIN PUBLIC KEY-----\n${keyString}\n-----END PUBLIC KEY-----`
  try {
    const verifier = crypto.createVerify('SHA256')
    verifier.write(message)
    verifier.end()
    return verifier.verify(pem, sig, 'base64')
  } catch {
    return false
  }
}

function pickMostTellingFailure (
  pkg: SignaturePackage,
  failures: string[]
): string {
  if (failures.length === 0) {
    return `${pkg.name}@${pkg.version} has no registry signature from a trusted key`
  }
  return failures.find((reason) => reason.includes('invalid registry signature')) ?? failures[0]
}

function toSignatureIssue (
  pkg: SignaturePackage & { integrity?: string, resolved?: string },
  reason: string
): SignatureIssue {
  return {
    integrity: pkg.integrity,
    name: pkg.name,
    reason,
    registry: pkg.registry,
    resolved: pkg.resolved,
    version: pkg.version,
  }
}
