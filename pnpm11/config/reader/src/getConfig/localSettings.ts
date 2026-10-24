import { isCamelCase } from '@pnpm/text.naming-cases'
import type { ProjectManifest } from '@pnpm/types'
import { safeReadProjectManifestOnly } from '@pnpm/workspace.project-manifest-reader'
import { readWorkspaceManifest, readWorkspaceManifestKeys, type WorkspaceManifest } from '@pnpm/workspace.workspace-manifest-reader'

import { quoteAndJoin } from '../quoteAndJoin.js'
import { isKnownSettingKey, quoteAndAnnotateUnknown } from '../unknownSettings.js'
import type { ConfigBuildState } from './configBuildState.js'
import { getIgnoredPnpmFieldKeys } from './ignoredPnpmFieldKeys.js'
import { getWantedPackageManager } from './packageManager.js'
import {
  isRefusedByAProjectManifest,
  PIN_DECISION_SETTINGS,
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
  { forSelfUpdate, skipUnreadableSettings }: { forSelfUpdate?: boolean, skipUnreadableSettings?: boolean }
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
    return applyProjectWorkspaceManifestUnlessUnreadable(state, { forSelfUpdate, skipUnreadableSettings, workspaceDir: pnpmConfig.workspaceDir })
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

async function applyProjectWorkspaceManifestUnlessUnreadable (
  state: ConfigBuildState,
  opts: { forSelfUpdate?: boolean, skipUnreadableSettings?: boolean, workspaceDir: string }
): Promise<Record<string, string> | undefined> {
  if (!opts.skipUnreadableSettings) return applyProjectWorkspaceManifest(state, opts)
  // A setting outside the pin decision cannot steer it, so one that fails
  // must not drop the `pmOnFail` or `lockfileDir` beside it. Neither may an
  // unreadable `packages`.
  try {
    const fullManifest = await readWorkspaceManifest(opts.workspaceDir)
    return applyAllOrNothing(state, () => applyLoadedProjectWorkspaceManifest(state, fullManifest, opts))
  } catch {}
  try {
    const pinManifest = await readWorkspaceManifestKeys(opts.workspaceDir, PIN_DECISION_SETTINGS)
    return applyReadablePinSettings(state, pinManifest, opts)
  } catch {
    state.pnpmConfig.workspacePackagePatterns = ['.']
    return undefined
  }
}

function applyReadablePinSettings (
  state: ConfigBuildState,
  pinManifest: WorkspaceManifest | undefined,
  opts: { forSelfUpdate?: boolean, workspaceDir: string }
): Record<string, string> | undefined {
  try {
    return applyAllOrNothing(state, () => applyLoadedProjectWorkspaceManifest(state, pinManifest, opts))
  } catch {}
  const readableManifest = pinManifest && pickReadableWorkspaceEntries(state, pinManifest, opts)
  if (readableManifest != null) {
    try {
      return applyAllOrNothing(state, () => applyLoadedProjectWorkspaceManifest(state, readableManifest, opts))
    } catch {}
  }
  state.pnpmConfig.workspacePackagePatterns = ['.']
  return undefined
}

function pickReadableWorkspaceEntries (
  state: ConfigBuildState,
  manifest: WorkspaceManifest,
  opts: { forSelfUpdate?: boolean, workspaceDir: string }
): WorkspaceManifest | undefined {
  const entries = Object.entries(manifest).filter(([key, value]) => {
    try {
      applyAllOrNothing(state, () => applyLoadedProjectWorkspaceManifest(state, { [key]: value } as WorkspaceManifest, opts))
      return true
    } catch {
      return false
    }
  })
  return entries.length > 0 ? Object.fromEntries(entries) as unknown as WorkspaceManifest : undefined
}

/**
 * A failure partway through must not leave the settings applied before it in
 * force.
 */
function applyAllOrNothing<Result> (state: ConfigBuildState, apply: () => Result): Result {
  const before = { ...state.pnpmConfig }
  const explicitlySetKeysBefore = [...state.explicitlySetKeys]
  const warningsLengthBefore = state.warnings.length
  try {
    return apply()
  } catch (err: unknown) {
    restoreConfig(state.pnpmConfig as unknown as Record<string, unknown>, before)
    state.explicitlySetKeys.clear()
    for (const key of explicitlySetKeysBefore) state.explicitlySetKeys.add(key)
    state.warnings.length = warningsLengthBefore
    throw err
  }
}

function restoreConfig (config: Record<string, unknown>, before: Record<string, unknown>): void {
  for (const key of Object.keys(config)) {
    if (!(key in before)) delete config[key]
  }
  Object.assign(config, before)
}

async function applyProjectWorkspaceManifest (
  state: ConfigBuildState,
  opts: { forSelfUpdate?: boolean, workspaceDir: string }
): Promise<Record<string, string> | undefined> {
  const workspaceManifest = await readWorkspaceManifest(opts.workspaceDir)
  return applyLoadedProjectWorkspaceManifest(state, workspaceManifest, opts)
}

function applyLoadedProjectWorkspaceManifest (
  state: ConfigBuildState,
  workspaceManifest: WorkspaceManifest | undefined,
  { forSelfUpdate, workspaceDir }: { forSelfUpdate?: boolean, workspaceDir: string }
): Record<string, string> | undefined {
  const { cliOptions, pnpmConfig } = state
  pnpmConfig.workspacePackagePatterns = cliOptions['workspace-packages'] as string[] ?? workspaceManifest?.packages ?? ['.']
  if (!workspaceManifest) return undefined
  warnAboutDroppedWorkspaceManifestKeys(state.warnings, workspaceManifest)
  const settings = addSettingsFromWorkspaceManifestToConfig(pnpmConfig, {
    configFromCliOpts: state.configFromCliOpts,
    projectManifest: pnpmConfig.rootProjectManifest,
    skipSettings: forSelfUpdate
      ? new Set([...PROJECT_MANIFEST_SKIPPED_KEYS, ...SELF_UPDATE_SKIPPED_SETTINGS])
      : PROJECT_MANIFEST_SKIPPED_KEYS,
    workspaceDir,
    workspaceManifest,
  })
  // Read after the manifest's entries it may not contribute, such as one whose
  // URL holds an env placeholder, are dropped.
  pnpmConfig.workspaceRegistryPrefixes = Object.keys(settings.registriesByPrefix ?? {})
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
