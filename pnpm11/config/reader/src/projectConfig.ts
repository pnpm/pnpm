import { PnpmError } from '@pnpm/error'
import { omit } from 'ramda'

import { type Config, PROJECT_CONFIG_FIELDS, type ProjectConfig, type ProjectConfigRecord } from './Config.js'

export type CreateProjectConfigRecordOptions = Pick<Config, 'packageConfigs'>

export function createProjectConfigRecord (opts: CreateProjectConfigRecordOptions): ProjectConfigRecord | undefined {
  return createProjectConfigRecordFromConfigSet(opts.packageConfigs)
}

export type ProjectModulesDirOptions = Pick<Config, 'packageConfigs' | 'lockfileDir' | 'modulesDir'>

/**
 * Resolve per-project modules directories after validating packageConfigs once.
 * Shared-lockfile workspaces ignore packageConfigs, as their install does.
 */
export function createProjectModulesDirResolver (opts: ProjectModulesDirOptions): (projectName: string | undefined) => string | undefined {
  const projectConfigs = opts.lockfileDir == null ? createProjectConfigRecord(opts) : undefined
  return (projectName) => (projectName == null ? undefined : projectConfigs?.[projectName]?.modulesDir) ?? opts.modulesDir
}

/**
 * The modules directories that packageConfigs sets, keyed by project name.
 * Empty in shared-lockfile workspaces, which ignore packageConfigs.
 */
export function getModulesDirsByProjectName (opts: ProjectModulesDirOptions): Record<string, string> {
  if (opts.lockfileDir != null) return {}
  return Object.fromEntries(
    Object.entries(createProjectConfigRecord(opts) ?? {})
      .flatMap(([projectName, { modulesDir }]) => modulesDir == null ? [] : [[projectName, modulesDir]])
  )
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

  assertProjectConfigValueTypes(config)
  assertOnlySupportedProjectConfigFields(config)

  const result: ProjectConfig = config
  if (result.hoist === false) {
    return { ...result, hoistPattern: undefined }
  }
  return result
}

type ProjectConfigValueType = 'boolean' | 'string' | 'object'

const PROJECT_CONFIG_VALUE_TYPES: Array<[field: string, expectedType: ProjectConfigValueType]> = [
  ['hoist', 'boolean'],
  ['modulesDir', 'string'],
  ['saveExact', 'boolean'],
  ['savePrefix', 'string'],
  ['overrides', 'object'],
]

function assertProjectConfigValueTypes (config: object): void {
  for (const [field, expectedType] of PROJECT_CONFIG_VALUE_TYPES) {
    if (!(field in config)) continue
    const value = (config as Record<string, unknown>)[field]
    if (value !== undefined && !isProjectConfigValueOfType(value, expectedType)) {
      throw new ProjectConfigInvalidValueTypeError(expectedType, value)
    }
  }
}

function isProjectConfigValueOfType (value: unknown, expectedType: ProjectConfigValueType): boolean {
  if (expectedType === 'object') {
    return typeof value === 'object' && value !== null && !Array.isArray(value)
  }
  return typeof value === expectedType
}

function assertOnlySupportedProjectConfigFields (config: object): void {
  for (const key in config) {
    if ((config as Record<string, unknown>)[key] !== undefined && !(PROJECT_CONFIG_FIELDS as string[]).includes(key)) {
      throw new ProjectConfigUnsupportedFieldError(key)
    }
  }
}

export class ProjectConfigsIsNeitherObjectNorArrayError extends PnpmError {
  readonly configSet: unknown
  constructor (configSet: unknown) {
    super('PROJECT_CONFIGS_IS_NEITHER_OBJECT_NOR_ARRAY', `Expecting packageConfigs to be either an object or an array but received ${JSON.stringify(configSet)}`)
    this.configSet = configSet
  }
}

export class ProjectConfigsArrayItemIsNotAnObjectError extends PnpmError {
  readonly item: unknown
  constructor (item: unknown) {
    super('PROJECT_CONFIGS_ARRAY_ITEM_IS_NOT_AN_OBJECT', `Expecting a packageConfigs item to be an object but received ${JSON.stringify(item)}`)
    this.item = item
  }
}

export class ProjectConfigsArrayItemMatchIsNotDefinedError extends PnpmError {
  constructor () {
    super('PROJECT_CONFIGS_ARRAY_ITEM_MATCH_IS_NOT_DEFINED', 'A packageConfigs match is not defined')
  }
}

export class ProjectConfigsArrayItemMatchIsNotAnArrayError extends PnpmError {
  readonly match: unknown
  constructor (match: unknown) {
    super('PROJECT_CONFIGS_ARRAY_ITEM_MATCH_IS_NOT_AN_ARRAY', `Expecting a packageConfigs match to be an array but received ${JSON.stringify(match)}`)
    this.match = match
  }
}

export class ProjectConfigsMatchItemIsNotAStringError extends PnpmError {
  readonly matchItem: unknown
  constructor (matchItem: unknown) {
    super('PROJECT_CONFIGS_MATCH_ITEM_IS_NOT_A_STRING', `Expecting a match item to be a string but received ${JSON.stringify(matchItem)}`)
    this.matchItem = matchItem
  }
}

const withoutMatch = omit(['match'])

function createProjectConfigRecordFromConfigSet (configSet: unknown): ProjectConfigRecord | undefined {
  if (configSet == null) return undefined
  if (typeof configSet !== 'object') throw new ProjectConfigsIsNeitherObjectNorArrayError(configSet)

  if (!Array.isArray(configSet)) {
    return createProjectConfigRecordFromObject(configSet as Record<string, unknown>)
  }
  return createProjectConfigRecordFromArray(configSet as unknown[])
}

function createProjectConfigRecordFromObject (configSet: Record<string, unknown>): ProjectConfigRecord {
  const result: ProjectConfigRecord = {}
  for (const projectName in configSet) {
    result[projectName] = createProjectConfigFromRaw(configSet[projectName])
  }
  return result
}

function createProjectConfigRecordFromArray (configSet: unknown[]): ProjectConfigRecord {
  const result: ProjectConfigRecord = {}
  for (const item of configSet) {
    assertValidProjectConfigsArrayItem(item)
    const projectConfig = createProjectConfigFromRaw(withoutMatch(item))

    for (const projectName of item.match) {
      if (typeof projectName !== 'string') {
        throw new ProjectConfigsMatchItemIsNotAStringError(projectName)
      }

      result[projectName] = projectConfig
    }
  }
  return result
}

function assertValidProjectConfigsArrayItem (item: unknown): asserts item is { match: unknown[] } {
  if (!item || typeof item !== 'object' || Array.isArray(item)) {
    throw new ProjectConfigsArrayItemIsNotAnObjectError(item)
  }

  if (!('match' in item)) {
    throw new ProjectConfigsArrayItemMatchIsNotDefinedError()
  }

  if (typeof item.match !== 'object' || !Array.isArray(item.match)) {
    throw new ProjectConfigsArrayItemMatchIsNotAnArrayError(item.match)
  }
}
