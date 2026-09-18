import {
  getInstalledBinNames,
  type GlobalPackageBinSnapshot,
  type GlobalPackageInfo,
  scanGlobalPackages,
} from '@pnpm/global.packages'

/**
 * A complete ownership snapshot for the groups about to be replaced or
 * removed, together with the at-risk bins owned by groups that will survive.
 *
 * Every manifest read settles before the caller mutates global state, so a
 * target whose manifests cannot be read fails closed. A dependency whose
 * directory is gone is not that case: it owns no bins, and reading it that way
 * is what lets a group with a deleted `node_modules` be replaced at all.
 * Survivors only need inspecting when a target bin will not be retained,
 * because no other bin can be removed.
 */
export async function getGlobalBinOwnership (
  globalDir: string,
  targetGroups: GlobalPackageInfo[],
  retainedBinNames: Set<string>
): Promise<{ groups: GlobalPackageBinSnapshot[], protectedBins: Set<string> }> {
  const targetHashes = new Set(targetGroups.map(({ hash }) => hash))
  const targetBinNames = await Promise.all(targetGroups.map((pkg) => getInstalledBinNames(pkg)))
  const groups = targetGroups.map((info, index) => ({ info, binNames: targetBinNames[index] }))
  const binNamesToProtect = new Set(targetBinNames.flat().filter((name) => !retainedBinNames.has(name)))
  if (binNamesToProtect.size === 0) return { groups, protectedBins: new Set() }

  const survivingGroups = scanGlobalPackages(globalDir).filter((pkg) => !targetHashes.has(pkg.hash))
  const survivorBinNames = await Promise.all(survivingGroups.map((pkg) => getInstalledBinNames(pkg)))
  const protectedBins = new Set(
    survivorBinNames.flat().filter((name) => binNamesToProtect.has(name))
  )
  return { groups, protectedBins }
}
