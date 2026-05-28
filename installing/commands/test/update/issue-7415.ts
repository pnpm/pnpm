import path from 'node:path'

import { jest, test } from '@jest/globals'
import { preparePackages } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/testing.registry-mock'
import { filterProjectsBySelectorObjectsFromDir } from '@pnpm/workspace.projects-filter'

jest.unstable_mockModule('@inquirer/prompts', () => {
  class Separator {
    separator: string
    readonly type = 'separator' as const
    constructor (separator: string) {
      this.separator = separator
    }
  }
  return {
    Separator,
    checkbox: jest.fn(),
    confirm: jest.fn(),
    input: jest.fn(),
    password: jest.fn(),
    select: jest.fn(),
  }
})

const { checkbox } = await import('@inquirer/prompts')
const { update, install } = await import('@pnpm/installing.commands')

const mockCheckbox = jest.mocked(checkbox)

const REGISTRY_URL = `http://localhost:${REGISTRY_MOCK_PORT}`

const DEFAULT_OPTIONS = {
  argv: {
    original: [],
  },
  bail: false,
  bin: 'node_modules/.bin',
  excludeLinksFromLockfile: false,
  extraEnv: {},
  cliOptions: {},
  deployAllFiles: false,
  include: {
    dependencies: true,
    devDependencies: true,
    optionalDependencies: true,
  },
  lock: true,
  pnpmfile: ['.pnpmfile.cjs'],
  pnpmHomeDir: '',
  preferWorkspacePackages: true,
  configByUri: {},
  registries: {
    default: REGISTRY_URL,
  },
  rootProjectManifestDir: '',
  sort: true,
  userConfig: {},
  workspaceConcurrency: 1,
  virtualStoreDirMaxLength: process.platform === 'win32' ? 60 : 120,
}

test('interactive recursive should not error on git specifier override', async () => {
  preparePackages([
    {
      location: '.',
      package: {},
    },
    {
      location: './project-1',
      package: {
        dependencies: {
          'is-negative': '2.1.0',
        },
      },
    },
  ])

  mockCheckbox.mockResolvedValue([])

  const { allProjects, selectedProjectsGraph } = await filterProjectsBySelectorObjectsFromDir(process.cwd(), [])
  const sharedOptions = {
    ...DEFAULT_OPTIONS,
    allProjects,
    selectedProjectsGraph,
    recursive: true,
    linkWorkspacePackages: true,
    cacheDir: path.resolve('cache'),
    storeDir: path.resolve('store'),
    dir: process.cwd(),
    lockfileDir: process.cwd(),
    workspaceDir: process.cwd(),
    overrides: {
      'is-negative': 'github:kevva/is-negative#2.1.0',
    },
  }

  await install.handler({
    ...sharedOptions,
  })
  await update.handler({
    ...sharedOptions,
    interactive: true,
    latest: true,
    cliOptions: {
      dev: true,
      optional: true,
      production: true,
    },
  })
})
