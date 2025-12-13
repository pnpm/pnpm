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
  if (filesIndex.has('binding.gyp')) {
    return true
  }
  for (const filename of filesIndex.keys()) {
    if (filename.match(/^\.hooks[\\/]/) != null) {
      return true
    }
  }
  return false
}
