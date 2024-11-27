import path from 'path'
import { assertProject } from '@pnpm/assert-project'
import { filterPackagesFromDir } from '@pnpm/workspace.filter-packages-from-dir'
import { rebuild } from '@pnpm/plugin-commands-rebuild'
import { preparePackages } from '@pnpm/prepare'
import { createTestIpcServer } from '@pnpm/test-ipc-server'
import { type PackageManifest } from '@pnpm/types'
import execa from 'execa'
import { sync as writeYamlFile } from 'write-yaml-file'
import { DEFAULT_OPTS, REGISTRY } from './utils'

const pnpmBin = path.join(__dirname, '../../../pnpm/bin/pnpm.cjs')

test('pnpm recursive rebuild', async () => {
  const projects = preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        '@pnpm.e2e/pre-and-postinstall-scripts-example': '*',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        '@pnpm.e2e/pre-and-postinstall-scripts-example': '*',
      },
    },
  ])

  const { allProjects, selectedProjectsGraph } = await filterPackagesFromDir(process.cwd(), [])
  await execa('node', [
    pnpmBin,
    'install',
    '-r',
    `--registry=${REGISTRY}`,
    `--store-dir=${path.resolve(DEFAULT_OPTS.storeDir)}`,
    `--cache-dir=${path.resolve(DEFAULT_OPTS.cacheDir)}`,
    '--ignore-scripts',
    '--reporter=append-only',
  ], { stdout: 'inherit' })

  projects['project-1'].hasNot('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')
  projects['project-1'].hasNot('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')
  projects['project-2'].hasNot('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')
  projects['project-2'].hasNot('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')

  const modulesManifest = projects['project-1'].readModulesManifest()
  await rebuild.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    registries: modulesManifest!.registries!,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  }, [])

  projects['project-1'].has('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')
  projects['project-1'].has('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')
  projects['project-2'].has('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')
  projects['project-2'].has('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')
})

test('pnpm recursive rebuild with hoisted node linker', async () => {
  const projects = preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        '@pnpm.e2e/pre-and-postinstall-scripts-example': '1',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        '@pnpm.e2e/pre-and-postinstall-scripts-example': '1',
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',

      dependencies: {
        '@pnpm.e2e/pre-and-postinstall-scripts-example': '2',
      },
    },
    {
      name: 'project-4',
      version: '1.0.0',

      dependencies: {
        '@pnpm.e2e/pre-and-postinstall-scripts-example': '2',
      },
    },
  ])

  const { allProjects, selectedProjectsGraph } = await filterPackagesFromDir(process.cwd(), [])
  writeYamlFile('pnpm-workspace.yaml', { packages: ['*'] })
  await execa('node', [
    pnpmBin,
    'install',
    '-r',
    `--registry=${REGISTRY}`,
    `--store-dir=${path.resolve(DEFAULT_OPTS.storeDir)}`,
    `--cache-dir=${path.resolve(DEFAULT_OPTS.cacheDir)}`,
    '--ignore-scripts',
    '--reporter=append-only',
    '--config.node-linker=hoisted',
  ], { stdout: 'inherit' })

  const rootProject = assertProject(process.cwd())
  rootProject.hasNot('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')
  rootProject.hasNot('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')
  projects['project-3'].hasNot('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')
  projects['project-3'].hasNot('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')
  projects['project-4'].hasNot('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')
  projects['project-4'].hasNot('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')

  const modulesManifest = rootProject.readModulesManifest()
  await rebuild.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    nodeLinker: 'hoisted',
    recursive: true,
    registries: modulesManifest!.registries!,
    selectedProjectsGraph,
    lockfileDir: process.cwd(),
    workspaceDir: process.cwd(),
  }, [])

  rootProject.has('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')
  rootProject.has('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')
  projects['project-1'].hasNot('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')
  projects['project-1'].hasNot('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')
  projects['project-2'].hasNot('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')
  projects['project-2'].hasNot('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')
  projects['project-3'].has('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')
  projects['project-3'].has('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')
  projects['project-4'].has('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')
  projects['project-4'].has('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')
})

test('rebuild multiple packages in correct order', async () => {
  await using server1 = await createTestIpcServer()
  await using server2 = await createTestIpcServer()

  const pkgs: Array<PackageManifest & { name: string }> = [
    {
      name: 'project-1',
      version: '1.0.0',

      scripts: {
        postinstall: `${server1.sendLineScript('project-1')} && ${server2.sendLineScript('project-1')}`,
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'project-1': '1',
      },
      scripts: {
        postinstall: server1.sendLineScript('project-2'),
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',

      dependencies: {
        'project-1': '1',
      },
      scripts: {
        postinstall: server2.sendLineScript('project-3'),
      },
    },
    {
      name: 'project-0',
      version: '1.0.0',

      dependencies: {},
    },
  ]
  preparePackages(pkgs)
  writeYamlFile('pnpm-workspace.yaml', { packages: pkgs.map(pkg => pkg.name) })

  const { allProjects, selectedProjectsGraph } = await filterPackagesFromDir(process.cwd(), [])
  await execa('node', [
    pnpmBin,
    'install',
    '-r',
    '--registry',
    REGISTRY,
    '--store-dir',
    path.resolve(DEFAULT_OPTS.storeDir),
    '--ignore-scripts',
  ])

  await rebuild.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  }, [])

  expect(server1.getLines()).toStrictEqual(['project-1', 'project-2'])
  expect(server2.getLines()).toStrictEqual(['project-1', 'project-3'])
})

test('never build neverBuiltDependencies', async () => {
  const projects = preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        '@pnpm.e2e/install-script-example': '*',
        '@pnpm.e2e/pre-and-postinstall-scripts-example': '*',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        '@pnpm.e2e/install-script-example': '*',
        '@pnpm.e2e/pre-and-postinstall-scripts-example': '*',
      },
    },
  ])

  const { allProjects, selectedProjectsGraph } = await filterPackagesFromDir(
    process.cwd(),
    []
  )
  await execa(
    'node',
    [
      pnpmBin,
      'install',
      '-r',
      `--registry=${REGISTRY}`,
      `--store-dir=${path.resolve(DEFAULT_OPTS.storeDir)}`,
      `--cache-dir=${path.resolve(DEFAULT_OPTS.cacheDir)}`,
      '--ignore-scripts',
      '--reporter=append-only',
    ],
    { stdout: 'inherit' }
  )
  projects['project-1'].hasNot(
    '@pnpm.e2e/install-script-example/generated-by-install.js'
  )
  projects['project-2'].hasNot(
    '@pnpm.e2e/install-script-example/generated-by-install.js'
  )
  projects['project-1'].hasNot(
    '@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js'
  )
  projects['project-1'].hasNot(
    '@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js'
  )
  projects['project-2'].hasNot(
    '@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js'
  )
  projects['project-2'].hasNot(
    '@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js'
  )

  const modulesManifest = projects['project-1'].readModulesManifest()
  await rebuild.handler(
    {
      ...DEFAULT_OPTS,
      neverBuiltDependencies: ['@pnpm.e2e/pre-and-postinstall-scripts-example'],
      allProjects,
      dir: process.cwd(),
      recursive: true,
      registries: modulesManifest!.registries!,
      selectedProjectsGraph,
      workspaceDir: process.cwd(),
    },
    []
  )

  projects['project-1'].has(
    '@pnpm.e2e/install-script-example/generated-by-install.js'
  )
  projects['project-2'].has(
    '@pnpm.e2e/install-script-example/generated-by-install.js'
  )
  projects['project-1'].hasNot(
    '@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js'
  )
  projects['project-1'].hasNot(
    '@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js'
  )
  projects['project-2'].hasNot(
    '@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js'
  )
  projects['project-2'].hasNot(
    '@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js'
  )
})

test('only build onlyBuiltDependencies', async () => {
  const projects = preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        '@pnpm.e2e/install-script-example': '*',
        '@pnpm.e2e/pre-and-postinstall-scripts-example': '*',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        '@pnpm.e2e/install-script-example': '*',
        '@pnpm.e2e/pre-and-postinstall-scripts-example': '*',
      },
    },
  ])

  const { allProjects, selectedProjectsGraph } = await filterPackagesFromDir(
    process.cwd(),
    []
  )
  await execa(
    'node',
    [
      pnpmBin,
      'install',
      '-r',
      `--registry=${REGISTRY}`,
      `--store-dir=${path.resolve(DEFAULT_OPTS.storeDir)}`,
      `--cache-dir=${path.resolve(DEFAULT_OPTS.cacheDir)}`,
      '--ignore-scripts',
      '--reporter=append-only',
    ],
    { stdout: 'inherit' }
  )
  projects['project-1'].hasNot(
    '@pnpm.e2e/install-script-example/generated-by-install.js'
  )
  projects['project-2'].hasNot(
    '@pnpm.e2e/install-script-example/generated-by-install.js'
  )
  projects['project-1'].hasNot(
    '@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js'
  )
  projects['project-1'].hasNot(
    '@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js'
  )
  projects['project-2'].hasNot(
    '@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js'
  )
  projects['project-2'].hasNot(
    '@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js'
  )

  const modulesManifest = projects['project-1'].readModulesManifest()
  await rebuild.handler(
    {
      ...DEFAULT_OPTS,
      onlyBuiltDependencies: ['@pnpm.e2e/pre-and-postinstall-scripts-example'],
      allProjects,
      dir: process.cwd(),
      recursive: true,
      registries: modulesManifest!.registries!,
      selectedProjectsGraph,
      workspaceDir: process.cwd(),
    },
    []
  )

  projects['project-1'].hasNot(
    '@pnpm.e2e/install-script-example/generated-by-install.js'
  )
  projects['project-2'].hasNot(
    '@pnpm.e2e/install-script-example/generated-by-install.js'
  )
  projects['project-1'].has(
    '@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js'
  )
  projects['project-1'].has(
    '@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js'
  )
  projects['project-2'].has(
    '@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js'
  )
  projects['project-2'].has(
    '@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js'
  )
})
