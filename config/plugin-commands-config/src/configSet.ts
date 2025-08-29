import fs from 'fs'
import path from 'path'
import util from 'util'
import { types } from '@pnpm/config'
import { PnpmError } from '@pnpm/error'
import { parsePropertyPath } from '@pnpm/object.property-path'
import { runNpm } from '@pnpm/run-npm'
import { updateWorkspaceManifest } from '@pnpm/workspace.manifest-writer'
import camelCase from 'camelcase'
import kebabCase from 'lodash.kebabcase'
import { readIniFile } from 'read-ini-file'
import { writeIniFile } from 'write-ini-file'
import { isCamelCase, isStrictlyKebabCase } from './checkCases.js'
import { type ConfigCommandOptions } from './ConfigCommandOptions.js'
import { settingShouldFallBackToNpm } from './settingShouldFallBackToNpm.js'

export async function configSet (opts: ConfigCommandOptions, key: string, valueParam: string | null): Promise<void> {
  let shouldFallbackToNpm = settingShouldFallBackToNpm(key)
  if (!shouldFallbackToNpm) {
    key = validateSimpleKey(key)
    shouldFallbackToNpm = settingShouldFallBackToNpm(key)
  }
  let value: unknown = valueParam
  if (valueParam != null && opts.json) {
    value = JSON.parse(valueParam)
  }
  if (opts.global && settingShouldFallBackToNpm(key)) {
    const _runNpm = runNpm.bind(null, opts.npmPath)
    if (value == null) {
      _runNpm(['config', 'delete', key])
      return
    }
    if (typeof value === 'string') {
      _runNpm(['config', 'set', `${key}=${value}`])
      return
    }
    throw new PnpmError('CONFIG_SET_AUTH_NON_STRING', `Cannot set ${key} to a non-string value (${JSON.stringify(value)})`)
  }
  if (opts.global === true || fs.existsSync(path.join(opts.dir, '.npmrc'))) {
    const configPath = opts.global ? path.join(opts.configDir, 'rc') : path.join(opts.dir, '.npmrc')
    const settings = await safeReadIniFile(configPath)
    key = validateRcKey(key)
    if (value == null) {
      if (settings[key] == null) return
      delete settings[key]
    } else {
      settings[key] = value
    }
    await writeIniFile(configPath, settings)
    return
  }
  key = validateWorkspaceKey(key)
  await updateWorkspaceManifest(opts.workspaceDir ?? opts.dir, {
    updatedFields: ({
      [key]: castField(value, kebabCase(key)),
    }),
  })
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

export class ConfigSetUnsupportedRcKeyError extends PnpmError {
  readonly key: string
  constructor (key: string) {
    super('CONFIG_SET_UNSUPPORTED_RC_KEY', `Key ${JSON.stringify(key)} isn't supported by rc files`, {
      hint: `Add ${JSON.stringify(camelCase(key))} to the project workspace manifest instead`,
    })
    this.key = key
  }
}

/**
 * Validate if the kebab-case of {@link key} is supported by rc files.
 *
 * Return the kebab-case if it is, throw an error otherwise.
 */
function validateRcKey (key: string): string {
  const kebabKey = kebabCase(key)
  if (kebabKey in types) {
    return kebabKey
  }
  throw new ConfigSetUnsupportedRcKeyError(key)
}

export class ConfigSetUnsupportedWorkspaceKeyError extends PnpmError {
  readonly key: string
  constructor (key: string) {
    super('CONFIG_SET_UNSUPPORTED_WORKSPACE_KEY', `The key ${JSON.stringify(key)} isn't supported by the workspace manifest`, {
      hint: `Try ${JSON.stringify(camelCase(key))}`,
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
  if (key in types) return camelCase(key)
  if (!isCamelCase(key)) throw new ConfigSetUnsupportedWorkspaceKeyError(key)
  return key
}

async function safeReadIniFile (configPath: string): Promise<Record<string, unknown>> {
  try {
    return await readIniFile(configPath) as Record<string, unknown>
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') return {}
    throw err
  }
}
