import path from 'path'
import { preparePackages } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { update, install } from '@pnpm/plugin-commands-installation'
import * as enquirer from 'enquirer'
import { filterPackagesFromDir } from '@pnpm/workspace.filter-packages-from-dir'

jest.mock('enquirer', () => ({ prompt: jest.fn() }))

// eslint-disable-next-line
const prompt = enquirer.prompt as any as jest.Mock<any, any, any>

const REGISTRY_URL = `http://localhost:${REGISTRY_MOCK_PORT}`

const DEFAULT_OPTIONS = {
  argv: {
    original: [],
  },
  bail: false,
  bin: 'node_modules/.bin',
  extraEnv: {},
  cliOptions: {},
  deployAllFiles: false,
  include: {
    dependencies: true,
    devDependencies: true,
    optionalDependencies: true,
  },
  lock: true,
  pnpmfile: '.pnpmfile.cjs',
  pnpmHomeDir: '',
  rawConfig: { registry: REGISTRY_URL },
  rawLocalConfig: { registry: REGISTRY_URL },
  registries: {
    default: REGISTRY_URL,
  },
  rootProjectManifestDir: '',
  sort: true,
  userConfig: {},
  workspaceConcurrency: 1,
  virtualStoreDirMaxLength: 120,
}

test('interactive recursive should not error on git specifier override', async () => {
  preparePackages([
    {
      location: '.',
      package: {
        pnpm: {
          overrides: {
            'is-negative': 'github:kevva/is-negative#2.1.0',
          },
        },
      },
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

  prompt.mockResolvedValue({
    updateDependencies: [],
  })

  const { allProjects, selectedProjectsGraph } = await filterPackagesFromDir(process.cwd(), [])
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
