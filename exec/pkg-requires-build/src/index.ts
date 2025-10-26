import { type DependencyManifest } from '@pnpm/types'

export function pkgRequiresBuild (manifest: Partial<DependencyManifest> | undefined, filesIndex: Record<string, unknown>): boolean {
  return Boolean(
    manifest?.scripts != null && (
      Boolean(manifest.scripts.preinstall) ||
      Boolean(manifest.scripts.install) ||
      Boolean(manifest.scripts.postinstall)
    ) ||
    filesIncludeInstallScripts(filesIndex)
  )
}

function filesIncludeInstallScripts (filesIndex: Record<string, unknown>): boolean {
  return filesIndex['binding.gyp'] != null ||
    Object.keys(filesIndex).some((filename) => !(filename.match(/^\.hooks[\\/]/) == null)) // TODO: optimize this
}
