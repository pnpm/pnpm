import type { PnpmContext } from '@pnpm/installing.context'
import { isEmptyLockfile } from '@pnpm/lockfile.fs'
import { clone, equals } from 'ramda'

import type { ProcessedInstallOptions as StrictInstallOptions } from './extendInstallOptions.js'
import { matchReplacedLockfileEntries } from './installPredicates.js'
import { runPreResolutionHooks, startLockfileVerification, willDelegateToPacquet } from './mutateModules.js'
import type {
  MutatedProject,
  MutateModulesOptions,
  PreparedFrozenMutation,
} from './mutationTypes.js'
import { verifyLockfileResolutions } from './verifyLockfileResolutions.js'

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
  openedRun: {
    ctx: PnpmContext
    installsOnly: boolean
    opts: StrictInstallOptions
    projects: MutatedProject[]
  },
  rawPrepared: MutateModulesOptions['_preparedFrozen']
): Promise<{ verifyLockfilePromise?: Promise<void> }> {
  const { ctx, installsOnly, opts, projects } = openedRun
  const prepared = matchesPreparedFrozenLockfile(ctx, rawPrepared) ? rawPrepared : undefined
  const lockfileChangedAfterPreflight = rawPrepared != null && prepared == null
  const { verifyLockfilePromise: initialVerifyPromise } = prepared?.verifiedLockfile
    ? {}
    : await startLockfileVerification(openedRun, { skipStatShortcut: lockfileChangedAfterPreflight })
  const wantedBeforeHook = lockfileChangedAfterPreflight && opts.hooks.preResolution?.length
    ? clone(ctx.wantedLockfile)
    : undefined
  await applyPreparedOrRunPreResolutionHooks(opts, ctx, prepared)
  if (
    wantedBeforeHook == null ||
    opts.trustLockfile ||
    willDelegateToPacquet(opts, ctx, installsOnly) ||
    equals(wantedBeforeHook, ctx.wantedLockfile)
  ) {
    return { verifyLockfilePromise: initialVerifyPromise }
  }
  const chained = (initialVerifyPromise ?? Promise.resolve()).then(() =>
    verifyLockfileResolutions(ctx.wantedLockfile, opts.resolutionVerifiers, {
      isReplaced: matchReplacedLockfileEntries(projects, ctx, opts),
    })
  )
  chained.catch(() => {})
  return { verifyLockfilePromise: chained }
}
