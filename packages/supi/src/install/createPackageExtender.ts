import { PackageManifest, PackageExtension, ReadPackageHook } from '@pnpm/types'
import parseWantedDependency from '@pnpm/parse-wanted-dependency'
import { isSubRange } from './createVersionsOverrider'

interface PackageExtensionMatch {
  packageExtension: PackageExtension
  pref: string | undefined
}

export default function (
  packageExtensions: Record<string, PackageExtension>
): ReadPackageHook {
  const extensionsByPkgName = {} as Record<string, PackageExtensionMatch[]>
  Object.entries(packageExtensions)
    .forEach(([selector, packageExtension]) => {
      const { alias, pref } = parseWantedDependency(selector)
      if (!extensionsByPkgName[alias!]) {
        extensionsByPkgName[alias!] = []
      }
      extensionsByPkgName[alias!].push({ packageExtension, pref })
    })
  return ((manifest: PackageManifest) => {
    const extensions = extensionsByPkgName[manifest.name]
    if (extensions == null) return manifest
    extendPkg(manifest, extensions)
    return manifest
  }) as ReadPackageHook
}

function extendPkg (manifest: PackageManifest, extensions: PackageExtensionMatch[]) {
  for (const extension of extensions) {
    if (extension.pref != null && !isSubRange(extension.pref, manifest.version)) continue
    for (const field of ['dependencies', 'optionalDependencies', 'peerDependencies', 'peerDependenciesMeta']) {
      if (!extension.packageExtension[field]) continue
      manifest[field] = {
        ...extension.packageExtension[field],
        ...manifest[field],
      }
    }
  }
}
