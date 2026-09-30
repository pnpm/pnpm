import { buildSelectedPkgs } from '@pnpm/building.after-install'
import { createAllowBuildFunction } from '@pnpm/building.policy'
import * as dp from '@pnpm/deps.path'
import { PnpmError } from '@pnpm/error'
import type { PnpmContext } from '@pnpm/installing.context'
import { type Modules, readModulesManifest, writeModulesManifest } from '@pnpm/installing.modules-yaml'
import type { LockfileObject } from '@pnpm/lockfile.fs'
import { lexCompare } from '@pnpm/text.ordinal-comparator'
import type { AllowBuild, DepPath, IgnoredBuilds } from '@pnpm/types'

import type { ProcessedInstallOptions as StrictInstallOptions } from './extendInstallOptions.js'
import type { MutationRun } from './mutationTypes.js'

export class IgnoredBuildsError extends PnpmError {
  constructor (ignoredBuilds: IgnoredBuilds) {
    const packageNames = dedupePackageNamesFromIgnoredBuilds(ignoredBuilds)
    super('IGNORED_BUILDS', `Ignored build scripts: ${packageNames.join(', ')}`, {
      hint: 'Run "pnpm approve-builds" to pick which dependencies should be allowed to run scripts.',
    })
  }
}

export function dedupePackageNamesFromIgnoredBuilds (ignoredBuilds: IgnoredBuilds): string[] {
  return Array.from(new Set(Array.from(ignoredBuilds ?? []).map(depPath => dp.getPkgIdWithPatchHash(depPath)))).sort(lexCompare)
}

/**
 * Rebuilds the previously ignored builds the current policy allows, and adds
 * the builds whose approval was revoked since the previous install.
 */
export async function settleIgnoredBuilds (
  { allowBuild, ctx, opts }: Pick<MutationRun, 'allowBuild' | 'ctx' | 'opts'>,
  installIgnoredBuilds: IgnoredBuilds | undefined
): Promise<IgnoredBuilds | undefined> {
  let ignoredBuilds = installIgnoredBuilds
  if (!opts.ignoreScripts && ignoredBuilds?.size) {
    ignoredBuilds = await runUnignoredDependencyBuilds(opts, ignoredBuilds, ctx.wantedLockfile, allowBuild)
  }
  const revokedBuilds = findRevokedBuilds(ctx, allowBuild, ignoredBuilds)
  if (revokedBuilds.length === 0) return ignoredBuilds
  ignoredBuilds ??= new Set()
  for (const depPath of revokedBuilds) {
    ignoredBuilds.add(depPath)
  }
  if (!opts.lockfileOnly && opts.enableModulesDir) {
    await recordIgnoredBuilds(ctx.rootModulesDir, ignoredBuilds)
  }
  return ignoredBuilds
}

async function runUnignoredDependencyBuilds (
  opts: StrictInstallOptions,
  previousIgnoredBuilds: IgnoredBuilds,
  currentLockfile: LockfileObject,
  allowBuild?: AllowBuild
): Promise<Set<DepPath>> {
  if (!allowBuild) {
    return previousIgnoredBuilds
  }
  const pkgsToBuild: string[] = []
  for (const ignoredPkg of previousIgnoredBuilds) {
    if (currentLockfile.packages?.[ignoredPkg] == null) continue
    if (allowBuild(ignoredPkg) === true) {
      // Package is explicitly allowed - rebuild it
      pkgsToBuild.push(dp.getPkgIdWithPatchHash(ignoredPkg))
    }
  }
  if (pkgsToBuild.length) {
    return (await buildSelectedPkgs(opts.allProjects, pkgsToBuild, {
      ...opts,
      reporter: undefined, // We don't want to attach the reporter again, it was already attached.
      rootProjectManifestDir: opts.lockfileDir,
    })).ignoredBuilds ?? previousIgnoredBuilds
  }
  return previousIgnoredBuilds
}

/**
 * Detect packages whose build approval was revoked between the previous
 * and current install. A package is considered revoked when it was
 * previously allowed (true) but is now undecided (undefined). Packages
 * explicitly denied (false) are not added to ignoredBuilds, consistent
 * with how buildModules treats them.
 */
function findRevokedBuilds (
  ctx: PnpmContext,
  allowBuild: AllowBuild | undefined,
  ignoredBuilds: IgnoredBuilds | undefined
): DepPath[] {
  const previousAllowBuilds = ctx.modulesFile?.allowBuilds
  if (
    !previousAllowBuilds ||
    !ctx.wantedLockfile.packages ||
    !Object.values(previousAllowBuilds).some((allowed) => allowed === true)
  ) return []
  const oldAllowBuild = createAllowBuildFunction({ allowBuilds: previousAllowBuilds })
  if (!oldAllowBuild) return []
  return (Object.keys(ctx.wantedLockfile.packages) as DepPath[]).filter((depPath) =>
    !ignoredBuilds?.has(depPath) &&
    // The old policy is evaluated with identity trust overridden so that
    // package-name approvals count as they did when they were granted,
    // even for git/tarball artifacts that the current policy no longer
    // approves by name.
    oldAllowBuild(depPath, { trustPackageIdentity: true }) === true &&
    allowBuild?.(depPath) === undefined
  )
}

/**
 * The install path already wrote .modules.yaml with the current
 * install's state, but it captured ignoredBuilds before the revocation
 * scan added to it. Re-read the manifest from disk so we only
 * update ignoredBuilds and don't clobber fields (hoistedDependencies,
 * pendingBuilds, etc.) the install just wrote. The current computed
 * set is authoritative — runUnignoredDependencyBuilds may have removed
 * entries (for packages it successfully rebuilt) that the on-disk
 * manifest still records, and those must not be re-introduced.
 */
async function recordIgnoredBuilds (rootModulesDir: string, ignoredBuilds: IgnoredBuilds): Promise<void> {
  const writtenManifest = await readModulesManifest(rootModulesDir)
  if (writtenManifest) {
    // writeModulesManifest converts ignoredBuilds to an array before
    // serializing, so a Set is fine here.
    writtenManifest.ignoredBuilds = ignoredBuilds
    await writeModulesManifest(rootModulesDir, writtenManifest as Modules)
  }
}
