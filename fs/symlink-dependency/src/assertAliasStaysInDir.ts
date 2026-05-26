import path from 'node:path'

// Defense-in-depth: refuse to symlink a dependency at a location that
// `path.join` lifts outside `destModulesDir`. Earlier passes reject
// path-traversal aliases at manifest-read time, but this layer also
// runs for paths reconstructed from lockfiles and snapshots, so we
// re-check before touching the filesystem.
export function assertAliasStaysInDir (destModulesDir: string, importAs: string): void {
  const resolvedDest = path.resolve(destModulesDir)
  const resolvedLink = path.resolve(destModulesDir, importAs)
  if (resolvedLink === resolvedDest || !resolvedLink.startsWith(resolvedDest + path.sep)) {
    const error = new Error(`Refusing to symlink dependency outside ${destModulesDir}: alias ${JSON.stringify(importAs)} resolves to ${resolvedLink}`) as Error & { code: string }
    error.code = 'ERR_PNPM_INVALID_DEPENDENCY_NAME'
    throw error
  }
}
