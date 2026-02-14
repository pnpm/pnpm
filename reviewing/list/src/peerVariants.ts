import chalk from 'chalk'

export function nameAtVersion (name: string, version: string, colorName?: (s: string) => string): string {
  if (!version) return colorName ? colorName(name) : name
  const styledName = colorName ? colorName(name) : name
  return `${styledName}${chalk.gray(`@${version}`)}`
}

export function peerHashSuffix (
  name: string,
  version: string,
  hash: string | undefined,
  multiPeerPkgs: Map<string, number>
): string {
  if (!hash) return ''
  const key = `${name}@${version}`
  const variantCount = multiPeerPkgs.get(key)
  if (variantCount == null) return ''
  return chalk.red(` peer#${hash} (${variantCount} variation${variantCount === 1 ? '' : 's'})`)
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
