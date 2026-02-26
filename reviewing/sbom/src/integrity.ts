import ssri from 'ssri'

export interface HashDigest {
  algorithm: string
  digest: string
}

/**
 * Convert an SRI integrity string to a list of algorithm + hex digest pairs.
 * e.g. "sha512-abc123..." → [{ algorithm: "SHA-512", digest: "..." }]
 */
export function integrityToHashes (integrity: string | undefined): HashDigest[] {
  if (!integrity) return []

  const parsed = ssri.parse(integrity)
  const hashes: HashDigest[] = []

  for (const [algo, entries] of Object.entries(parsed)) {
    if (!entries?.length) continue
    for (const entry of entries) {
      const hexDigest = Buffer.from(entry.digest, 'base64').toString('hex')
      hashes.push({
        algorithm: normalizeShaAlgorithm(algo),
        digest: hexDigest,
      })
    }
  }

  return hashes
}

function normalizeShaAlgorithm (algo: string): string {
  switch (algo) {
  case 'sha1':
    return 'SHA-1'
  case 'sha256':
    return 'SHA-256'
  case 'sha384':
    return 'SHA-384'
  case 'sha512':
    return 'SHA-512'
  default:
    return algo.toUpperCase()
  }
}
