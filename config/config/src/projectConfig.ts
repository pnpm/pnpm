import { PnpmError } from '@pnpm/error'
import { PROJECT_CONFIG_FIELDS, type Config, type ProjectConfig, type ProjectConfigRecord } from './Config.js'

export type CreateProjectConfigRecordOptions = Pick<Config, 'projectSettings'>

export function createProjectConfigRecord (opts: CreateProjectConfigRecordOptions): ProjectConfigRecord | undefined {
  return createProjectConfigRecordFromConfigSet(opts.projectSettings)
}

export class ProjectConfigIsNotAnObjectError extends PnpmError {
  readonly actualRawConfig: unknown
  constructor (actualRawConfig: unknown) {
    super('PROJECT_CONFIG_NOT_AN_OBJECT', `Expecting project-specific config to be an object, but received ${JSON.stringify(actualRawConfig)}`)
    this.actualRawConfig = actualRawConfig
  }
}

export class ProjectConfigInvalidValueTypeError extends PnpmError {
  readonly expectedType: string
  readonly actualType: string
  readonly actualValue: unknown
  constructor (expectedType: string, actualValue: unknown) {
    const actualType = typeof actualValue
    super('PROJECT_CONFIG_INVALID_VALUE_TYPE', `Expecting a value of type ${expectedType} but received a value of type ${actualType}: ${JSON.stringify(actualValue)}`)
    this.expectedType = expectedType
    this.actualType = actualType
    this.actualValue = actualValue
  }
}

export class ProjectConfigUnsupportedFieldError extends PnpmError {
  readonly field: string
  constructor (field: string) {
    super('PROJECT_CONFIG_UNSUPPORTED_FIELD', `Field ${field} is not supported but was specified`)
    this.field = field
  }
}

function createProjectConfigFromRaw (config: unknown): ProjectConfig {
  if (typeof config !== 'object' || !config || Array.isArray(config)) {
    throw new ProjectConfigIsNotAnObjectError(config)
  }

  if ('hoist' in config && config.hoist !== undefined && typeof config.hoist !== 'boolean') {
    throw new ProjectConfigInvalidValueTypeError('boolean', config.hoist)
  }

  if ('modulesDir' in config && config.modulesDir !== undefined && typeof config.modulesDir !== 'string') {
    throw new ProjectConfigInvalidValueTypeError('string', config.modulesDir)
  }

  if ('saveExact' in config && config.saveExact !== undefined && typeof config.saveExact !== 'boolean') {
    throw new ProjectConfigInvalidValueTypeError('boolean', config.saveExact)
  }

  if ('savePrefix' in config && config.savePrefix !== undefined && typeof config.savePrefix !== 'string') {
    throw new ProjectConfigInvalidValueTypeError('string', config.savePrefix)
  }

  for (const key in config) {
    if ((config as Record<string, unknown>)[key] !== undefined && !(PROJECT_CONFIG_FIELDS as string[]).includes(key)) {
      throw new ProjectConfigUnsupportedFieldError(key)
    }
  }

  const result: ProjectConfig = config
  if (result.hoist === false) {
    return { ...result, hoistPattern: undefined }
  }
  return result
}

export class ProjectSettingsIsNeitherObjectNorArrayError extends PnpmError {
  readonly configSet: unknown
  constructor (configSet: unknown) {
    super('PROJECT_SETTINGS_IS_NEITHER_OBJECT_NOR_ARRAY', `Expecting projectSettings to be either an object or an array but received ${JSON.stringify(configSet)}`)
    this.configSet = configSet
  }
}

export class ProjectSettingsArrayItemIsNotAnObjectError extends PnpmError {
  readonly item: unknown
  constructor (item: unknown) {
    super('PROJECT_SETTINGS_ARRAY_ITEM_IS_NOT_AN_OBJECT', `Expecting a projectSettings item to be an object but received ${JSON.stringify(item)}`)
    this.item = item
  }
}

export class ProjectSettingsArrayItemMatchIsNotDefinedError extends PnpmError {
  constructor () {
    super('PROJECT_SETTINGS_ARRAY_ITEM_MATCH_IS_NOT_DEFINED', 'A projectSettings match is not defined')
  }
}

export class ProjectSettingsArrayItemMatchIsNotAnArrayError extends PnpmError {
  readonly match: unknown
  constructor (match: unknown) {
    super('PROJECT_SETTINGS_ARRAY_ITEM_MATCH_IS_NOT_AN_ARRAY', `Expecting a projectSettings match to be an array but received ${JSON.stringify(match)}`)
    this.match = match
  }
}

export class ProjectSettingsArrayItemSettingsIsNotDefinedError extends PnpmError {
  constructor () {
    super('PROJECT_SETTINGS_ARRAY_ITEM_SETTINGS_IS_NOT_DEFINED', 'A projectSettings settings is not defined')
  }
}

export class ProjectSettingsMatchItemIsNotAStringError extends PnpmError {
  readonly matchItem: unknown
  constructor (matchItem: unknown) {
    super('PROJECT_SETTINGS_MATCH_ITEM_IS_NOT_A_STRING', `Expecting a match item to be a string but received ${JSON.stringify(matchItem)}`)
    this.matchItem = matchItem
  }
}

function createProjectConfigRecordFromConfigSet (configSet: unknown): ProjectConfigRecord | undefined {
  if (configSet == null) return undefined
  if (typeof configSet !== 'object') throw new ProjectSettingsIsNeitherObjectNorArrayError(configSet)

  const result: ProjectConfigRecord = {}

  if (!Array.isArray(configSet)) {
    for (const projectName in configSet) {
      const projectConfig = (configSet as Record<string, unknown>)[projectName]
      result[projectName] = createProjectConfigFromRaw(projectConfig)
    }
    return result
  }

  for (const item of configSet as unknown[]) {
    if (!item || typeof item !== 'object' || Array.isArray(item)) {
      throw new ProjectSettingsArrayItemIsNotAnObjectError(item)
    }

    if (!('match' in item)) {
      throw new ProjectSettingsArrayItemMatchIsNotDefinedError()
    }

    if (typeof item.match !== 'object' || !Array.isArray(item.match)) {
      throw new ProjectSettingsArrayItemMatchIsNotAnArrayError(item.match)
    }

    if (!('settings' in item)) {
      throw new ProjectSettingsArrayItemSettingsIsNotDefinedError()
    }

    const projectConfig = createProjectConfigFromRaw(item.settings)

    for (const projectName of item.match as unknown[]) {
      if (typeof projectName !== 'string') {
        throw new ProjectSettingsMatchItemIsNotAStringError(projectName)
      }

      result[projectName] = projectConfig
    }
  }

  return result
}
