import { PackageManifest, PackageExtension, ReadPackageHook } from '@pnpm/types'
import { parseWantedDependency } from '@pnpm/parse-wanted-dependency'
import semver from 'semver'

interface PackageExtensionMatch {
  packageExtension: PackageExtension
  range: string | undefined
}

export function createPackageExtender (
  packageExtensions: Record<string, PackageExtension>
): ReadPackageHook {
  const extensionsByPkgName = {} as Record<string, PackageExtensionMatch[]>
  Object.entries(packageExtensions)
    .forEach(([selector, packageExtension]) => {
      const { alias, pref } = parseWantedDependency(selector)
      if (!extensionsByPkgName[alias!]) {
        extensionsByPkgName[alias!] = []
      }
      extensionsByPkgName[alias!].push({ packageExtension, range: pref })
    })
  return extendPkgHook.bind(null, extensionsByPkgName) as ReadPackageHook
}

function extendPkgHook (extensionsByPkgName: Record<string, PackageExtensionMatch[]>, manifest: PackageManifest) {
  const extensions = extensionsByPkgName[manifest.name]
  if (extensions == null) return manifest
  extendPkg(manifest, extensions)
  return manifest
}

function extendPkg (manifest: PackageManifest, extensions: PackageExtensionMatch[]) {
  for (const { range, packageExtension } of extensions) {
    if (range != null && !semver.satisfies(manifest.version, range)) continue
    for (const field of ['dependencies', 'optionalDependencies', 'peerDependencies', 'peerDependenciesMeta']) {
      if (!packageExtension[field]) continue
      manifest[field] = {
        ...packageExtension[field],
        ...manifest[field],
      }
    }
  }
}
