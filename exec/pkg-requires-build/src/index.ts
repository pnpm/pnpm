import { type DependencyManifest } from '@pnpm/types'

type FilesIndexArg = Map<string, unknown> | Record<string, unknown>

export function pkgRequiresBuild (manifest: Partial<DependencyManifest> | undefined, filesIndex: FilesIndexArg): boolean {
  return Boolean(
    manifest?.scripts != null && (
      Boolean(manifest.scripts.preinstall) ||
      Boolean(manifest.scripts.install) ||
      Boolean(manifest.scripts.postinstall)
    ) ||
    filesIncludeInstallScripts(filesIndex)
  )
}

function filesIncludeInstallScripts (filesIndex: FilesIndexArg): boolean {
  const keys = filesIndex instanceof Map ? filesIndex.keys() : Object.keys(filesIndex)
  for (const filename of keys) {
    if (filename === 'binding.gyp') {
      return true
    }
    if (filename.match(/^\.hooks[\\/]/) != null) {
      return true
    }
  }
  return false
}
