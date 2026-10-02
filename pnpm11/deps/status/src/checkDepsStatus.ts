import { isError } from '@pnpm/error'
import { getWantedLockfileName } from '@pnpm/lockfile.fs'
import { globalWarn } from '@pnpm/logger'
import type { Project } from '@pnpm/types'
import { findWorkspaceProjectsNoCheck } from '@pnpm/workspace.projects-reader'
import { loadWorkspaceState, WORKSPACE_STATE_SETTING_KEYS, type WorkspaceState, type WorkspaceStateSettings } from '@pnpm/workspace.state'
import { readWorkspaceManifest } from '@pnpm/workspace.workspace-manifest-reader'
import { equals } from 'ramda'

import { checkRootProjectDepsStatus } from './checkRootProjectDepsStatus.js'
import { checkWorkspaceDepsStatus } from './checkWorkspaceDepsStatus.js'
import { findLocalFileDepsIssue } from './findLocalFileDepsIssue.js'
import { outdatedResult } from './outdatedResult.js'
import type { CheckDepsStatusOptions, CheckDepsStatusResult } from './types.js'
import { getWantedLockfileDirs, scanWantedLockfiles } from './wantedLockfiles.js'

export type { CheckDepsStatusOptions, CheckDepsStatusResult } from './types.js'

export async function checkDepsStatus (opts: CheckDepsStatusOptions): Promise<CheckDepsStatusResult> {
  const workspaceState = loadWorkspaceState(opts.workspaceDir ?? opts.rootProjectManifestDir)
  if (!workspaceState) {
    if (opts.allProjects == null && opts.workspaceDir == null && opts.rootProjectManifest == null) {
      // There is no project here at all (for example, a mistyped command fell
      // back to `pnpm run` in a directory without a manifest). Reporting
      // "outdated" would make verify-deps-before-run spawn a `pnpm install`
      // that can only fail with NO_PKG_MANIFEST — report "unknown" instead so
      // the caller skips the check and the command fails with its own error.
      return { upToDate: undefined, workspaceState }
    }
    return {
      upToDate: false,
      issue: 'Cannot check whether dependencies are outdated',
      workspaceState,
    }
  }
  try {
    return await _checkDepsStatus(opts, workspaceState)
  } catch (error) {
    if (isError(error) && 'code' in error && String(error.code).startsWith('ERR_PNPM_RUN_CHECK_DEPS_')) {
      return {
        upToDate: false,
        issue: error.message,
        workspaceState,
      }
    }
    // This function never throws an error.
    // We want to ensure that pnpm CLI never crashes when checking the status of dependencies.
    // In the worst-case scenario, the install will run redundantly.
    return {
      upToDate: undefined,
      issue: isError(error) ? error.message : undefined,
      workspaceState,
    }
  }
}

async function _checkDepsStatus (opts: CheckDepsStatusOptions, workspaceState: WorkspaceState): Promise<CheckDepsStatusResult> {
  const earlyResult = checkWithoutLockfiles(opts, workspaceState)
  if (earlyResult != null) return earlyResult

  const lockfileDirs = getWantedLockfileDirs(opts)
  const wantedLockfileName = await getWantedLockfileName({
    useGitBranchLockfile: opts.useGitBranchLockfile,
    mergeGitBranchLockfiles: opts.mergeGitBranchLockfiles,
    cwd: opts.workspaceDir ?? opts.lockfileDir ?? opts.rootProjectManifestDir,
  })
  const lockfilesScan = scanWantedLockfiles(lockfileDirs, workspaceState.lastValidatedTimestamp, {
    wantedLockfileName,
    mergeGitBranchLockfiles: opts.mergeGitBranchLockfiles,
  })
  if (lockfilesScan.conflictedDir != null) {
    return outdatedResult(`The lockfile in ${lockfilesScan.conflictedDir} has merge conflicts`, workspaceState)
  }

  const { allProjects, rootProjectManifest, rootProjectManifestDir, workspaceDir } = opts
  if (allProjects && workspaceDir) {
    return checkWorkspaceDepsStatus({ opts, workspaceState, allProjects, workspaceDir, wantedLockfileName, lockfilesScan })
  }
  if (allProjects) {
    // this error shouldn't happen
    throw new Error('Impossible variant: allProjects is defined but workspaceDir is undefined')
  }
  const workspaceProjects = await findWorkspaceProjects(opts)
  if (workspaceProjects != null) {
    return checkDepsStatus({
      ...opts,
      allProjects: workspaceProjects,
    })
  }

  if (rootProjectManifest && rootProjectManifestDir) {
    return checkRootProjectDepsStatus({ opts, workspaceState, wantedLockfileName, rootProjectManifest, rootProjectManifestDir })
  }

  // `opts.allProject` being `undefined` means that the run command was not run with `--recursive`.
  // `rootProjectManifest` being `undefined` means that there's no root manifest.
  // Both means that `pnpm run` would fail, so checking lockfiles here is pointless.
  globalWarn('Skipping check.')
  return { upToDate: undefined, workspaceState }
}

/**
 * The result of the checks that do not read the lockfiles, or `undefined`
 * when those checks pass.
 */
function checkWithoutLockfiles (opts: CheckDepsStatusOptions, workspaceState: WorkspaceState): CheckDepsStatusResult | undefined {
  // This check must run before the node-linker=pnp early return below:
  // that return reports up-to-date because verify-deps-before-run cannot
  // inspect a PnP install, but for the optimistic repeat-install caller
  // (the only one setting this flag) "up-to-date" would skip the install
  // and break the local-file-deps guarantee.
  if (opts.treatLocalFileDepsAsOutdated) {
    const localFileDepsIssue = findLocalFileDepsIssue(opts)
    if (localFileDepsIssue != null) return outdatedResult(localFileDepsIssue, workspaceState)
  }

  if (opts.nodeLinker === 'pnp') {
    globalWarn('verify-deps-before-run does not work with node-linker=pnp')
    return { upToDate: true, workspaceState: undefined }
  }

  if (opts.ignoreFilteredInstallCache && workspaceState.filteredInstall) {
    return { upToDate: undefined, workspaceState }
  }

  const settingsIssue = findSettingsIssue(opts, workspaceState)
  return settingsIssue == null ? undefined : outdatedResult(settingsIssue, workspaceState)
}

function findSettingsIssue (opts: CheckDepsStatusOptions, workspaceState: WorkspaceState): string | undefined {
  if (workspaceState.settings) {
    const changedSetting = findChangedSetting(opts, workspaceState.settings)
    if (changedSetting != null) return `The value of the ${changedSetting} setting has changed`
  }
  if ((opts.configDependencies != null || workspaceState.configDependencies != null) && !equals(opts.configDependencies ?? {}, workspaceState.configDependencies ?? {})) {
    return 'Configuration dependencies are not up to date'
  }
  return undefined
}

function findChangedSetting (opts: CheckDepsStatusOptions, storedSettings: WorkspaceState['settings']): string | undefined {
  const ignoredSettings = new Set<keyof WorkspaceStateSettings>(opts.ignoredWorkspaceStateSettings)
  ignoredSettings.add('catalogs')
  for (const settingName of WORKSPACE_STATE_SETTING_KEYS) {
    if (ignoredSettings.has(settingName as keyof WorkspaceStateSettings)) continue
    const settingKey = settingName as keyof WorkspaceStateSettings
    const storedValue = normalizeUnsetSetting(settingKey, storedSettings[settingKey])
    const currentValue = normalizeUnsetSetting(settingKey, opts[settingKey])
    if (!equals(storedValue, currentValue)) return settingName
  }
  return undefined
}

/**
 * Finds the workspace projects of a non-recursive command run inside a
 * workspace, or `undefined` outside of one.
 */
async function findWorkspaceProjects (opts: CheckDepsStatusOptions): Promise<Project[] | undefined> {
  const workspaceRoot = opts.workspaceDir ?? opts.rootProjectManifestDir
  const workspaceManifest = await readWorkspaceManifest(workspaceRoot)
  if (!(workspaceManifest ?? opts.workspaceDir)) return undefined
  return findWorkspaceProjectsNoCheck(opts.rootProjectManifestDir, {
    patterns: workspaceManifest == null ? undefined : workspaceManifest.packages ?? ['.'],
    modulesDir: opts.modulesDir,
    modulesDirsByProjectName: opts.modulesDirsByProjectName,
  })
}

/**
 * Settings whose unset form means the same as a concrete value.
 *
 * The workspace state only records settings that were configured, so a
 * setting left at its default has no key in the state file. The resolved
 * config, on the other hand, may carry the value the default resolves to:
 * `@pnpm/config.reader` writes `enableGlobalVirtualStore: false` when `ci`
 * is set, and reading `allowBuilds` yields `{}`. Normalizing unset values
 * ensures an unrecorded setting matches its resolved default.
 */
const SETTING_UNSET_EQUIVALENTS: Partial<Record<keyof WorkspaceStateSettings, unknown>> = {
  allowBuilds: {},
  enableGlobalVirtualStore: false,
}

function normalizeUnsetSetting (settingName: keyof WorkspaceStateSettings, value: unknown): unknown {
  return value ?? SETTING_UNSET_EQUIVALENTS[settingName]
}
