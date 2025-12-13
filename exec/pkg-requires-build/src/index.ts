import { type DependencyManifest } from '@pnpm/types'

export function pkgRequiresBuild (manifest: Partial<DependencyManifest> | undefined, filesIndex: Map<string, unknown>): boolean {
  return Boolean(
    manifest?.scripts != null && (
      Boolean(manifest.scripts.preinstall) ||
      Boolean(manifest.scripts.install) ||
      Boolean(manifest.scripts.postinstall)
    ) ||
    filesIncludeInstallScripts(filesIndex)
  )
}

function filesIncludeInstallScripts (filesIndex: Map<string, unknown>): boolean {
  return filesIndex.has('binding.gyp') ||
    Array.from(filesIndex.keys()).some((filename) => !(filename.match(/^\.hooks[\\/]/) == null)) // TODO: optimize this
}
