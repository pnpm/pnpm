import { type PackageManifest, type PackageExtension, type ReadPackageHook } from '@pnpm/types'
import { parseWantedDependency } from '@pnpm/parse-wanted-dependency'
import semver from 'semver'

interface PackageExtensionMatch {
  packageExtension: PackageExtension
  range: string | undefined
}

type ExtensionsByPkgName = Map<string, PackageExtensionMatch[]>

export function createPackageExtender (
  packageExtensions: Record<string, PackageExtension>
): ReadPackageHook {
  const extensionsByPkgName: ExtensionsByPkgName = new Map()
  Object.entries(packageExtensions)
    .forEach(([selector, packageExtension]) => {
      const { alias, pref } = parseWantedDependency(selector)
      if (!extensionsByPkgName.has(alias!)) {
        extensionsByPkgName.set(alias!, [])
      }
      extensionsByPkgName.get(alias!)!.push({ packageExtension, range: pref })
    })
  return extendPkgHook.bind(null, extensionsByPkgName) as ReadPackageHook
}

function extendPkgHook (extensionsByPkgName: ExtensionsByPkgName, manifest: PackageManifest): PackageManifest {
  const extensions = extensionsByPkgName.get(manifest.name)
  if (extensions == null) return manifest
  extendPkg(manifest, extensions)
  return manifest
}

function extendPkg (manifest: PackageManifest, extensions: PackageExtensionMatch[]): void {
  for (const { range, packageExtension } of extensions) {
    if (range != null && !semver.satisfies(manifest.version, range)) continue
    for (const field of ['dependencies', 'optionalDependencies', 'peerDependencies', 'peerDependenciesMeta'] as const) {
      if (!packageExtension[field]) continue
      manifest[field] = {
        ...packageExtension[field],
        ...manifest[field],
      } as any // eslint-disable-line @typescript-eslint/no-explicit-any
    }
  }
}
