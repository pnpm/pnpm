import path from 'node:path'

import { WANTED_LOCKFILE } from '@pnpm/constants'
import type { PnpmContext } from '@pnpm/installing.context'
import { getWantedLockfileName } from '@pnpm/lockfile.fs'

import type { ProcessedInstallOptions as StrictInstallOptions } from './extendInstallOptions.js'
import { buildExoticSubdepsScopeFromContext } from './verifyLockfileExoticSubdeps.js'
import {
  resolveLocalPolicyForInstall,
  withContextLocalPolicyOptions,
} from './verifyLockfileLocalResolutions.js'
import { snapshotVerificationPolicy, verifyLockfileResolutions } from './verifyLockfileResolutions.js'

export async function verifyLockfilePolicies (
  ctx: PnpmContext,
  opts: StrictInstallOptions,
  { wantedMutated }: { wantedMutated: boolean }
): Promise<Record<string, unknown>> {
  const canUseOnDiskCache = !wantedMutated &&
    !opts.mergeGitBranchLockfiles &&
    ctx.existsWantedLockfile &&
    !ctx.lockfileHadConflicts &&
    opts.cacheDir != null &&
    (opts.resolutionVerifiers.length > 0 || Boolean(opts.blockExoticSubdeps))
  const wantedLockfileName = canUseOnDiskCache
    ? await getWantedLockfileName({
      useGitBranchLockfile: opts.useGitBranchLockfile,
      mergeGitBranchLockfiles: opts.mergeGitBranchLockfiles,
    })
    : undefined
  const wantedLockfilePath = wantedLockfileName != null &&
    (!opts.useGitBranchLockfile || wantedLockfileName !== WANTED_LOCKFILE)
    ? path.resolve(ctx.lockfileDir, wantedLockfileName)
    : undefined
  const liveProjects = Object.values(ctx.projects)
  const localPolicy = await resolveLocalPolicyForInstall(
    ctx.wantedLockfile,
    withContextLocalPolicyOptions(opts, ctx.workspacePackages, liveProjects),
    liveProjects
  )
  const exoticSubdepsScope = opts.blockExoticSubdeps
    ? buildExoticSubdepsScopeFromContext(ctx, opts)
    : undefined
  await verifyLockfileResolutions(ctx.wantedLockfile, opts.resolutionVerifiers, {
    cacheDir: wantedMutated ? undefined : opts.cacheDir,
    lockfilePath: wantedLockfilePath,
    blockExoticSubdeps: opts.blockExoticSubdeps,
    exoticSubdepsScope,
    localPolicy,
  })
  return snapshotVerificationPolicy(opts.resolutionVerifiers, {
    blockExoticSubdeps: opts.blockExoticSubdeps,
    exoticSubdepsScope,
  })
}
