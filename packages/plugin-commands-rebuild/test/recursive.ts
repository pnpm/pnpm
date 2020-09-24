import { readProjects } from '@pnpm/filter-workspace-packages'
import { rebuild } from '@pnpm/plugin-commands-rebuild'
import { preparePackages } from '@pnpm/prepare'
import { PackageManifest } from '@pnpm/types'
import { DEFAULT_OPTS, REGISTRY } from './utils'
import path = require('path')
import execa = require('execa')
import test = require('tape')
import writeYamlFile = require('write-yaml-file')

const pnpmBin = path.join(__dirname, '../../pnpm/bin/pnpm.js')

test('pnpm recursive rebuild', async (t) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'pre-and-postinstall-scripts-example': '*',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'pre-and-postinstall-scripts-example': '*',
      },
    },
  ])

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

  await projects['project-1'].hasNot('pre-and-postinstall-scripts-example/generated-by-preinstall.js')
  await projects['project-1'].hasNot('pre-and-postinstall-scripts-example/generated-by-postinstall.js')
  await projects['project-2'].hasNot('pre-and-postinstall-scripts-example/generated-by-preinstall.js')
  await projects['project-2'].hasNot('pre-and-postinstall-scripts-example/generated-by-postinstall.js')

  await rebuild.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  }, [])

  await projects['project-1'].has('pre-and-postinstall-scripts-example/generated-by-preinstall.js')
  await projects['project-1'].has('pre-and-postinstall-scripts-example/generated-by-postinstall.js')
  await projects['project-2'].has('pre-and-postinstall-scripts-example/generated-by-preinstall.js')
  await projects['project-2'].has('pre-and-postinstall-scripts-example/generated-by-postinstall.js')
  t.end()
})

// TODO: make this test pass
test.skip('rebuild multiple packages in correct order', async (t) => {
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
  preparePackages(t, pkgs)
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

  t.deepEqual(outputs1, ['project-1', 'project-2'])
  t.deepEqual(outputs2, ['project-1', 'project-3'])
  t.end()
})
