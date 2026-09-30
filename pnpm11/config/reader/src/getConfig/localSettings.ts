import { isCamelCase } from '@pnpm/text.naming-cases'
import type { ProjectManifest } from '@pnpm/types'
import { safeReadProjectManifestOnly } from '@pnpm/workspace.project-manifest-reader'
import { readWorkspaceManifest, type WorkspaceManifest } from '@pnpm/workspace.workspace-manifest-reader'

import { quoteAndJoin } from '../quoteAndJoin.js'
import { isKnownSettingKey, quoteAndAnnotateUnknown } from '../unknownSettings.js'
import type { ConfigBuildState } from './configBuildState.js'
import { getIgnoredPnpmFieldKeys } from './ignoredPnpmFieldKeys.js'
import { getWantedPackageManager } from './packageManager.js'
import {
  isRefusedByAProjectManifest,
  PROJECT_MANIFEST_SKIPPED_KEYS,
  quoteAndExplain,
  quoteAndSuggestCamelCase,
  SCHEMA_DIRECTIVE_KEY,
  SELF_UPDATE_SKIPPED_SETTINGS,
} from './skippedKeys.js'
import { addSettingsFromWorkspaceManifestToConfig } from './workspaceManifestSettings.js'

/**
 * Reads the project's own manifests and its `pnpm-workspace.yaml` (or, for a
 * global command outside a workspace, the one in the global package
 * directory).
 *
 * Returns the registries in effect right after a workspace manifest that
 * declares `registries` was applied.
 */
export async function applyLocalSettings (
  state: ConfigBuildState,
  { forSelfUpdate }: { forSelfUpdate?: boolean }
): Promise<Record<string, string> | undefined> {
  const { cliOptions, pnpmConfig, warnings } = state
  pnpmConfig.rootProjectManifest = await safeReadProjectManifestOnly(pnpmConfig.rootProjectManifestDir) ?? undefined
  if (pnpmConfig.rootProjectManifest != null) {
    warnAboutRootProjectManifestFields(state, pnpmConfig.rootProjectManifest)
  }

  if (cliOptions['shared-workspace-lockfile'] != null && !pnpmConfig.workspaceDir && !cliOptions['global']) {
    warnings.push('The "shared-workspace-lockfile" option was ignored because no "pnpm-workspace.yaml" was found.')
  }

  await readEnginePinManifest(state)

  if (pnpmConfig.workspaceDir != null) {
    return applyProjectWorkspaceManifest(state, { forSelfUpdate, workspaceDir: pnpmConfig.workspaceDir })
  }
  if (cliOptions['global']) {
    return applyGlobalPackageDirWorkspaceManifest(state)
  }
  return undefined
}

function warnAboutRootProjectManifestFields ({ pnpmConfig, warnings }: ConfigBuildState, rootProjectManifest: ProjectManifest): void {
  if (rootProjectManifest.workspaces?.length && !pnpmConfig.workspaceDir) {
    warnings.push('The "workspaces" field in package.json is not supported by pnpm. Create a "pnpm-workspace.yaml" file instead.')
  }
  const ignoredPnpmFieldKeys = getIgnoredPnpmFieldKeys(rootProjectManifest)
  if (ignoredPnpmFieldKeys.length > 0) {
    warnings.push(`The "pnpm" field in package.json is no longer read by pnpm. The following keys were ignored: ${quoteAndJoin(ignoredPnpmFieldKeys.map(k => `pnpm.${k}`))}. See https://pnpm.io/settings for the new home of each setting.`)
  }
}

async function readEnginePinManifest ({ pnpmConfig, warnings }: ConfigBuildState): Promise<void> {
  // `lockfileDir` moves `rootProjectManifestDir` off the workspace root,
  // and the engine pins stay with the workspace the contributor works in.
  // Re-read only when the two directories differ.
  const enginePinManifestDir = pnpmConfig.workspaceDir ?? pnpmConfig.dir
  pnpmConfig.enginePinManifest = enginePinManifestDir === pnpmConfig.rootProjectManifestDir
    ? pnpmConfig.rootProjectManifest
    : await safeReadProjectManifestOnly(enginePinManifestDir) ?? undefined
  if (pnpmConfig.enginePinManifest == null) return
  const wantedPmResult = getWantedPackageManager(pnpmConfig.enginePinManifest)
  if (wantedPmResult.pm) {
    pnpmConfig.wantedPackageManager = wantedPmResult.pm
  }
  warnings.push(...wantedPmResult.warnings)
}

async function applyProjectWorkspaceManifest (
  state: ConfigBuildState,
  { forSelfUpdate, workspaceDir }: { forSelfUpdate?: boolean, workspaceDir: string }
): Promise<Record<string, string> | undefined> {
  const { cliOptions, pnpmConfig } = state
  const workspaceManifest = await readWorkspaceManifest(workspaceDir)

  pnpmConfig.workspacePackagePatterns = cliOptions['workspace-packages'] as string[] ?? workspaceManifest?.packages ?? ['.']
  if (!workspaceManifest) return undefined
  warnAboutDroppedWorkspaceManifestKeys(state.warnings, workspaceManifest)
  addSettingsFromWorkspaceManifestToConfig(pnpmConfig, {
    configFromCliOpts: state.configFromCliOpts,
    projectManifest: pnpmConfig.rootProjectManifest,
    skipSettings: forSelfUpdate
      ? new Set([...PROJECT_MANIFEST_SKIPPED_KEYS, ...SELF_UPDATE_SKIPPED_SETTINGS])
      : PROJECT_MANIFEST_SKIPPED_KEYS,
    workspaceDir,
    workspaceManifest,
  })
  return getRegistriesDeclaredByWorkspaceManifest(state, workspaceManifest)
}

/** For global installs, read settings from pnpm-workspace.yaml in the global package directory. */
async function applyGlobalPackageDirWorkspaceManifest (state: ConfigBuildState): Promise<Record<string, string> | undefined> {
  const { pnpmConfig } = state
  const workspaceManifest = await readWorkspaceManifest(pnpmConfig.globalPkgDir)
  if (!workspaceManifest) return undefined
  addSettingsFromWorkspaceManifestToConfig(pnpmConfig, {
    configFromCliOpts: state.configFromCliOpts,
    projectManifest: pnpmConfig.rootProjectManifest,
    workspaceDir: pnpmConfig.globalPkgDir,
    workspaceManifest,
  })
  return getRegistriesDeclaredByWorkspaceManifest(state, workspaceManifest)
}

function getRegistriesDeclaredByWorkspaceManifest (
  { pnpmConfig }: ConfigBuildState,
  workspaceManifest: WorkspaceManifest
): Record<string, string> | undefined {
  return workspaceManifest.registries != null
    ? pnpmConfig.registriesByScope as Record<string, string> | undefined
    : undefined
}

type WorkspaceManifestKeyRejection = 'refused' | 'unrecognized' | 'notCamelCase'

function warnAboutDroppedWorkspaceManifestKeys (warnings: string[], workspaceManifest: WorkspaceManifest): void {
  const keysByRejection: Record<WorkspaceManifestKeyRejection, string[]> = { refused: [], unrecognized: [], notCamelCase: [] }
  for (const [key, value] of Object.entries(workspaceManifest)) {
    // An unrecognized key is only reported, never dropped: this file's
    // unknown camelCase keys reach the config record, which
    // `pnpm config list` prints, and taking that away is the breaking
    // change v12 makes rather than v11.
    if (key === SCHEMA_DIRECTIVE_KEY || value == null) continue
    const rejection = getWorkspaceManifestKeyRejection(key)
    if (rejection != null) {
      keysByRejection[rejection].push(key)
    }
  }
  if (keysByRejection.refused.length > 0) {
    warnings.push(`The following settings cannot be set in a project's pnpm-workspace.yaml and were ignored: ${quoteAndExplain(keysByRejection.refused)}.`)
  }
  if (keysByRejection.unrecognized.length > 0) {
    warnings.push(`The following settings in pnpm-workspace.yaml are not recognized by this version of pnpm and were ignored: ${quoteAndAnnotateUnknown(keysByRejection.unrecognized)}.`)
  }
  if (keysByRejection.notCamelCase.length > 0) {
    warnings.push(`The following settings in pnpm-workspace.yaml were ignored because they are not written in camelCase: ${quoteAndSuggestCamelCase(keysByRejection.notCamelCase)}.`)
  }
}

function getWorkspaceManifestKeyRejection (key: string): WorkspaceManifestKeyRejection | undefined {
  if (isRefusedByAProjectManifest(key)) return 'refused'
  if (!isKnownSettingKey(key)) return 'unrecognized'
  if (!isCamelCase(key)) return 'notCamelCase'
  return undefined
}
