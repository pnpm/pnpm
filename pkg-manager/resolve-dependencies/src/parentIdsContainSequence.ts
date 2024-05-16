export function parentIdsContainSequence (pkgIds: string[], pkgId1: string, pkgId2: string): boolean {
  const pkg1Index = pkgIds.indexOf(pkgId1)
  if (pkg1Index === -1 || pkg1Index === pkgIds.length - 1) {
    return false
  }
  const pkg2Index = pkgIds.lastIndexOf(pkgId2)
  return pkg1Index < pkg2Index && pkg2Index !== pkgIds.length - 1
}
