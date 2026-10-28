import { redactAndSanitize } from '@pnpm/error'
import { isCamelCase } from '@pnpm/text.naming-cases'
import type { WorkspaceManifest } from '@pnpm/workspace.workspace-manifest-reader'
import kebabCase from 'lodash.kebabcase'

import { isConfigFileKey } from '../configFileKey.js'
import { getGlobalConfigPath } from '../dirs.js'
import type { OptionsFromRootManifest } from '../getOptionsFromRootManifest.js'
import { quoteAndJoin } from '../quoteAndJoin.js'
import { isKnownSettingKey, quoteAndAnnotateUnknown } from '../unknownSettings.js'
import type { ConfigBuildState } from './configBuildState.js'
import type { BootstrapNetworkSettings, ProxySettings } from './registries.js'
import {
  GLOBAL_CONFIG_SKIPPED_KEYS,
  isRefusedByAProjectManifest,
  PIN_DECISION_SETTINGS,
  quoteAndExplain,
  quoteAndSuggestCamelCase,
  SCHEMA_DIRECTIVE_KEY,
} from './skippedKeys.js'
import { addSettingsFromWorkspaceManifestToConfig } from './workspaceManifestSettings.js'

export interface AppliedGlobalYamlConfig {
  /** The registries in effect right after the file was applied, or undefined when there is no global config.yaml. */
  registries: Record<string, string> | undefined
  /** The file's network settings, which the package-manager bootstrap reads too. */
  bootstrapNetworkSettings: BootstrapNetworkSettings
}

/**
 * Applies the settings of the global config.yaml, dropping (and warning about)
 * the keys that file may not set.
 */
export function applyGlobalYamlConfig (
  state: ConfigBuildState,
  { configDir, globalYamlConfig, skipUnreadableSettings }: { configDir: string, globalYamlConfig: WorkspaceManifest | undefined, skipUnreadableSettings?: boolean }
): AppliedGlobalYamlConfig {
  if (!globalYamlConfig) return { registries: undefined, bootstrapNetworkSettings: {} }
  if (!skipUnreadableSettings) {
    return applyGlobalYamlManifest(state, { configDir, globalYamlConfig })
  }
  try {
    return applyAllOrNothingSync(state, () => applyGlobalYamlManifest(state, { configDir, globalYamlConfig: { ...globalYamlConfig } }))
  } catch {}
  const pinDecisionSubset = pickKeys(globalYamlConfig, PIN_DECISION_SETTINGS)
  try {
    return applyAllOrNothingSync(state, () => applyGlobalYamlManifest(state, { configDir, globalYamlConfig: { ...pinDecisionSubset } }))
  } catch {}
  const readableSubset: Record<string, unknown> = {}
  for (const [key, value] of Object.entries(pinDecisionSubset)) {
    try {
      applyAllOrNothingSync(state, () => applyGlobalYamlManifest(state, { configDir, globalYamlConfig: { [key]: value } as unknown as WorkspaceManifest }))
      readableSubset[key] = value
    } catch {}
  }
  try {
    return applyAllOrNothingSync(state, () => applyGlobalYamlManifest(state, { configDir, globalYamlConfig: readableSubset as unknown as WorkspaceManifest }))
  } catch {
    return { registries: undefined, bootstrapNetworkSettings: {} }
  }
}

function pickKeys (manifest: WorkspaceManifest, keys: ReadonlySet<string>): WorkspaceManifest {
  const picked: Record<string, unknown> = {}
  for (const key of keys) {
    if (key in manifest) {
      picked[key] = (manifest as unknown as Record<string, unknown>)[key]
    }
  }
  return picked as unknown as WorkspaceManifest
}

function applyAllOrNothingSync<Result> (state: ConfigBuildState, apply: () => Result): Result {
  const before = { ...state.pnpmConfig }
  const explicitlySetKeysBefore = [...state.explicitlySetKeys]
  const warningsLengthBefore = state.warnings.length
  try {
    return apply()
  } catch (err: unknown) {
    for (const key of Object.keys(state.pnpmConfig)) {
      if (!(key in before)) delete (state.pnpmConfig as unknown as Record<string, unknown>)[key]
    }
    Object.assign(state.pnpmConfig, before)
    state.explicitlySetKeys.clear()
    for (const key of explicitlySetKeysBefore) state.explicitlySetKeys.add(key)
    state.warnings.length = warningsLengthBefore
    throw err
  }
}

function applyGlobalYamlManifest (
  state: ConfigBuildState,
  { configDir, globalYamlConfig }: { configDir: string, globalYamlConfig: WorkspaceManifest }
): AppliedGlobalYamlConfig {
  // Consumed by loadNpmrcConfig above; drop so it isn't flagged as unknown.
  delete (globalYamlConfig as unknown as Record<string, unknown>)._auth
  const droppedKeys = removeKeysTheGlobalConfigFileCannotSet(globalYamlConfig)
  if (droppedKeys.ignoredKeys.length > 0 || droppedKeys.kebabKeys.length > 0) {
    warnAboutDroppedGlobalConfigKeys(state.warnings, { ...droppedKeys, globalYamlConfigPath: getGlobalConfigPath(configDir) })
  }
  const settings = addSettingsFromWorkspaceManifestToConfig(state.pnpmConfig, {
    configFromCliOpts: state.configFromCliOpts,
    expandRequestDestinationEnv: true,
    projectManifest: undefined,
    skipSettings: GLOBAL_CONFIG_SKIPPED_KEYS,
    trustedSource: true,
    workspaceDir: undefined,
    workspaceManifest: globalYamlConfig,
  })
  return {
    registries: state.pnpmConfig.registriesByScope as Record<string, string> | undefined,
    bootstrapNetworkSettings: pickBootstrapNetworkSettings(settings),
  }
}

function pickBootstrapNetworkSettings (settings: OptionsFromRootManifest & ProxySettings): BootstrapNetworkSettings {
  const { httpProxy, httpsProxy, localAddress, noProxy, noproxy, proxy, strictSsl } = settings
  return { httpProxy, httpsProxy, localAddress, noProxy, noproxy, proxy, strictSsl }
}

interface DroppedGlobalConfigKeys {
  ignoredKeys: string[]
  /** The gate is kebab-based, but only camelCase keys are picked up later. */
  kebabKeys: string[]
}

type GlobalConfigKeyRejection = 'schemaDirective' | 'notAConfigFileKey' | 'notCamelCase'

function removeKeysTheGlobalConfigFileCannotSet (globalYamlConfig: WorkspaceManifest): DroppedGlobalConfigKeys {
  const dropped: DroppedGlobalConfigKeys = { ignoredKeys: [], kebabKeys: [] }
  for (const key in globalYamlConfig) {
    // A key set to null is dropped like any other the file may not set, but
    // it is not reported: it chose nothing, so there is nothing to correct.
    // A null a setting accepts (`httpProxy`, `pnprServer`, ...) is a value
    // like any other and passes through untouched.
    const setsNothing = globalYamlConfig[key as keyof typeof globalYamlConfig] == null
    const rejection = getGlobalConfigKeyRejection(key)
    if (rejection == null) continue
    delete globalYamlConfig[key as keyof typeof globalYamlConfig]
    if (setsNothing) continue
    if (rejection === 'notAConfigFileKey') {
      dropped.ignoredKeys.push(key)
    } else if (rejection === 'notCamelCase') {
      dropped.kebabKeys.push(key)
    }
  }
  return dropped
}

function getGlobalConfigKeyRejection (key: string): GlobalConfigKeyRejection | undefined {
  if (key === SCHEMA_DIRECTIVE_KEY) return 'schemaDirective'
  if (!isConfigFileKey(kebabCase(key))) return 'notAConfigFileKey'
  if (!isCamelCase(key)) return 'notCamelCase'
  return undefined
}

function warnAboutDroppedGlobalConfigKeys (
  warnings: string[],
  { ignoredKeys, kebabKeys, globalYamlConfigPath }: DroppedGlobalConfigKeys & { globalYamlConfigPath: string }
): void {
  const movable = ignoredKeys.filter((key) => !isRefusedByAProjectManifest(key) && isKnownSettingKey(key))
  const unrecognized = ignoredKeys.filter((key) => !isRefusedByAProjectManifest(key) && !isKnownSettingKey(key))
  const nowhere = ignoredKeys.filter(isRefusedByAProjectManifest)
  if (movable.length > 0) {
    warnings.push(`The following settings cannot be set in the global config file ("${globalYamlConfigPath}") and were ignored: ${quoteAndJoin(movable.map(redactAndSanitize))}. Move them to a project-level pnpm-workspace.yaml. To share these settings across projects, use config dependencies: https://pnpm.io/11.x/config-dependencies`)
  }
  if (unrecognized.length > 0) {
    warnings.push(`The following settings in the global config file ("${globalYamlConfigPath}") are not recognized by this version of pnpm and were ignored: ${quoteAndAnnotateUnknown(unrecognized)}.`)
  }
  if (nowhere.length > 0) {
    warnings.push(`The following settings cannot be set in the global config file ("${globalYamlConfigPath}") and were ignored: ${quoteAndExplain(nowhere)}.`)
  }
  if (kebabKeys.length > 0) {
    warnings.push(`The following settings in the global config file ("${globalYamlConfigPath}") were ignored because they are not written in camelCase: ${quoteAndSuggestCamelCase(kebabKeys)}.`)
  }
}
