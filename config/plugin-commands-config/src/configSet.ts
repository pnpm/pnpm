import fs from 'fs'
import path from 'path'
import util from 'util'
import { types } from '@pnpm/config'
import { PnpmError } from '@pnpm/error'
import { getObjectValueByPropertyPath } from '@pnpm/object.property-path'
import { runNpm } from '@pnpm/run-npm'
import { updateWorkspaceManifest } from '@pnpm/workspace.manifest-writer'
import camelCase from 'camelcase'
import kebabCase from 'lodash.kebabcase'
import { readIniFile } from 'read-ini-file'
import { writeIniFile } from 'write-ini-file'
import { type ConfigCommandOptions } from './ConfigCommandOptions'
import { isStrictlyKebabCase } from './isStrictlyKebabCase'
import { parseConfigPropertyPath } from './parseConfigPropertyPath'

export async function configSet (opts: ConfigCommandOptions, key: string, value: string | null): Promise<void> {
  if (opts.global && settingShouldFallBackToNpm(key)) {
    const _runNpm = runNpm.bind(null, opts.npmPath)
    if (value == null) {
      _runNpm(['config', 'delete', key])
    } else {
      _runNpm(['config', 'set', `${key}=${value}`])
    }
    return
  }

  const parsedPropertyPath = isStrictlyKebabCase(key)
    ? [key] // we don't parse kebab-case keys as property paths because it's not a valid JS syntax
    : Array.from(parseConfigPropertyPath(key))
  if (parsedPropertyPath.length === 0) {
    throw new PnpmError('SET_CONFIG_BY_EMPTY_PATH', 'Cannot set config value by an empty path')
  }
  const prefix = parsedPropertyPath.slice(0, -1)
  const bottomLevelKey = parsedPropertyPath.at(-1)!

  if (opts.global === true || fs.existsSync(path.join(opts.dir, '.npmrc'))) {
    const configPath = opts.global ? path.join(opts.configDir, 'rc') : path.join(opts.dir, '.npmrc')
    const overallSettings = await safeReadIniFile(configPath)
    type TargetSubSettings = string | number | null | undefined | Record<string | number, unknown>
    const targetSubSettings = getObjectValueByPropertyPath(overallSettings, prefix) as TargetSubSettings
    if (typeof targetSubSettings !== 'object') {
      throw new PnpmError('SET_PROPERTY_OF_NON_OBJECT', `Cannot set property ${JSON.stringify(bottomLevelKey)} of a non-object (${JSON.stringify(targetSubSettings)})`)
    }
    if (value == null) {
      if (targetSubSettings?.[bottomLevelKey] == null) return
      delete targetSubSettings[bottomLevelKey]
    } else if (targetSubSettings == null) {
      throw new PnpmError('SET_PROPERTY_OF_NULL', `Cannot set property ${JSON.stringify(bottomLevelKey)} of null`)
    } else {
      targetSubSettings[bottomLevelKey] = value
    }
    await writeIniFile(configPath, overallSettings)
    return
  }

  // const [topLevelKey, ...middle] = prefix
  // const

  key = camelCase(key)
  await updateWorkspaceManifest(opts.workspaceDir ?? opts.dir, {
    [key]: castField(value, kebabCase(key)),
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

export function settingShouldFallBackToNpm (key: string): boolean {
  return (
    ['registry', '_auth', '_authToken', 'username', '_password'].includes(key) ||
    key[0] === '@' ||
    key.startsWith('//')
  )
}

async function safeReadIniFile (configPath: string): Promise<Record<string, unknown>> {
  try {
    return await readIniFile(configPath) as Record<string, unknown>
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') return {}
    throw err
  }
}
