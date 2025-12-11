import path from 'path'
import util from 'util'
import readYamlFile from 'read-yaml-file'
import { PnpmError } from '@pnpm/error'
import { type Config } from './Config.js'

const LOCAL_CONFIG_FIELDS = [
  'modulesDir',
  'saveExact',
  'savePrefix',
] as const satisfies Array<keyof Config>

export type LocalConfig = Partial<Pick<Config, typeof LOCAL_CONFIG_FIELDS[number]>>

export async function readLocalConfig (prefix: string): Promise<LocalConfig> {
  let rawLocalConfig: unknown
  try {
    rawLocalConfig = await readRawLocalConfig(prefix)
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') return {}
    throw err
  }

  validateRawLocalConfig(rawLocalConfig)
  return rawLocalConfig
}

async function readRawLocalConfig (prefix: string): Promise<unknown> {
  return readYamlFile.default(path.join(prefix, 'config.yaml'))
}

export class LocalConfigIsNotAnObjectError extends PnpmError {
  readonly actualRawConfig: unknown
  constructor (actualRawConfig: unknown) {
    super('LOCAL_CONFIG_NOT_AN_OBJECT', `Expecting local config to be an object, but received ${JSON.stringify(actualRawConfig)}`)
    this.actualRawConfig = actualRawConfig
  }
}

export class LocalConfigInvalidValueTypeError extends PnpmError {
  readonly expectedType: string
  readonly actualType: string
  readonly actualValue: unknown
  constructor (expectedType: string, actualValue: unknown) {
    const actualType = typeof actualValue
    super('LOCAL_CONFIG_INVALID_VALUE_TYPE', `Expecting a value of type ${expectedType} but received a value of type ${actualType}: ${JSON.stringify(actualValue)}`)
    this.expectedType = expectedType
    this.actualType = actualType
    this.actualValue = actualValue
  }
}

export class LocalConfigUnsupportedFieldError extends PnpmError {
  readonly field: string
  constructor (field: string) {
    super('LOCAL_CONFIG_UNSUPPORTED_FIELD', `Field ${field} is not supported but was specified`)
    this.field = field
  }
}

function validateRawLocalConfig (config: unknown): asserts config is LocalConfig {
  if (typeof config !== 'object' || !config || Array.isArray(config)) throw new LocalConfigIsNotAnObjectError(config)
  if ('modulesDir' in config && config.modulesDir !== undefined && typeof config.modulesDir !== 'string') {
    throw new LocalConfigInvalidValueTypeError('string', config.modulesDir)
  }
  if ('saveExact' in config && config.saveExact !== undefined && typeof config.saveExact !== 'boolean') {
    throw new LocalConfigInvalidValueTypeError('boolean', config.saveExact)
  }
  if ('savePrefix' in config && config.savePrefix !== undefined && typeof config.savePrefix !== 'string') {
    throw new LocalConfigInvalidValueTypeError('string', config.savePrefix)
  }
  for (const key in config) {
    if (!(LOCAL_CONFIG_FIELDS as string[]).includes(key)) {
      throw new LocalConfigUnsupportedFieldError(key)
    }
  }
}
