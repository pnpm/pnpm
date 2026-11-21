import path from 'node:path'

import type { PnpmContext } from '@pnpm/installing.context'
import { getWantedLockfileName } from '@pnpm/lockfile.fs'

import type { ProcessedInstallOptions as StrictInstallOptions } from './extendInstallOptions.js'
import { matchReplacedLockfileEntries } from './installPredicates.js'
import { willDelegateToPacquet } from './mutateModules.js'
import type { MutationRun } from './mutationTypes.js'
import {
  resolveLocalPolicyForInstall,
  withContextLocalPolicyOptions,
} from './verifyLockfileLocalResolutions.js'
import {
  buildExoticSubdepsScopeFromContext,
  type LocalResolutionPolicy,
  verifyLockfileLocalResolutions,
  verifyLockfileResolutions,
} from './verifyLockfileResolutions.js'

async function resolveContextLocalPolicy (ctx: PnpmContext, opts: StrictInstallOptions): Promise<LocalResolutionPolicy> {
  const liveProjects = Object.values(ctx.projects)
  return resolveLocalPolicyForInstall(
    ctx.wantedLockfile,
    withContextLocalPolicyOptions(opts, ctx.workspacePackages, liveProjects),
    liveProjects
  )
}

/**
 * Re-validate every entry in the lockfile against the policies the
 * resolver chain was built with.
 *
 * The verification is kicked off here, right after the lockfile is loaded,
 * but not awaited inline. It would otherwise block every later install
 * stage on per-entry registry round trips. Its synchronous prologue (cache
 * lookup, lockfile hashing, candidate collection) runs now against the
 * pristine lockfile, so the async fan-out reads a stable snapshot even
 * while the install mutates `ctx.wantedLockfile` concurrently. The verdict
 * is reconciled with the install in `settleInstall`: a failure aborts the
 * install even mid-flight, and an install that finishes first is held back
 * until the verdict arrives.
 *
 * The pending verdict is returned wrapped in an object, so that awaiting
 * this function does not await the verification.
 */
export async function startLockfileVerification (
  { ctx, installsOnly, opts, projects }: Pick<MutationRun, 'ctx' | 'installsOnly' | 'opts' | 'projects'>,
  { skipStatShortcut = false, disableCache = false }: { skipStatShortcut?: boolean, disableCache?: boolean } = {}
): Promise<{ verifyLockfilePromise?: Promise<void> }> {
  const localPolicy = await resolveContextLocalPolicy(ctx, opts)
  verifyLockfileLocalResolutions(ctx.wantedLockfile, localPolicy)
  if (willDelegateToPacquet(opts, ctx, installsOnly) || opts.trustLockfile) return {}
  const cacheActive = !disableCache && !skipStatShortcut &&
    ctx.existsWantedLockfile && !ctx.wantedLockfileIsModified &&
    !opts.useGitBranchLockfile && !opts.mergeGitBranchLockfiles &&
    !ctx.lockfileHadConflicts && opts.cacheDir != null &&
    (opts.resolutionVerifiers.length > 0 || Boolean(opts.blockExoticSubdeps))
  const wantedLockfilePath = cacheActive ? path.resolve(ctx.lockfileDir, await getWantedLockfileName(opts)) : undefined
  const verifyLockfilePromise = verifyLockfileResolutions(ctx.wantedLockfile, opts.resolutionVerifiers, {
    cacheDir: disableCache ? undefined : opts.cacheDir,
    lockfilePath: wantedLockfilePath,
    isReplaced: matchReplacedLockfileEntries(projects, ctx, opts),
    blockExoticSubdeps: opts.blockExoticSubdeps,
    exoticSubdepsScope: opts.blockExoticSubdeps ? buildExoticSubdepsScopeFromContext(ctx, opts) : undefined,
    localPolicy,
  })
  // Keep the rejection from going unhandled in the window before
  // `settleInstall` awaits the verdict. A preResolution hook or the
  // install kickoff below could throw and bail out before we get there.
  verifyLockfilePromise.catch(() => {})
  return { verifyLockfilePromise }
}
