import path from 'node:path'

// `path.join(modulesDir, alias)` paired with a containment check, so a
// caller can't accidentally use the joined path without verifying that
// it lives inside `modulesDir`. Earlier passes reject path-traversal
// aliases at manifest-read time, but this layer also runs for paths
// reconstructed from lockfiles and snapshots, so the check stays here
// as a final guarantee.
export function safeJoinModulesDir (modulesDir: string, alias: string): string {
  const link = path.join(modulesDir, alias)
  const resolvedDir = path.resolve(modulesDir)
  const resolvedLink = path.resolve(link)
  if (resolvedLink === resolvedDir || !resolvedLink.startsWith(resolvedDir + path.sep)) {
    const error = new Error(`Refusing to symlink dependency outside ${modulesDir}: alias ${JSON.stringify(alias)} resolves to ${resolvedLink}`) as Error & { code: string }
    error.code = 'ERR_PNPM_INVALID_DEPENDENCY_NAME'
    throw error
  }
  return link
}
