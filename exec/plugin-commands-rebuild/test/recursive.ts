import path from 'path'
import { assertProject } from '@pnpm/assert-project'
import { readProjects } from '@pnpm/filter-workspace-packages'
import { rebuild } from '@pnpm/plugin-commands-rebuild'
import { preparePackages } from '@pnpm/prepare'
import { type PackageManifest } from '@pnpm/types'
import execa from 'execa'
import writeYamlFile from 'write-yaml-file'
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

  const { allProjects, selectedProjectsGraph } = await readProjects(process.cwd(), [])
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

  await projects['project-1'].hasNot('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')
  await projects['project-1'].hasNot('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')
  await projects['project-2'].hasNot('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')
  await projects['project-2'].hasNot('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')

  const modulesManifest = await projects['project-1'].readModulesManifest()
  await rebuild.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    registries: modulesManifest!.registries!,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  }, [])

  await projects['project-1'].has('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')
  await projects['project-1'].has('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')
  await projects['project-2'].has('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')
  await projects['project-2'].has('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')
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

  const { allProjects, selectedProjectsGraph } = await readProjects(process.cwd(), [])
  await writeYamlFile('pnpm-workspace.yaml', { packages: ['*'] })
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
  await rootProject.hasNot('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')
  await rootProject.hasNot('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')
  await projects['project-3'].hasNot('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')
  await projects['project-3'].hasNot('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')
  await projects['project-4'].hasNot('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')
  await projects['project-4'].hasNot('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')

  const modulesManifest = await rootProject.readModulesManifest()
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

  await rootProject.has('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')
  await rootProject.has('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')
  await projects['project-1'].hasNot('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')
  await projects['project-1'].hasNot('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')
  await projects['project-2'].hasNot('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')
  await projects['project-2'].hasNot('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')
  await projects['project-3'].has('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')
  await projects['project-3'].has('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')
  await projects['project-4'].has('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')
  await projects['project-4'].has('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')
})

// TODO: make this test pass
test.skip('rebuild multiple packages in correct order', async () => {
  const pkgs = [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'json-append': '1',
      },
      scripts: {
        postinstall: 'node -e "process.stdout.write(\'project-1\')" | json-append ../output1.json && node -e "process.stdout.write(\'project-1\')" | json-append ../output2.json',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'json-append': '1',
        'project-1': '1',
      },
      scripts: {
        postinstall: 'node -e "process.stdout.write(\'project-2\')" | json-append ../output1.json',
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',

      dependencies: {
        'json-append': '1',
        'project-1': '1',
      },
      scripts: {
        postinstall: 'node -e "process.stdout.write(\'project-3\')" | json-append ../output2.json',
      },
    },
    {
      name: 'project-0',
      version: '1.0.0',

      dependencies: {},
    },
  ] as PackageManifest[]
  preparePackages(pkgs)
  await writeYamlFile('pnpm-workspace.yaml', { packages: ['project-1'] })

  const { allProjects, selectedProjectsGraph } = await readProjects(process.cwd(), [])
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

  const outputs1 = await import(path.resolve('output1.json')) as string[]
  const outputs2 = await import(path.resolve('output2.json')) as string[]

  expect(outputs1).toStrictEqual(['project-1', 'project-2'])
  expect(outputs2).toStrictEqual(['project-1', 'project-3'])
})
