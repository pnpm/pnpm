import chalk from 'chalk'

export function nameAtVersion (name: string, version: string, colorName?: (s: string) => string): string {
  if (!version) return colorName ? colorName(name) : name
  const styledName = colorName ? colorName(name) : name
  return `${styledName}${chalk.gray(`@${version}`)}`
}

export function peerHashSuffix (pkg: {
  name: string
  version: string
  peersSuffixHash?: string | undefined
}, multiPeerPkgs: Map<string, number>): string {
  if (!pkg.peersSuffixHash) return ''
  const key = `${pkg.name}@${pkg.version}`
  const variantCount = multiPeerPkgs.get(key)
  if (variantCount == null) return ''
  return chalk.red(` peer#${pkg.peersSuffixHash} (${variantCount} variation${variantCount === 1 ? '' : 's'})`)
}

export const DEDUPED_LABEL = chalk.dim(' [deduped]')

export function collectHashes (hashesPerPkg: Map<string, Set<string>>, pkg: {
  name: string
  version: string
  peersSuffixHash?: string | undefined
}): void {
  if (!pkg.peersSuffixHash) return
  const key = `${pkg.name}@${pkg.version}`
  let hashes = hashesPerPkg.get(key)
  if (hashes == null) {
    hashes = new Set()
    hashesPerPkg.set(key, hashes)
  }
  hashes.add(pkg.peersSuffixHash)
}

/**
 * Given a map of `name@version` → Set of distinct peer hashes,
 * returns only those entries with more than one variant.
 */
export function filterMultiPeerEntries (hashesPerPkg: Map<string, Set<string>>): Map<string, number> {
  const result = new Map<string, number>()
  for (const [key, hashes] of hashesPerPkg) {
    if (hashes.size > 1) {
      result.set(key, hashes.size)
    }
  }
  return result
}
