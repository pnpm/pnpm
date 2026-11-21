
import type { PnpmContext } from '@pnpm/installing.context'
import { isEmptyLockfile } from '@pnpm/lockfile.fs'
import { clone, equals } from 'ramda'

import type { ProcessedInstallOptions as StrictInstallOptions } from './extendInstallOptions.js'
import { runPreResolutionHooks } from './mutateModules.js'
import type {
  MutatedProject,
  MutateModulesOptions,
  PreparedFrozenMutation,
} from './mutationTypes.js'
import { startLockfileVerification } from './startLockfileVerification.js'
import { buildExoticSubdepsScopeFromContext } from './verifyLockfileExoticSubdeps.js'
import { canTrustPreparedPolicy } from './verifyLockfileResolutions.js'

interface OpenedPreparedRun {
  ctx: PnpmContext
  installsOnly: boolean
  opts: StrictInstallOptions
  projects: MutatedProject[]
}

export function assertProjectManifestsExist (ctx: PnpmContext, ignorePackageManifest: boolean): void {
  if (ignorePackageManifest) return
  for (const { manifest, rootDir } of Object.values(ctx.projects)) {
    if (!manifest) {
      throw new Error(`No package.json found in "${rootDir}"`)
    }
  }
}

export function applyPreparedFrozenProjects (
  opts: StrictInstallOptions,
  prepared: MutateModulesOptions['_preparedFrozen']
): void {
  if (prepared == null) return
  for (const project of opts.allProjects) {
    const hooked = prepared.hookedProjects.get(project.rootDir)
    if (hooked != null) {
      project.manifest = hooked.manifest
      project.originalManifest = hooked.originalManifest
      project.alreadyHooked = true
    }
  }
}

export function matchesPreparedFrozenLockfile (
  ctx: PnpmContext,
  prepared: MutateModulesOptions['_preparedFrozen']
): prepared is PreparedFrozenMutation {
  if (prepared == null) return false
  if (ctx.existsWantedLockfile !== prepared.existsWantedLockfile) return false
  if (ctx.lockfileHadConflicts !== prepared.lockfileHadConflicts) return false
  if (!prepared.existsWantedLockfile) return true
  return prepared.validatedInputLockfile != null && equals(ctx.wantedLockfile, prepared.validatedInputLockfile)
}

export async function applyPreparedOrRunPreResolutionHooks (
  opts: StrictInstallOptions,
  ctx: PnpmContext,
  prepared: MutateModulesOptions['_preparedFrozen']
): Promise<void> {
  if (!prepared?.preResolutionRan) {
    await runPreResolutionHooks(opts, ctx)
    return
  }
  if (prepared.hookedWantedLockfile != null) {
    ctx.wantedLockfile = prepared.hookedWantedLockfile
  }
  if (prepared.hookedCurrentLockfile != null) {
    ctx.currentLockfile = prepared.hookedCurrentLockfile
  }
  if (prepared.hookedWantedLockfile != null || prepared.hookedCurrentLockfile != null) {
    ctx.currentLockfileIsUpToDate = equals(ctx.currentLockfile, ctx.wantedLockfile)
    ctx.existsNonEmptyWantedLockfile = ctx.existsWantedLockfile && !isEmptyLockfile(ctx.wantedLockfile)
  }
}

export async function settlePreparedMutationHooksAndVerification (
  openedRun: OpenedPreparedRun,
  rawPrepared: MutateModulesOptions['_preparedFrozen']
): Promise<{ verifyLockfilePromise?: Promise<void> }> {
  const { ctx, opts } = openedRun
  const prepared = matchesPreparedFrozenLockfile(ctx, rawPrepared) ? rawPrepared : undefined
  const preparedCoversCurrentPolicy = preparedPolicyIsTrusted(ctx, opts, prepared)
  if (prepared?.preResolutionRan) {
    return settleRestoredPreResolution(openedRun, prepared, preparedCoversCurrentPolicy)
  }
  const lockfileChangedAfterPreflight = rawPrepared != null && prepared == null
  const hasPreResolutionHooks = Boolean(opts.hooks.preResolution?.length)
  const wantedBeforeHook = hasPreResolutionHooks ? clone(ctx.wantedLockfile) : undefined
  await applyPreparedOrRunPreResolutionHooks(opts, ctx, prepared)
  const wantedMutated = wantedBeforeHook != null && !equals(wantedBeforeHook, ctx.wantedLockfile)
  if (wantedMutated) ctx.wantedLockfileIsModified = true
  if (preparedCoversCurrentPolicy && !wantedMutated) return {}
  return startLockfileVerification(openedRun, {
    skipStatShortcut: lockfileChangedAfterPreflight || hasPreResolutionHooks || wantedMutated,
    disableCache: wantedMutated,
  })
}

function preparedPolicyIsTrusted (
  ctx: PnpmContext,
  opts: StrictInstallOptions,
  prepared: PreparedFrozenMutation | undefined
): boolean {
  if (!prepared?.verifiedLockfile) return false
  if (prepared.verifiedPolicySnapshot == null) return true
  const blockExoticSubdeps = !opts.trustLockfile && Boolean(opts.blockExoticSubdeps)
  return canTrustPreparedPolicy(prepared.verifiedPolicySnapshot, opts.resolutionVerifiers, {
    blockExoticSubdeps,
    exoticSubdepsScope: blockExoticSubdeps ? buildExoticSubdepsScopeFromContext(ctx, opts) : undefined,
  })
}

async function settleRestoredPreResolution (
  openedRun: OpenedPreparedRun,
  prepared: PreparedFrozenMutation,
  preparedCoversCurrentPolicy: boolean
): Promise<{ verifyLockfilePromise?: Promise<void> }> {
  const { ctx, opts } = openedRun
  const wantedMutated = prepared.hookedWantedLockfile != null &&
    !equals(ctx.wantedLockfile, prepared.hookedWantedLockfile)
  await applyPreparedOrRunPreResolutionHooks(opts, ctx, prepared)
  if (wantedMutated) ctx.wantedLockfileIsModified = true
  if (preparedCoversCurrentPolicy) return {}
  return startLockfileVerification(openedRun, {
    skipStatShortcut: true,
    disableCache: wantedMutated,
  })
}
