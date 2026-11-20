import path from 'node:path'

import { allowBuildKeyFromIgnoredBuild, createAllowBuildFunction, isBuildExplicitlyDisallowed } from '@pnpm/building.policy'
import * as dp from '@pnpm/deps.path'
import { resolveLocalParam } from '@pnpm/global.commands'
import { type Modules, readModulesManifest, writeModulesManifest } from '@pnpm/installing.modules-yaml'
import { readWantedLockfile } from '@pnpm/lockfile.fs'
import { isLocalFilesystemSpecifier } from '@pnpm/resolving.local-resolver'
import { parseWantedDependency } from '@pnpm/resolving.parse-wanted-dependency'
import { lexCompare } from '@pnpm/text.ordinal-comparator'
import type { IgnoredBuilds, ProjectId } from '@pnpm/types'

export { isLocalFilesystemSpecifier }

export function resolveLocalPackageSpec (spec: string, baseDir: string): string {
  const { alias, bareSpecifier } = parseWantedDependency(spec)
  if (alias != null && bareSpecifier != null) {
    const resolved = resolveLocalParam(bareSpecifier, baseDir)
    return resolved === bareSpecifier ? spec : `${alias}@${resolved}`
  }
  return resolveLocalParam(spec, baseDir)
}

/**
 * Approves explicitly requested top-level `dlx` packages whose non-semver
 * specifiers (`http(s):`, `file:`, `git+`, ...) require exact `depPath`
 * approval, updating `allowBuilds` and clearing their `ignoredBuilds` entries
 * in `.modules.yaml`. Returns the keys that must now be built.
 */
export async function approveDirectNonRegistryBuilds (opts: {
  dir: string
  allowBuilds: Record<string, boolean | string>
}): Promise<string[]> {
  const modulesDir = path.join(opts.dir, 'node_modules')
  const modulesManifest = await readModulesManifest(modulesDir)
  if (!modulesManifest?.ignoredBuilds?.size) return []
  const lockfile = await readWantedLockfile(opts.dir, { ignoreIncompatible: false })
  const directDeps = lockfile?.importers['.' as ProjectId]?.dependencies
  if (!directDeps) return []
  const approvedKeys = collectApprovedDirectBuildKeys(directDeps, modulesManifest.ignoredBuilds, opts.allowBuilds)
  if (approvedKeys.size === 0) return []
  const sortedKeys = Array.from(approvedKeys).sort(lexCompare)
  for (const key of sortedKeys) {
    opts.allowBuilds[key] = true
  }
  pruneApprovedIgnoredBuilds(modulesManifest, approvedKeys)
  await writeModulesManifest(modulesDir, modulesManifest as Modules)
  return sortedKeys
}

function collectApprovedDirectBuildKeys (
  directDeps: Record<string, string>,
  ignoredBuilds: IgnoredBuilds,
  allowBuilds: Record<string, boolean | string>
): Set<string> {
  const allowBuild = createAllowBuildFunction({ allowBuilds })
  const approvedKeys = new Set<string>()
  for (const [alias, version] of Object.entries(directDeps)) {
    if (allowBuilds[alias] === false) continue
    const depPath = dp.refToRelative(version, alias)
    if (depPath == null || isBuildExplicitlyDisallowed(depPath, allowBuild) || !ignoredBuilds.has(depPath)) {
      continue
    }
    const allowKey = allowBuildKeyFromIgnoredBuild(depPath)
    if (allowKey === dp.getPkgIdWithPatchHash(depPath)) {
      approvedKeys.add(allowKey)
    }
  }
  return approvedKeys
}

function pruneApprovedIgnoredBuilds (modulesManifest: Modules, approvedKeys: Set<string>): void {
  if (!modulesManifest.ignoredBuilds) return
  for (const depPath of modulesManifest.ignoredBuilds) {
    if (approvedKeys.has(allowBuildKeyFromIgnoredBuild(depPath))) {
      modulesManifest.ignoredBuilds.delete(depPath)
    }
  }
  if (modulesManifest.ignoredBuilds.size === 0) {
    delete modulesManifest.ignoredBuilds
  }
}
