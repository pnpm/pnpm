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
  expect(createProjectConfigRecord({ projectSettings: undefined })).toBeUndefined()
  expect(createProjectConfigRecord({ projectSettings: null as unknown as undefined })).toBeUndefined()
})

it('errors on invalid projectSettings', () => {
  expect(() => createProjectConfigRecord({
    projectSettings: 0 as unknown as ProjectConfigSet,
  })).toThrow(expect.objectContaining({
    configSet: 0,
    code: 'ERR_PNPM_PROJECT_SETTINGS_IS_NEITHER_OBJECT_NOR_ARRAY',
  }))

  expect(() => createProjectConfigRecord({
    projectSettings: 'some string' as unknown as ProjectConfigSet,
  })).toThrow(expect.objectContaining({
    configSet: 'some string',
    code: 'ERR_PNPM_PROJECT_SETTINGS_IS_NEITHER_OBJECT_NOR_ARRAY',
  }))

  expect(() => createProjectConfigRecord({
    projectSettings: true as unknown as ProjectConfigSet,
  })).toThrow(expect.objectContaining({
    configSet: true,
    code: 'ERR_PNPM_PROJECT_SETTINGS_IS_NEITHER_OBJECT_NOR_ARRAY',
  }))
})

describe('record', () => {
  it('returns an empty record for an empty record', () => {
    expect(createProjectConfigRecord({ projectSettings: {} })).toStrictEqual({})
  })

  it('returns a valid record for a valid record', () => {
    const projectSettings: ProjectConfigRecord = {
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
    expect(createProjectConfigRecord({ projectSettings })).toStrictEqual(projectSettings)
  })

  it('explicitly sets hoistPattern to undefined when hoist is false', () => {
    expect(createProjectConfigRecord({
      projectSettings: {
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
      projectSettings: {
        'project-1': 0 as unknown as ProjectConfig,
      },
    })).toThrow(expect.objectContaining({
      actualRawConfig: 0,
      code: 'ERR_PNPM_PROJECT_CONFIG_NOT_AN_OBJECT',
    }))

    expect(() => createProjectConfigRecord({
      projectSettings: {
        'project-1': 'some string' as unknown as ProjectConfig,
      },
    })).toThrow(expect.objectContaining({
      actualRawConfig: 'some string',
      code: 'ERR_PNPM_PROJECT_CONFIG_NOT_AN_OBJECT',
    }))

    expect(() => createProjectConfigRecord({
      projectSettings: {
        'project-1': true as unknown as ProjectConfig,
      },
    })).toThrow(expect.objectContaining({
      actualRawConfig: true,
      code: 'ERR_PNPM_PROJECT_CONFIG_NOT_AN_OBJECT',
    }))

    expect(() => createProjectConfigRecord({
      projectSettings: {
        'project-1': null as unknown as ProjectConfig,
      },
    })).toThrow(expect.objectContaining({
      actualRawConfig: null,
      code: 'ERR_PNPM_PROJECT_CONFIG_NOT_AN_OBJECT',
    }))

    expect(() => createProjectConfigRecord({
      projectSettings: {
        'project-1': [0, 1, 2] as unknown as ProjectConfig,
      },
    })).toThrow(expect.objectContaining({
      actualRawConfig: [0, 1, 2],
      code: 'ERR_PNPM_PROJECT_CONFIG_NOT_AN_OBJECT',
    }))
  })

  it('errors on invalid hoist', () => {
    expect(() => createProjectConfigRecord({
      projectSettings: {
        'project-1': { hoist: 'invalid' as unknown as boolean },
      },
    })).toThrow(expect.objectContaining({
      expectedType: 'boolean',
      actualValue: 'invalid',
      code: 'ERR_PNPM_PROJECT_CONFIG_INVALID_VALUE_TYPE',
    }))

    expect(() => createProjectConfigRecord({
      projectSettings: {
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
      projectSettings: {
        'project-1': { modulesDir: 0 as unknown as string },
      },
    })).toThrow(expect.objectContaining({
      expectedType: 'string',
      actualValue: 0,
      code: 'ERR_PNPM_PROJECT_CONFIG_INVALID_VALUE_TYPE',
    }))

    expect(() => createProjectConfigRecord({
      projectSettings: {
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
      projectSettings: {
        'project-1': { saveExact: 'invalid' as unknown as boolean },
      },
    })).toThrow(expect.objectContaining({
      expectedType: 'boolean',
      actualValue: 'invalid',
      code: 'ERR_PNPM_PROJECT_CONFIG_INVALID_VALUE_TYPE',
    }))

    expect(() => createProjectConfigRecord({
      projectSettings: {
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
      projectSettings: {
        'project-1': { savePrefix: 0 as unknown as string },
      },
    })).toThrow(expect.objectContaining({
      expectedType: 'string',
      actualValue: 0,
      code: 'ERR_PNPM_PROJECT_CONFIG_INVALID_VALUE_TYPE',
    }))

    expect(() => createProjectConfigRecord({
      projectSettings: {
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
      projectSettings: {
        'project-1': {
          ignoreScripts: true,
        } as Partial<Config>,
      },
    })).toThrow(expect.objectContaining({
      field: 'ignoreScripts',
      code: 'ERR_PNPM_PROJECT_CONFIG_UNSUPPORTED_FIELD',
    }))

    expect(() => createProjectConfigRecord({
      projectSettings: {
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
      projectSettings: {
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
  it('returns an empty record for an empty array', () => {
    expect(createProjectConfigRecord({ projectSettings: [] })).toStrictEqual({})
  })

  it('returns a map of project-specific settings for a non-empty array', () => {
    const projectSettings = [
      {
        match: ['project-1'],
        settings: {
          modulesDir: 'foo',
        },
      },
      {
        match: ['project-2', 'project-3'],
        settings: {
          saveExact: true,
        },
      },
      {
        match: ['project-4', 'project-5', 'project-6'],
        settings: {
          savePrefix: '~',
        },
      },
    ] as const satisfies ProjectConfigMultiMatch[]

    const record: ProjectConfigRecord | undefined = createProjectConfigRecord({ projectSettings })

    expect(record).toStrictEqual({
      'project-1': projectSettings[0].settings,
      'project-2': projectSettings[1].settings,
      'project-3': projectSettings[1].settings,
      'project-4': projectSettings[2].settings,
      'project-5': projectSettings[2].settings,
      'project-6': projectSettings[2].settings,
    } as ProjectConfigRecord)

    expect(createProjectConfigRecord({ projectSettings: record })).toStrictEqual(record)
  })

  it('explicitly sets hoistPattern to undefined when hoist is false', () => {
    expect(createProjectConfigRecord({
      projectSettings: [{
        match: ['project-1'],
        settings: { hoist: false },
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
      projectSettings: [0 as unknown as ProjectConfigMultiMatch],
    })).toThrow(expect.objectContaining({
      item: 0,
      code: 'ERR_PNPM_PROJECT_SETTINGS_ARRAY_ITEM_IS_NOT_AN_OBJECT',
    }))

    expect(() => createProjectConfigRecord({
      projectSettings: ['some string' as unknown as ProjectConfigMultiMatch],
    })).toThrow(expect.objectContaining({
      item: 'some string',
      code: 'ERR_PNPM_PROJECT_SETTINGS_ARRAY_ITEM_IS_NOT_AN_OBJECT',
    }))

    expect(() => createProjectConfigRecord({
      projectSettings: [true as unknown as ProjectConfigMultiMatch],
    })).toThrow(expect.objectContaining({
      item: true,
      code: 'ERR_PNPM_PROJECT_SETTINGS_ARRAY_ITEM_IS_NOT_AN_OBJECT',
    }))

    expect(() => createProjectConfigRecord({
      projectSettings: [null as unknown as ProjectConfigMultiMatch],
    })).toThrow(expect.objectContaining({
      item: null,
      code: 'ERR_PNPM_PROJECT_SETTINGS_ARRAY_ITEM_IS_NOT_AN_OBJECT',
    }))
  })

  it('errors on undefined match', () => {
    expect(() => createProjectConfigRecord({
      projectSettings: [{
        settings: {},
      } as ProjectConfigMultiMatch],
    })).toThrow(expect.objectContaining({
      code: 'ERR_PNPM_PROJECT_SETTINGS_ARRAY_ITEM_MATCH_IS_NOT_DEFINED',
    }))
  })

  it('errors on undefined settings', () => {
    expect(() => createProjectConfigRecord({
      projectSettings: [{
        match: [] as string[],
      } as ProjectConfigMultiMatch],
    })).toThrow(expect.objectContaining({
      code: 'ERR_PNPM_PROJECT_SETTINGS_ARRAY_ITEM_SETTINGS_IS_NOT_DEFINED',
    }))
  })

  it('errors on non-array match', () => {
    expect(() => createProjectConfigRecord({
      projectSettings: [{
        match: 0 as unknown as string[],
        settings: {},
      }],
    })).toThrow(expect.objectContaining({
      match: 0,
      code: 'ERR_PNPM_PROJECT_SETTINGS_ARRAY_ITEM_MATCH_IS_NOT_AN_ARRAY',
    }))

    expect(() => createProjectConfigRecord({
      projectSettings: [{
        match: 'some string' as unknown as string[],
        settings: {},
      }],
    })).toThrow(expect.objectContaining({
      match: 'some string',
      code: 'ERR_PNPM_PROJECT_SETTINGS_ARRAY_ITEM_MATCH_IS_NOT_AN_ARRAY',
    }))

    expect(() => createProjectConfigRecord({
      projectSettings: [{
        match: true as unknown as string[],
        settings: {},
      }],
    })).toThrow(expect.objectContaining({
      match: true,
      code: 'ERR_PNPM_PROJECT_SETTINGS_ARRAY_ITEM_MATCH_IS_NOT_AN_ARRAY',
    }))

    expect(() => createProjectConfigRecord({
      projectSettings: [{
        match: undefined as unknown as string[],
        settings: {},
      }],
    })).toThrow(expect.objectContaining({
      match: undefined,
      code: 'ERR_PNPM_PROJECT_SETTINGS_ARRAY_ITEM_MATCH_IS_NOT_AN_ARRAY',
    }))

    expect(() => createProjectConfigRecord({
      projectSettings: [{
        match: null as unknown as string[],
        settings: {},
      }],
    })).toThrow(expect.objectContaining({
      match: null,
      code: 'ERR_PNPM_PROJECT_SETTINGS_ARRAY_ITEM_MATCH_IS_NOT_AN_ARRAY',
    }))
  })

  it('errors on non-string match item', () => {
    expect(() => createProjectConfigRecord({
      projectSettings: [{
        match: [0 as unknown as string],
        settings: {},
      }],
    })).toThrow(expect.objectContaining({
      matchItem: 0,
      code: 'ERR_PNPM_PROJECT_SETTINGS_MATCH_ITEM_IS_NOT_A_STRING',
    }))

    expect(() => createProjectConfigRecord({
      projectSettings: [{
        match: [null as unknown as string],
        settings: {},
      }],
    })).toThrow(expect.objectContaining({
      matchItem: null,
      code: 'ERR_PNPM_PROJECT_SETTINGS_MATCH_ITEM_IS_NOT_A_STRING',
    }))

    expect(() => createProjectConfigRecord({
      projectSettings: [{
        match: [{} as unknown as string],
        settings: {},
      }],
    })).toThrow(expect.objectContaining({
      matchItem: {},
      code: 'ERR_PNPM_PROJECT_SETTINGS_MATCH_ITEM_IS_NOT_A_STRING',
    }))
  })

  it('errors on invalid project config', () => {
    expect(() => createProjectConfigRecord({
      projectSettings: [{
        match: [],
        settings: 0 as unknown as ProjectConfig,
      }],
    })).toThrow(expect.objectContaining({
      actualRawConfig: 0,
      code: 'ERR_PNPM_PROJECT_CONFIG_NOT_AN_OBJECT',
    }))
  })

  it('errors on invalid hoist', () => {
    expect(() => createProjectConfigRecord({
      projectSettings: [{
        match: ['project-1'],
        settings: { hoist: 'invalid' as unknown as boolean },
      }],
    })).toThrow(expect.objectContaining({
      expectedType: 'boolean',
      actualValue: 'invalid',
      code: 'ERR_PNPM_PROJECT_CONFIG_INVALID_VALUE_TYPE',
    }))

    expect(() => createProjectConfigRecord({
      projectSettings: [{
        match: ['project-1'],
        settings: { hoist: 0 as unknown as boolean },
      }],
    })).toThrow(expect.objectContaining({
      expectedType: 'boolean',
      actualValue: 0,
      code: 'ERR_PNPM_PROJECT_CONFIG_INVALID_VALUE_TYPE',
    }))
  })

  it('errors on invalid modulesDir', () => {
    expect(() => createProjectConfigRecord({
      projectSettings: [{
        match: ['project-1'],
        settings: { modulesDir: 0 as unknown as string },
      }],
    })).toThrow(expect.objectContaining({
      expectedType: 'string',
      actualValue: 0,
      code: 'ERR_PNPM_PROJECT_CONFIG_INVALID_VALUE_TYPE',
    }))

    expect(() => createProjectConfigRecord({
      projectSettings: [{
        match: ['project-1'],
        settings: { modulesDir: true as unknown as string },
      }],
    })).toThrow(expect.objectContaining({
      expectedType: 'string',
      actualValue: true,
      code: 'ERR_PNPM_PROJECT_CONFIG_INVALID_VALUE_TYPE',
    }))
  })

  it('errors on invalid saveExact', () => {
    expect(() => createProjectConfigRecord({
      projectSettings: [{
        match: ['project-1'],
        settings: { saveExact: 'invalid' as unknown as boolean },
      }],
    })).toThrow(expect.objectContaining({
      expectedType: 'boolean',
      actualValue: 'invalid',
      code: 'ERR_PNPM_PROJECT_CONFIG_INVALID_VALUE_TYPE',
    }))

    expect(() => createProjectConfigRecord({
      projectSettings: [{
        match: ['project-1'],
        settings: { saveExact: 0 as unknown as boolean },
      }],
    })).toThrow(expect.objectContaining({
      expectedType: 'boolean',
      actualValue: 0,
      code: 'ERR_PNPM_PROJECT_CONFIG_INVALID_VALUE_TYPE',
    }))
  })

  it('errors on invalid savePrefix', () => {
    expect(() => createProjectConfigRecord({
      projectSettings: [{
        match: ['project-1'],
        settings: { savePrefix: 0 as unknown as string },
      }],
    })).toThrow(expect.objectContaining({
      expectedType: 'string',
      actualValue: 0,
      code: 'ERR_PNPM_PROJECT_CONFIG_INVALID_VALUE_TYPE',
    }))

    expect(() => createProjectConfigRecord({
      projectSettings: [{
        match: ['project-1'],
        settings: { savePrefix: false as unknown as string },
      }],
    })).toThrow(expect.objectContaining({
      expectedType: 'string',
      actualValue: false,
      code: 'ERR_PNPM_PROJECT_CONFIG_INVALID_VALUE_TYPE',
    }))
  })

  it('errors on unsupported fields', () => {
    expect(() => createProjectConfigRecord({
      projectSettings: [{
        match: ['project-1'],
        settings: {
          ignoreScripts: true,
        } as Partial<Config>,
      }],
    })).toThrow(expect.objectContaining({
      field: 'ignoreScripts',
      code: 'ERR_PNPM_PROJECT_CONFIG_UNSUPPORTED_FIELD',
    }))

    expect(() => createProjectConfigRecord({
      projectSettings: [{
        match: ['project-1'],
        settings: {
          hoistPattern: ['*'],
        } as Partial<Config>,
      }],
    })).toThrow(expect.objectContaining({
      field: 'hoistPattern',
      code: 'ERR_PNPM_PROJECT_CONFIG_UNSUPPORTED_FIELD',
    }))
  })

  it('does not error on unsupported but undefined fields', () => {
    expect(createProjectConfigRecord({
      projectSettings: [{
        match: ['project-1'],
        settings: {
          ignoreScripts: undefined,
          hoistPattern: undefined,
        } as Partial<Config>,
      }],
    })).toStrictEqual({
      'project-1': {
        ignoreScripts: undefined,
        hoistPattern: undefined,
      } as Partial<Config>,
    })
  })
})
