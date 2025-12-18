import { omit } from 'ramda'
import {
  type Config,
  type ProjectConfig,
  type ProjectConfigMultiMatch,
  type ProjectConfigRecord,
  type ProjectConfigSet,
} from '../src/Config.js'
import { createProjectConfigRecord } from '../src/projectConfig.js'

it('returns undefined for undefined', () => {
  expect(createProjectConfigRecord({})).toBeUndefined()
  expect(createProjectConfigRecord({ projectConfigs: undefined })).toBeUndefined()
  expect(createProjectConfigRecord({ projectConfigs: null as unknown as undefined })).toBeUndefined()
})

it('errors on invalid projectConfigs', () => {
  expect(() => createProjectConfigRecord({
    projectConfigs: 0 as unknown as ProjectConfigSet,
  })).toThrow(expect.objectContaining({
    configSet: 0,
    code: 'ERR_PNPM_PROJECT_CONFIGS_IS_NEITHER_OBJECT_NOR_ARRAY',
  }))

  expect(() => createProjectConfigRecord({
    projectConfigs: 'some string' as unknown as ProjectConfigSet,
  })).toThrow(expect.objectContaining({
    configSet: 'some string',
    code: 'ERR_PNPM_PROJECT_CONFIGS_IS_NEITHER_OBJECT_NOR_ARRAY',
  }))

  expect(() => createProjectConfigRecord({
    projectConfigs: true as unknown as ProjectConfigSet,
  })).toThrow(expect.objectContaining({
    configSet: true,
    code: 'ERR_PNPM_PROJECT_CONFIGS_IS_NEITHER_OBJECT_NOR_ARRAY',
  }))
})

describe('record', () => {
  it('returns an empty record for an empty record', () => {
    expect(createProjectConfigRecord({ projectConfigs: {} })).toStrictEqual({})
  })

  it('returns a valid record for a valid record', () => {
    const projectConfigs: ProjectConfigRecord = {
      'project-1': {
        modulesDir: 'foo',
      },
      'project-2': {
        saveExact: true,
      },
      'project-3': {
        savePrefix: '~',
      },
    }
    expect(createProjectConfigRecord({ projectConfigs })).toStrictEqual(projectConfigs)
  })

  it('explicitly sets hoistPattern to undefined when hoist is false', () => {
    expect(createProjectConfigRecord({
      projectConfigs: {
        'project-1': { hoist: false },
      },
    })).toStrictEqual({
      'project-1': {
        hoist: false,
        hoistPattern: undefined,
      },
    } as ProjectConfigRecord)
  })

  it('errors on invalid project config', () => {
    expect(() => createProjectConfigRecord({
      projectConfigs: {
        'project-1': 0 as unknown as ProjectConfig,
      },
    })).toThrow(expect.objectContaining({
      actualRawConfig: 0,
      code: 'ERR_PNPM_PROJECT_CONFIG_NOT_AN_OBJECT',
    }))

    expect(() => createProjectConfigRecord({
      projectConfigs: {
        'project-1': 'some string' as unknown as ProjectConfig,
      },
    })).toThrow(expect.objectContaining({
      actualRawConfig: 'some string',
      code: 'ERR_PNPM_PROJECT_CONFIG_NOT_AN_OBJECT',
    }))

    expect(() => createProjectConfigRecord({
      projectConfigs: {
        'project-1': true as unknown as ProjectConfig,
      },
    })).toThrow(expect.objectContaining({
      actualRawConfig: true,
      code: 'ERR_PNPM_PROJECT_CONFIG_NOT_AN_OBJECT',
    }))

    expect(() => createProjectConfigRecord({
      projectConfigs: {
        'project-1': null as unknown as ProjectConfig,
      },
    })).toThrow(expect.objectContaining({
      actualRawConfig: null,
      code: 'ERR_PNPM_PROJECT_CONFIG_NOT_AN_OBJECT',
    }))

    expect(() => createProjectConfigRecord({
      projectConfigs: {
        'project-1': [0, 1, 2] as unknown as ProjectConfig,
      },
    })).toThrow(expect.objectContaining({
      actualRawConfig: [0, 1, 2],
      code: 'ERR_PNPM_PROJECT_CONFIG_NOT_AN_OBJECT',
    }))
  })

  it('errors on invalid hoist', () => {
    expect(() => createProjectConfigRecord({
      projectConfigs: {
        'project-1': { hoist: 'invalid' as unknown as boolean },
      },
    })).toThrow(expect.objectContaining({
      expectedType: 'boolean',
      actualValue: 'invalid',
      code: 'ERR_PNPM_PROJECT_CONFIG_INVALID_VALUE_TYPE',
    }))

    expect(() => createProjectConfigRecord({
      projectConfigs: {
        'project-1': { hoist: 0 as unknown as boolean },
      },
    })).toThrow(expect.objectContaining({
      expectedType: 'boolean',
      actualValue: 0,
      code: 'ERR_PNPM_PROJECT_CONFIG_INVALID_VALUE_TYPE',
    }))
  })

  it('errors on invalid modulesDir', () => {
    expect(() => createProjectConfigRecord({
      projectConfigs: {
        'project-1': { modulesDir: 0 as unknown as string },
      },
    })).toThrow(expect.objectContaining({
      expectedType: 'string',
      actualValue: 0,
      code: 'ERR_PNPM_PROJECT_CONFIG_INVALID_VALUE_TYPE',
    }))

    expect(() => createProjectConfigRecord({
      projectConfigs: {
        'project-1': { modulesDir: true as unknown as string },
      },
    })).toThrow(expect.objectContaining({
      expectedType: 'string',
      actualValue: true,
      code: 'ERR_PNPM_PROJECT_CONFIG_INVALID_VALUE_TYPE',
    }))
  })

  it('errors on invalid saveExact', () => {
    expect(() => createProjectConfigRecord({
      projectConfigs: {
        'project-1': { saveExact: 'invalid' as unknown as boolean },
      },
    })).toThrow(expect.objectContaining({
      expectedType: 'boolean',
      actualValue: 'invalid',
      code: 'ERR_PNPM_PROJECT_CONFIG_INVALID_VALUE_TYPE',
    }))

    expect(() => createProjectConfigRecord({
      projectConfigs: {
        'project-1': { saveExact: 0 as unknown as boolean },
      },
    })).toThrow(expect.objectContaining({
      expectedType: 'boolean',
      actualValue: 0,
      code: 'ERR_PNPM_PROJECT_CONFIG_INVALID_VALUE_TYPE',
    }))
  })

  it('errors on invalid savePrefix', () => {
    expect(() => createProjectConfigRecord({
      projectConfigs: {
        'project-1': { savePrefix: 0 as unknown as string },
      },
    })).toThrow(expect.objectContaining({
      expectedType: 'string',
      actualValue: 0,
      code: 'ERR_PNPM_PROJECT_CONFIG_INVALID_VALUE_TYPE',
    }))

    expect(() => createProjectConfigRecord({
      projectConfigs: {
        'project-1': { savePrefix: false as unknown as string },
      },
    })).toThrow(expect.objectContaining({
      expectedType: 'string',
      actualValue: false,
      code: 'ERR_PNPM_PROJECT_CONFIG_INVALID_VALUE_TYPE',
    }))
  })

  it('errors on unsupported fields', () => {
    expect(() => createProjectConfigRecord({
      projectConfigs: {
        'project-1': {
          ignoreScripts: true,
        } as Partial<Config>,
      },
    })).toThrow(expect.objectContaining({
      field: 'ignoreScripts',
      code: 'ERR_PNPM_PROJECT_CONFIG_UNSUPPORTED_FIELD',
    }))

    expect(() => createProjectConfigRecord({
      projectConfigs: {
        'project-1': {
          hoistPattern: ['*'],
        } as Partial<Config>,
      },
    })).toThrow(expect.objectContaining({
      field: 'hoistPattern',
      code: 'ERR_PNPM_PROJECT_CONFIG_UNSUPPORTED_FIELD',
    }))
  })

  it('does not error on unsupported but undefined fields', () => {
    expect(createProjectConfigRecord({
      projectConfigs: {
        'project-1': {
          ignoreScripts: undefined,
          hoistPattern: undefined,
        } as Partial<Config>,
      },
    })).toStrictEqual({
      'project-1': {
        ignoreScripts: undefined,
        hoistPattern: undefined,
      } as Partial<Config>,
    })
  })
})

describe('array', () => {
  type ProjectConfigWithExtraFields = Pick<ProjectConfigMultiMatch, 'match'> & Partial<Config>

  it('returns an empty record for an empty array', () => {
    expect(createProjectConfigRecord({ projectConfigs: [] })).toStrictEqual({})
  })

  it('returns a map of project-specific settings for a non-empty array', () => {
    const withoutMatch: (withMatch: ProjectConfigMultiMatch) => ProjectConfig = omit(['match'])

    const projectConfigs = [
      {
        match: ['project-1'],
        modulesDir: 'foo',
      },
      {
        match: ['project-2', 'project-3'],
        saveExact: true,
      },
      {
        match: ['project-4', 'project-5', 'project-6'],
        savePrefix: '~',
      },
    ] as const satisfies ProjectConfigMultiMatch[]

    const record: ProjectConfigRecord | undefined = createProjectConfigRecord({ projectConfigs })

    expect(record).toStrictEqual({
      'project-1': withoutMatch(projectConfigs[0]),
      'project-2': withoutMatch(projectConfigs[1]),
      'project-3': withoutMatch(projectConfigs[1]),
      'project-4': withoutMatch(projectConfigs[2]),
      'project-5': withoutMatch(projectConfigs[2]),
      'project-6': withoutMatch(projectConfigs[2]),
    } as ProjectConfigRecord)

    expect(createProjectConfigRecord({ projectConfigs: record })).toStrictEqual(record)
  })

  it('explicitly sets hoistPattern to undefined when hoist is false', () => {
    expect(createProjectConfigRecord({
      projectConfigs: [{
        match: ['project-1'],
        hoist: false,
      }],
    })).toStrictEqual({
      'project-1': {
        hoist: false,
        hoistPattern: undefined,
      },
    } as ProjectConfigRecord)
  })

  it('errors on invalid array items', () => {
    expect(() => createProjectConfigRecord({
      projectConfigs: [0 as unknown as ProjectConfigMultiMatch],
    })).toThrow(expect.objectContaining({
      item: 0,
      code: 'ERR_PNPM_PROJECT_CONFIGS_ARRAY_ITEM_IS_NOT_AN_OBJECT',
    }))

    expect(() => createProjectConfigRecord({
      projectConfigs: ['some string' as unknown as ProjectConfigMultiMatch],
    })).toThrow(expect.objectContaining({
      item: 'some string',
      code: 'ERR_PNPM_PROJECT_CONFIGS_ARRAY_ITEM_IS_NOT_AN_OBJECT',
    }))

    expect(() => createProjectConfigRecord({
      projectConfigs: [true as unknown as ProjectConfigMultiMatch],
    })).toThrow(expect.objectContaining({
      item: true,
      code: 'ERR_PNPM_PROJECT_CONFIGS_ARRAY_ITEM_IS_NOT_AN_OBJECT',
    }))

    expect(() => createProjectConfigRecord({
      projectConfigs: [null as unknown as ProjectConfigMultiMatch],
    })).toThrow(expect.objectContaining({
      item: null,
      code: 'ERR_PNPM_PROJECT_CONFIGS_ARRAY_ITEM_IS_NOT_AN_OBJECT',
    }))
  })

  it('errors on undefined match', () => {
    expect(() => createProjectConfigRecord({
      projectConfigs: [{} as ProjectConfigMultiMatch],
    })).toThrow(expect.objectContaining({
      code: 'ERR_PNPM_PROJECT_CONFIGS_ARRAY_ITEM_MATCH_IS_NOT_DEFINED',
    }))
  })

  it('errors on non-array match', () => {
    expect(() => createProjectConfigRecord({
      projectConfigs: [{
        match: 0 as unknown as string[],
      }],
    })).toThrow(expect.objectContaining({
      match: 0,
      code: 'ERR_PNPM_PROJECT_CONFIGS_ARRAY_ITEM_MATCH_IS_NOT_AN_ARRAY',
    }))

    expect(() => createProjectConfigRecord({
      projectConfigs: [{
        match: 'some string' as unknown as string[],
      }],
    })).toThrow(expect.objectContaining({
      match: 'some string',
      code: 'ERR_PNPM_PROJECT_CONFIGS_ARRAY_ITEM_MATCH_IS_NOT_AN_ARRAY',
    }))

    expect(() => createProjectConfigRecord({
      projectConfigs: [{
        match: true as unknown as string[],
      }],
    })).toThrow(expect.objectContaining({
      match: true,
      code: 'ERR_PNPM_PROJECT_CONFIGS_ARRAY_ITEM_MATCH_IS_NOT_AN_ARRAY',
    }))

    expect(() => createProjectConfigRecord({
      projectConfigs: [{
        match: undefined as unknown as string[],
      }],
    })).toThrow(expect.objectContaining({
      match: undefined,
      code: 'ERR_PNPM_PROJECT_CONFIGS_ARRAY_ITEM_MATCH_IS_NOT_AN_ARRAY',
    }))

    expect(() => createProjectConfigRecord({
      projectConfigs: [{
        match: null as unknown as string[],
      }],
    })).toThrow(expect.objectContaining({
      match: null,
      code: 'ERR_PNPM_PROJECT_CONFIGS_ARRAY_ITEM_MATCH_IS_NOT_AN_ARRAY',
    }))
  })

  it('errors on non-string match item', () => {
    expect(() => createProjectConfigRecord({
      projectConfigs: [{
        match: [0 as unknown as string],
      }],
    })).toThrow(expect.objectContaining({
      matchItem: 0,
      code: 'ERR_PNPM_PROJECT_CONFIGS_MATCH_ITEM_IS_NOT_A_STRING',
    }))

    expect(() => createProjectConfigRecord({
      projectConfigs: [{
        match: [null as unknown as string],
      }],
    })).toThrow(expect.objectContaining({
      matchItem: null,
      code: 'ERR_PNPM_PROJECT_CONFIGS_MATCH_ITEM_IS_NOT_A_STRING',
    }))

    expect(() => createProjectConfigRecord({
      projectConfigs: [{
        match: [{} as unknown as string],
      }],
    })).toThrow(expect.objectContaining({
      matchItem: {},
      code: 'ERR_PNPM_PROJECT_CONFIGS_MATCH_ITEM_IS_NOT_A_STRING',
    }))
  })

  it('errors on invalid hoist', () => {
    expect(() => createProjectConfigRecord({
      projectConfigs: [{
        match: ['project-1'],
        hoist: 'invalid' as unknown as boolean,
      }],
    })).toThrow(expect.objectContaining({
      expectedType: 'boolean',
      actualValue: 'invalid',
      code: 'ERR_PNPM_PROJECT_CONFIG_INVALID_VALUE_TYPE',
    }))

    expect(() => createProjectConfigRecord({
      projectConfigs: [{
        match: ['project-1'],
        hoist: 0 as unknown as boolean,
      }],
    })).toThrow(expect.objectContaining({
      expectedType: 'boolean',
      actualValue: 0,
      code: 'ERR_PNPM_PROJECT_CONFIG_INVALID_VALUE_TYPE',
    }))
  })

  it('errors on invalid modulesDir', () => {
    expect(() => createProjectConfigRecord({
      projectConfigs: [{
        match: ['project-1'],
        modulesDir: 0 as unknown as string,
      }],
    })).toThrow(expect.objectContaining({
      expectedType: 'string',
      actualValue: 0,
      code: 'ERR_PNPM_PROJECT_CONFIG_INVALID_VALUE_TYPE',
    }))

    expect(() => createProjectConfigRecord({
      projectConfigs: [{
        match: ['project-1'],
        modulesDir: true as unknown as string,
      }],
    })).toThrow(expect.objectContaining({
      expectedType: 'string',
      actualValue: true,
      code: 'ERR_PNPM_PROJECT_CONFIG_INVALID_VALUE_TYPE',
    }))
  })

  it('errors on invalid saveExact', () => {
    expect(() => createProjectConfigRecord({
      projectConfigs: [{
        match: ['project-1'],
        saveExact: 'invalid' as unknown as boolean,
      }],
    })).toThrow(expect.objectContaining({
      expectedType: 'boolean',
      actualValue: 'invalid',
      code: 'ERR_PNPM_PROJECT_CONFIG_INVALID_VALUE_TYPE',
    }))

    expect(() => createProjectConfigRecord({
      projectConfigs: [{
        match: ['project-1'],
        saveExact: 0 as unknown as boolean,
      }],
    })).toThrow(expect.objectContaining({
      expectedType: 'boolean',
      actualValue: 0,
      code: 'ERR_PNPM_PROJECT_CONFIG_INVALID_VALUE_TYPE',
    }))
  })

  it('errors on invalid savePrefix', () => {
    expect(() => createProjectConfigRecord({
      projectConfigs: [{
        match: ['project-1'],
        savePrefix: 0 as unknown as string,
      }],
    })).toThrow(expect.objectContaining({
      expectedType: 'string',
      actualValue: 0,
      code: 'ERR_PNPM_PROJECT_CONFIG_INVALID_VALUE_TYPE',
    }))

    expect(() => createProjectConfigRecord({
      projectConfigs: [{
        match: ['project-1'],
        savePrefix: false as unknown as string,
      }],
    })).toThrow(expect.objectContaining({
      expectedType: 'string',
      actualValue: false,
      code: 'ERR_PNPM_PROJECT_CONFIG_INVALID_VALUE_TYPE',
    }))
  })

  it('errors on unsupported fields', () => {
    expect(() => createProjectConfigRecord({
      projectConfigs: [{
        match: ['project-1'],
        ignoreScripts: true,
      } as ProjectConfigWithExtraFields],
    })).toThrow(expect.objectContaining({
      field: 'ignoreScripts',
      code: 'ERR_PNPM_PROJECT_CONFIG_UNSUPPORTED_FIELD',
    }))

    expect(() => createProjectConfigRecord({
      projectConfigs: [{
        match: ['project-1'],
        hoistPattern: ['*'],
      }],
    })).toThrow(expect.objectContaining({
      field: 'hoistPattern',
      code: 'ERR_PNPM_PROJECT_CONFIG_UNSUPPORTED_FIELD',
    }))
  })

  it('does not error on unsupported but undefined fields', () => {
    expect(createProjectConfigRecord({
      projectConfigs: [{
        match: ['project-1'],
        ignoreScripts: undefined,
        hoistPattern: undefined,
      } as ProjectConfigWithExtraFields],
    })).toStrictEqual({
      'project-1': {
        ignoreScripts: undefined,
        hoistPattern: undefined,
      } as Partial<Config>,
    })
  })
})
