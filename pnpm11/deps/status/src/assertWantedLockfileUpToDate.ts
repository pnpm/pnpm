import { parseOverrides } from '@pnpm/config.parse-overrides'
import { hashObjectNullableWithPrefix } from '@pnpm/crypto.object-hasher'
import { PnpmError } from '@pnpm/error'
import { checkPatchedDepPaths, type LockfileObject } from '@pnpm/lockfile.fs'
import {
  calcPatchHashes,
  createOverridesMapFromParsed,
  getOutdatedLockfileSetting,
  resolvePatchedDependencies,
} from '@pnpm/lockfile.settings-checker'
import { linkedPackagesAreUpToDate, satisfiesPackageManifest } from '@pnpm/lockfile.verification'
import type { WorkspacePackages } from '@pnpm/resolving.resolver-base'
import type { DependencyManifest, ProjectId, ProjectManifest } from '@pnpm/types'

import type { CheckDepsStatusOptions } from './types.js'

export interface AssertWantedLockfileUpToDateContext {
  autoInstallPeers?: boolean
  config: CheckDepsStatusOptions
  excludeLinksFromLockfile?: boolean
  injectWorkspacePackages?: boolean
  linkWorkspacePackages: boolean | 'deep'
  getManifestsByDir: () => Record<string, DependencyManifest>
  getWorkspacePackages: () => WorkspacePackages | undefined
  rootDir: string
  patchedDependencies?: Record<string, string>
}

export interface AssertWantedLockfileUpToDateOptions {
  projectDir: string
  projectId: ProjectId
  projectManifest: ProjectManifest
  wantedLockfile: LockfileObject
  wantedLockfileDir: string
}

export async function assertWantedLockfileUpToDate (
  ctx: AssertWantedLockfileUpToDateContext,
  opts: AssertWantedLockfileUpToDateOptions
): Promise<void> {
  await assertLockfileSettingsUpToDate(ctx.config, opts)
  assertPatchHashesUpToDate(opts)

  if (!satisfiesPackageManifest(
    {
      autoInstallPeers: ctx.autoInstallPeers,
      excludeLinksFromLockfile: ctx.excludeLinksFromLockfile,
      ignoredOptionalDependencies: ctx.config.ignoredOptionalDependencies,
    },
    opts.wantedLockfile.importers[opts.projectId],
    opts.projectManifest
  ).satisfies) {
    throw new PnpmError('RUN_CHECK_DEPS_UNSATISFIED_PKG_MANIFEST', `The lockfile in ${opts.wantedLockfileDir} does not satisfy project of id ${opts.projectId}`, {
      hint: 'Run `pnpm install` to update the lockfile',
    })
  }

  await assertLinkedPackagesUpToDate(ctx, opts)
}

async function assertLockfileSettingsUpToDate (
  config: CheckDepsStatusOptions,
  { wantedLockfile, wantedLockfileDir }: AssertWantedLockfileUpToDateOptions
): Promise<void> {
  const resolvedPatchedDeps = resolvePatchedDependencies(config.patchedDependencies, wantedLockfileDir)
  const [
    patchedDependencies,
    pnpmfileChecksum,
  ] = await Promise.all([
    calcPatchHashes(resolvedPatchedDeps ?? {}),
    config.hooks?.calculatePnpmfileChecksum?.(),
  ])

  const outdatedLockfileSettingName = getOutdatedLockfileSetting(wantedLockfile, {
    catalogs: config.catalogs,
    autoInstallPeers: config.autoInstallPeers,
    injectWorkspacePackages: config.injectWorkspacePackages,
    excludeLinksFromLockfile: config.excludeLinksFromLockfile,
    peersSuffixMaxLength: config.peersSuffixMaxLength,
    overrides: createOverridesMapFromParsed(parseOverrides(config.overrides ?? {}, config.catalogs)),
    ignoredOptionalDependencies: config.ignoredOptionalDependencies?.sort(),
    packageExtensionsChecksum: hashObjectNullableWithPrefix(config.packageExtensions),
    patchedDependencies,
    pnpmfileChecksum,
    ignorePnpmfileChecksum: config.ignorePnpmfile === true && pnpmfileChecksum == null,
  })

  if (outdatedLockfileSettingName) {
    throw new PnpmError('RUN_CHECK_DEPS_OUTDATED_LOCKFILE', `Setting ${outdatedLockfileSettingName} of lockfile in ${wantedLockfileDir} is outdated`, {
      hint: 'Run `pnpm install` to update the lockfile',
    })
  }
}

function assertPatchHashesUpToDate ({ wantedLockfile, wantedLockfileDir }: AssertWantedLockfileUpToDateOptions): void {
  switch (checkPatchedDepPaths(wantedLockfile)) {
    case 'stale':
      throw new PnpmError('RUN_CHECK_DEPS_STALE_PATCH_HASHES', `The lockfile in ${wantedLockfileDir} has patch hashes that disagree with its own "patchedDependencies"`, {
        hint: 'Run `pnpm install` to update the lockfile',
      })
    case 'indeterminate':
      throw new PnpmError('RUN_CHECK_DEPS_UNCHECKABLE_PATCH_HASHES', `The lockfile in ${wantedLockfileDir} cannot be checked for stale patch hashes`, {
        hint: 'Run `pnpm install` to update the lockfile',
      })
    case 'up-to-date':
      break
  }
}

async function assertLinkedPackagesUpToDate (
  ctx: AssertWantedLockfileUpToDateContext,
  { projectDir, projectId, projectManifest, wantedLockfile, wantedLockfileDir }: AssertWantedLockfileUpToDateOptions
): Promise<void> {
  if (!await linkedPackagesAreUpToDate({
    linkWorkspacePackages: !!ctx.linkWorkspacePackages,
    lockfileDir: wantedLockfileDir,
    workspaceDir: ctx.config.workspaceDir,
    manifestsByDir: ctx.getManifestsByDir(),
    workspacePackages: ctx.getWorkspacePackages(),
    lockfilePackages: wantedLockfile.packages,
  }, {
    dir: projectDir,
    manifest: projectManifest,
    snapshot: wantedLockfile.importers[projectId],
  })) {
    throw new PnpmError('RUN_CHECK_DEPS_LINKED_PKGS_OUTDATED', `The linked packages by ${projectDir} is outdated`, {
      hint: 'Run `pnpm install` to update the packages',
    })
  }
}
