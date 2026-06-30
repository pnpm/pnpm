import { getInstalledBinNames, scanGlobalPackages } from '@pnpm/global.packages'

/**
 * The set of bin names provided by global package groups *other* than those
 * in `excludeHashes`.
 *
 * Used before unlinking a group's bins (on remove / update / replace) so
 * that a bin name shared with — and owned by — another still-installed
 * group is never removed. Without this, removing one group could delete a
 * bin that actually belongs to a different global package.
 */
export async function getBinNamesOfOtherGroups (
  globalDir: string,
  excludeHashes: Set<string>
): Promise<Set<string>> {
  const others = scanGlobalPackages(globalDir).filter((pkg) => !excludeHashes.has(pkg.hash))
  const names = new Set<string>()
  await Promise.all(
    others.map(async (pkg) => {
      for (const name of await getInstalledBinNames(pkg)) {
        names.add(name)
      }
    })
  )
  return names
}
