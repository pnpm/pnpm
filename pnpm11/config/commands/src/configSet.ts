import path from 'node:path'
import util from 'node:util'

import { type ConfigFileKey, isConfigFileKey, isIniConfigKey, isProjectManifestSkippedSetting, types, whereRefusedKeyBelongs } from '@pnpm/config.reader'
import { GLOBAL_CONFIG_YAML_FILENAME, WORKSPACE_MANIFEST_FILENAME } from '@pnpm/constants'
import { PnpmError } from '@pnpm/error'
import { parsePropertyPath } from '@pnpm/object.property-path'
import { isCamelCase, isStrictlyKebabCase } from '@pnpm/text.naming-cases'
import { updateWorkspaceManifest } from '@pnpm/workspace.workspace-manifest-writer'
import camelCase from 'camelcase'
import kebabCase from 'lodash.kebabcase'
import { pathExists } from 'path-exists'
import { readIniFile } from 'read-ini-file'
import { writeIniFile } from 'write-ini-file'

import type { ConfigCommandOptions } from './ConfigCommandOptions.js'
import { getConfigFileInfo } from './getConfigFileInfo.js'

export async function configSet (opts: ConfigCommandOptions, key: string, valueParam: string | null): Promise<void> {
  let isAuthSetting = isIniConfigKey(key)
  if (!isAuthSetting) {
    key = validateSimpleKey(key)
    isAuthSetting = isIniConfigKey(key)
  }
  let value: unknown = valueParam
  if (valueParam != null && opts.json) {
    value = JSON.parse(valueParam)
  }

  if (isAuthSetting) {
    const configPath = opts.global
      ? path.join(opts.configDir, 'auth.ini')
      : path.join(opts.dir, '.npmrc')
    if (value != null && typeof value !== 'string' && isStringOnlyIniKey(key)) {
      throw new PnpmError('CONFIG_SET_AUTH_NON_STRING', `Cannot set ${key} to a non-string value (${JSON.stringify(value)})`)
    }
    const settings = await safeReadIniFile(configPath)
    if (value == null) {
      if (settings[key] == null) return
      delete settings[key]
    } else {
      settings[key] = value
    }
    await writeIniFile(configPath, settings)
    return
  }

  const { configDir, configFileName } = getConfigFileInfo(key, opts)
  const configPath = path.join(configDir, configFileName)

  switch (configFileName) {
    case GLOBAL_CONFIG_YAML_FILENAME:
    case WORKSPACE_MANIFEST_FILENAME: {
      if (configFileName === GLOBAL_CONFIG_YAML_FILENAME) {
        key = validateYamlConfigKey(key)
      }
      const writtenKey = validateWorkspaceKey(key)
      // `castField` is what decides a removal: `pnpm config delete` arrives
      // with a null value, and `pnpm config set <key> null` casts to one. Both
      // are how a manifest that already carries one of these gets fixed, so
      // the result of the cast — not the raw parameter — gates the refusal.
      const castValue = castField(value, kebabCase(writtenKey))
      if (castValue != null && configFileName === WORKSPACE_MANIFEST_FILENAME && isProjectManifestSkippedSetting(writtenKey)) {
        throw new ConfigSetNotAProjectSettingError(writtenKey)
      }
      // Removing from a file that is not there is a no-op, not an error: the
      // writer deletes a manifest once the removal empties it, and would fail
      // to delete one that was never written.
      if (castValue == null && !await pathExists(configPath)) break
      const updatedFields: Record<string, unknown> = {
        [writtenKey]: castValue,
      }
      // pnpm always writes the normalized spelling, but a hand-edited file may
      // carry another one — and for a project manifest that is the spelling
      // the reader names when it reports the setting as ignored. Remove both,
      // so the remedy the warning implies actually clears the file.
      if (castValue == null && key !== writtenKey) {
        updatedFields[key] = null
      }
      await updateWorkspaceManifest(configDir, { fileName: configFileName, updatedFields })
      break
    }

    case 'auth.ini':
    case '.npmrc': {
      const settings = await safeReadIniFile(configPath)
      key = validateIniConfigKey(key)
      if (value == null) {
        if (settings[key] == null) return
        delete settings[key]
      } else {
        settings[key] = value
      }
      await writeIniFile(configPath, settings)
      break
    }

    default: {
      const _typeGuard: never = configFileName
      throw new Error(`Unhandled case: ${JSON.stringify(_typeGuard)}`)
    }
  }
}

function castField (value: unknown, key: string) {
  if (typeof value !== 'string') {
    return value
  }

  const type = types[key as keyof typeof types] as (string | number | boolean | null | NumberConstructor)
  const typeList = Array.isArray(type) ? type : [type]
  const isNumber = typeList.includes(Number)

  value = value.trim()

  switch (value) {
    case 'true': {
      return true
    }
    case 'false': {
      return false
    }
    case 'null': {
      return null
    }
    case 'undefined': {
      return undefined
    }
  }

  if (isNumber && !isNaN(value as number)) {
    value = Number(value)
  }

  return value
}

export class ConfigSetKeyEmptyKeyError extends PnpmError {
  constructor () {
    super('CONFIG_SET_EMPTY_KEY', 'Cannot set config with an empty key')
  }
}

export class ConfigSetDeepKeyError extends PnpmError {
  constructor () {
    // it shouldn't be supported until there is a mechanism to validate the config value
    super('CONFIG_SET_DEEP_KEY', 'Setting deep property path is not supported')
  }
}

/**
 * Validate if {@link key} is a simple key or a property path.
 *
 * If it is an empty property path or a property path longer than 1, throw an error.
 *
 * If it is a simple key (or a property path with length of 1), return it.
 */
function validateSimpleKey (key: string): string {
  if (isStrictlyKebabCase(key)) return key

  const iter = parsePropertyPath(key)

  const first = iter.next()
  if (first.done) throw new ConfigSetKeyEmptyKeyError()

  const second = iter.next()
  if (!second.done) throw new ConfigSetDeepKeyError()

  return first.value.toString()
}

export class ConfigSetUnsupportedIniConfigKeyError extends PnpmError {
  readonly key: string
  constructor (key: string) {
    super('CONFIG_SET_UNSUPPORTED_INI_CONFIG_KEY', `Key ${JSON.stringify(key)} isn't supported by INI config files`, {
      hint: `Add ${JSON.stringify(camelCase(key))} to the project workspace manifest instead`,
    })
    this.key = key
  }
}

/**
 * Validate whether the kebab-case of {@link key} is supported by INI config files.
 *
 * Return the kebab-case if it is, throw an error otherwise.
 *
 * "INI config files" includes:
 * * The global INI config file named `rc`.
 * * The local INI config file named `.npmrc`.
 */
function validateIniConfigKey (key: string): string {
  const kebabKey = kebabCase(key)
  if (Object.hasOwn(types, kebabKey)) {
    return kebabKey
  }
  throw new ConfigSetUnsupportedIniConfigKeyError(key)
}

export class ConfigSetUnsupportedWorkspaceKeyError extends PnpmError {
  readonly key: string
  constructor (key: string) {
    super('CONFIG_SET_UNSUPPORTED_WORKSPACE_KEY', `The key ${JSON.stringify(key)} isn't supported by the workspace manifest`, {
      // No `hintForRefusedKey` here: `validateWorkspaceKey` returns early for
      // every refused setting, so this error never sees one.
      hint: `Try ${JSON.stringify(camelCase(key))}`,
    })
    this.key = key
  }
}

/** The suggestion for {@link key}, which falls back when the key is allowed in a project manifest. */
function hintForRefusedKey (key: string, fallback: string): string {
  const camelKey = camelCase(key)
  if (!isProjectManifestSkippedSetting(camelKey)) return fallback
  return whereRefusedKeyBelongs(camelKey)
}

export class ConfigSetNotAProjectSettingError extends PnpmError {
  readonly key: string
  constructor (key: string) {
    super('CONFIG_SET_NOT_A_PROJECT_SETTING', `The key ${JSON.stringify(key)} cannot be set in a project's pnpm-workspace.yaml`, {
      hint: whereRefusedKeyBelongs(key),
    })
    this.key = key
  }
}

/**
 * Only an rc option key would be allowed to be kebab-case, otherwise, it must be camelCase.
 *
 * Return the camelCase of {@link key} if it's valid.
 */
function validateWorkspaceKey (key: string): string {
  if (Object.hasOwn(types, key) || isConfigFileKey(key)) return camelCase(key)
  // Most of these are absent from `types`, so their kebab-case spelling would
  // fall through to the rejection below — leaving no way to clear the key the
  // reader's warning just named, since it reports the spelling the file used.
  // Writing one is still refused, by the caller.
  if (isProjectManifestSkippedSetting(camelCase(key))) return camelCase(key)
  if (!isCamelCase(key)) throw new ConfigSetUnsupportedWorkspaceKeyError(key)
  return key
}

const STRING_ONLY_INI_KEYS = ['_auth', '_authToken', '_password', 'username', 'registry']

function isStringOnlyIniKey (key: string): boolean {
  if (STRING_ONLY_INI_KEYS.includes(key)) return true
  if (key.startsWith('@')) return true
  if (key.startsWith('//')) return true
  return false
}

async function safeReadIniFile (configPath: string): Promise<Record<string, unknown>> {
  try {
    return await readIniFile(configPath) as Record<string, unknown>
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') return {}
    throw err
  }
}

export class ConfigSetUnsupportedYamlConfigKeyError extends PnpmError {
  readonly key: string
  constructor (key: string) {
    super('CONFIG_SET_UNSUPPORTED_YAML_CONFIG_KEY', `The key ${JSON.stringify(key)} isn't supported by the global config.yaml file`, {
      hint: hintForRefusedKey(key, 'Try setting them instead to the local pnpm-workspace.yaml file'),
    })
    this.key = key
  }
}

/**
 * Validate whether the {@link key} is allowed in the global config.yaml file.
 *
 * Return the kebab-case if it is, throw an error otherwise.
 */
function validateYamlConfigKey (key: string): ConfigFileKey {
  const kebabKey = kebabCase(key)
  if (!isConfigFileKey(kebabKey)) {
    throw new ConfigSetUnsupportedYamlConfigKeyError(key)
  }
  return kebabKey
}
