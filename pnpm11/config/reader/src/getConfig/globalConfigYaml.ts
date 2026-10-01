import { redactAndSanitize } from '@pnpm/error'
import { isCamelCase } from '@pnpm/text.naming-cases'
import type { WorkspaceManifest } from '@pnpm/workspace.workspace-manifest-reader'
import kebabCase from 'lodash.kebabcase'

import { isConfigFileKey } from '../configFileKey.js'
import { getGlobalConfigPath } from '../dirs.js'
import { quoteAndJoin } from '../quoteAndJoin.js'
import { isKnownSettingKey, quoteAndAnnotateUnknown } from '../unknownSettings.js'
import type { ConfigBuildState } from './configBuildState.js'
import {
  GLOBAL_CONFIG_SKIPPED_KEYS,
  isRefusedByAProjectManifest,
  quoteAndExplain,
  quoteAndSuggestCamelCase,
  SCHEMA_DIRECTIVE_KEY,
} from './skippedKeys.js'
import { addSettingsFromWorkspaceManifestToConfig } from './workspaceManifestSettings.js'

/**
 * Applies the settings of the global config.yaml, dropping (and warning about)
 * the keys that file may not set.
 *
 * Returns the registries in effect right after that file was applied, or
 * undefined when there is no global config.yaml.
 */
export function applyGlobalYamlConfig (
  state: ConfigBuildState,
  { configDir, globalYamlConfig }: { configDir: string, globalYamlConfig: WorkspaceManifest | undefined }
): Record<string, string> | undefined {
  if (!globalYamlConfig) return undefined
  // Consumed by loadNpmrcConfig above; drop so it isn't flagged as unknown.
  delete (globalYamlConfig as unknown as Record<string, unknown>)._auth
  const droppedKeys = removeKeysTheGlobalConfigFileCannotSet(globalYamlConfig)
  if (droppedKeys.ignoredKeys.length > 0 || droppedKeys.kebabKeys.length > 0) {
    warnAboutDroppedGlobalConfigKeys(state.warnings, { ...droppedKeys, globalYamlConfigPath: getGlobalConfigPath(configDir) })
  }
  addSettingsFromWorkspaceManifestToConfig(state.pnpmConfig, {
    configFromCliOpts: state.configFromCliOpts,
    expandRequestDestinationEnv: true,
    projectManifest: undefined,
    skipSettings: GLOBAL_CONFIG_SKIPPED_KEYS,
    trustedSource: true,
    workspaceDir: undefined,
    workspaceManifest: globalYamlConfig,
  })
  return state.pnpmConfig.registriesByScope as Record<string, string> | undefined
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
