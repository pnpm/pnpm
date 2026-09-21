import fs from 'node:fs'
import path from 'node:path'
import util from 'node:util'

import validateNpmPackageName from 'validate-npm-package-name'

// Joins `modulesDir` with a dependency alias and guarantees the result
// stays a direct child of `modulesDir`. The alias becomes a directory
// name inside `node_modules`, so it must be a valid npm package name: a
// single `name` or `@scope/name` of URL-friendly characters with no
// leading `.` or `_`, and not a reserved name. That rejects
// path-traversal (`../x`), absolute, and pnpm-owned aliases (`.bin`,
// `.pnpm`, `node_modules`) before they can escape `modulesDir` or
// overwrite pnpm's own layout. The containment check is kept as a
// belt-and-suspenders guard for any platform-specific join behavior the
// name check might not anticipate.
//
// Earlier passes reject such aliases at manifest-read and resolution
// time, but this layer also runs for paths reconstructed from lockfiles
// and snapshots, so the check stays here as a final guarantee.
export function safeJoinModulesDir (modulesDir: string, alias: string): string {
  if (!validateNpmPackageName(alias).validForOldPackages) {
    throw invalidDependencyNameError(modulesDir, alias)
  }
  const link = path.join(modulesDir, alias)
  const resolvedDir = path.resolve(modulesDir)
  const resolvedLink = path.resolve(link)
  if (resolvedLink === resolvedDir || !resolvedLink.startsWith(resolvedDir + path.sep)) {
    throw invalidDependencyNameError(modulesDir, alias, resolvedLink)
  }
  return link
}

export function safeJoinWorkspaceModulesDir (modulesDir: string, alias: string): string {
  const components = alias.split('/')
  const firstComponent = components[0].toLowerCase()
  if (
    alias.length === 0 ||
    alias.startsWith('/') ||
    alias.includes('\\') ||
    alias[1] === ':' ||
    ['.bin', '.pnpm', 'node_modules'].includes(firstComponent) ||
    components.some(component => component.length === 0 || component === '.' || component === '..')
  ) {
    throw invalidDependencyNameError(modulesDir, alias)
  }
  const link = path.join(modulesDir, ...components)
  const resolvedDir = path.resolve(modulesDir)
  const resolvedLink = path.resolve(link)
  if (resolvedLink === resolvedDir || !resolvedLink.startsWith(resolvedDir + path.sep)) {
    throw invalidDependencyNameError(modulesDir, alias, resolvedLink)
  }
  return link
}

export async function prepareWorkspaceModulesDir (modulesDir: string, alias: string): Promise<string> {
  const destination = safeJoinWorkspaceModulesDir(modulesDir, alias)
  await fs.promises.mkdir(modulesDir, { recursive: true })
  const parentComponents = path.relative(modulesDir, path.dirname(destination)).split(path.sep).filter(Boolean)
  await prepareWorkspaceParent({ alias, components: parentComponents, current: modulesDir, modulesDir })
  return destination
}

async function prepareWorkspaceParent (opts: {
  alias: string
  components: string[]
  current: string
  modulesDir: string
}): Promise<void> {
  const [component, ...remainingComponents] = opts.components
  if (component == null) return
  const current = path.join(opts.current, component)
  const stat = await lstatOrCreateDirectory(current)
  if (stat.isSymbolicLink() || !stat.isDirectory()) {
    throw invalidDependencyNameError(opts.modulesDir, opts.alias, current)
  }
  await prepareWorkspaceParent({ ...opts, components: remainingComponents, current })
}

async function lstatOrCreateDirectory (dir: string): Promise<fs.Stats> {
  try {
    return await fs.promises.lstat(dir)
  } catch (error: unknown) {
    if (!util.types.isNativeError(error) || !('code' in error) || error.code !== 'ENOENT') throw error
  }
  try {
    await fs.promises.mkdir(dir)
  } catch (error: unknown) {
    if (!util.types.isNativeError(error) || !('code' in error) || error.code !== 'EEXIST') throw error
  }
  return fs.promises.lstat(dir)
}

function invalidDependencyNameError (modulesDir: string, alias: string, resolvedLink?: string): Error & { code: string } {
  const detail = resolvedLink ? ` (it resolves to ${resolvedLink})` : ''
  const error = new Error(`Refusing to place a dependency under ${modulesDir} with the invalid alias ${JSON.stringify(alias)}${detail}`) as Error & { code: string }
  error.code = 'ERR_PNPM_INVALID_DEPENDENCY_NAME'
  return error
}
