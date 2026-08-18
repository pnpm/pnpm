import {
  getInstalledBinNames,
  type GlobalPackageBinSnapshot,
  type GlobalPackageInfo,
  scanGlobalPackages,
} from '@pnpm/global.packages'

/**
 * A complete ownership snapshot for the groups about to be replaced or
 * removed, together with the bins owned by every group that will survive.
 *
 * Every manifest read settles before the caller mutates global state. That
 * makes an incomplete target or survivor fail closed instead of allowing a
 * partial ownership result to drive removals.
 */
export async function getGlobalBinOwnership (
  globalDir: string,
  targetGroups: GlobalPackageInfo[]
): Promise<{ groups: GlobalPackageBinSnapshot[], protectedBins: Set<string> }> {
  const targetHashes = new Set(targetGroups.map(({ hash }) => hash))
  const survivingGroups = scanGlobalPackages(globalDir).filter((pkg) => !targetHashes.has(pkg.hash))
  const binNamesByGroup = await Promise.all(
    [...targetGroups, ...survivingGroups].map(async (pkg) => getInstalledBinNames(pkg))
  )
  const groups = targetGroups.map((info, index) => ({ info, binNames: binNamesByGroup[index] }))
  const protectedBins = new Set(binNamesByGroup.slice(targetGroups.length).flat())
  return { groups, protectedBins }
}
