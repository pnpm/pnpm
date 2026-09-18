import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { prepare, preparePackages } from '@pnpm/prepare'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpm, execPnpmSync } from './utils/index.js'

test('recursive JSON combines projects with separate lockfiles', async () => {
  preparePackages([
    {
      location: 'packages/project-1',
      package: { name: 'project-1', version: '1.0.0', dependencies: { '@pnpm.e2e/pkg-with-1-dep': '100.0.0' } },
    },
    {
      location: 'packages/project-2',
      package: { name: 'project-2', version: '1.0.0', dependencies: { '@pnpm.e2e/hello-world-js-bin': '1.0.0' } },
    },
  ])
  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['packages/*'],
    sharedWorkspaceLockfile: false,
  })
  await execPnpm(['install'])

  const { stdout } = execPnpmSync(['-r', '--filter', 'project-*', 'list', '--json'], { expectSuccess: true })
  expect(JSON.parse(stdout.toString())).toMatchObject([
    {
      name: 'project-1',
      path: fs.realpathSync('packages/project-1'),
      dependencies: { '@pnpm.e2e/pkg-with-1-dep': { version: '100.0.0' } },
    },
    {
      name: 'project-2',
      path: fs.realpathSync('packages/project-2'),
      dependencies: { '@pnpm.e2e/hello-world-js-bin': { version: '1.0.0' } },
    },
  ])

  const projectOnly = execPnpmSync(['-r', '--filter', 'project-*', 'list', '--json', '--depth', '-1'], { expectSuccess: true })
  expect(JSON.parse(projectOnly.stdout.toString())).toStrictEqual([
    { name: 'project-1', version: '1.0.0', path: fs.realpathSync('packages/project-1'), private: false },
    { name: 'project-2', version: '1.0.0', path: fs.realpathSync('packages/project-2'), private: false },
  ])

  const single = execPnpmSync(['-r', '--filter', 'project-2', 'list', '--json', '--long'], { expectSuccess: true })
  expect(JSON.parse(single.stdout.toString())).toMatchObject([
    {
      name: 'project-2',
      dependencies: {
        '@pnpm.e2e/hello-world-js-bin': {
          version: '1.0.0',
          description: 'A package with a hello world js bin',
        },
      },
    },
  ])

  const search = execPnpmSync(['-r', '--filter', 'project-*', 'list', '@pnpm.e2e/pkg-with-1-dep', '--json'], { expectSuccess: true })
  const searchedProjects = JSON.parse(search.stdout.toString())
  expect(searchedProjects).toHaveLength(2)
  expect(searchedProjects[0].dependencies).toHaveProperty(['@pnpm.e2e/pkg-with-1-dep'])
  expect(searchedProjects[1]).not.toHaveProperty('dependencies')

  const parseable = execPnpmSync(['-r', '--filter', 'project-*', 'list', '--parseable'], { expectSuccess: true })
  const bothFormats = execPnpmSync(['-r', '--filter', 'project-*', 'list', '--parseable', '--json'], { expectSuccess: true })
  expect(bothFormats.stdout.toString()).toBe(parseable.stdout.toString())
})

test('recursive list uses each project modules directory in every output format', async () => {
  const dependency = '@pnpm.e2e/hello-world-js-bin'
  preparePackages(['project-1', 'project-2'].map((name) => ({
    location: `packages/${name}`,
    package: { name, version: '1.0.0', dependencies: { [dependency]: '1.0.0' } },
  })))
  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['packages/*'],
    sharedWorkspaceLockfile: false,
    packageConfigs: { 'project-1': { modulesDir: 'custom_modules' } },
  })
  await execPnpm(['install'])

  const packagePaths = [
    fs.realpathSync(`packages/project-1/custom_modules/${dependency}`),
    fs.realpathSync(`packages/project-2/node_modules/${dependency}`),
  ]
  const json = execPnpmSync(['-r', '--filter', 'project-*', 'list', '--json', '--long'], { expectSuccess: true })
  const projects = JSON.parse(json.stdout.toString())
  expect(projects).toHaveLength(2)
  expect(projects.map((project: { dependencies: Record<string, { path: string }> }) => fs.realpathSync(project.dependencies[dependency].path))).toEqual(packagePaths)
  expect(projects).toMatchObject(packagePaths.map(() => ({
    dependencies: { [dependency]: { description: 'A package with a hello world js bin' } },
  })))

  const long = execPnpmSync(['-r', '--filter', 'project-*', 'list', '--long'], { expectSuccess: true })
  expect(long.stdout.toString().match(/A package with a hello world js bin/g)).toHaveLength(2)

  const parseable = execPnpmSync(['-r', '--filter', 'project-*', 'list', '--parseable'], { expectSuccess: true })
  expect(parseable.stdout.toString().trim().split(/\r?\n/).filter(Boolean).map((value) => fs.realpathSync(value))).toEqual([
    fs.realpathSync('packages/project-1'), packagePaths[0],
    fs.realpathSync('packages/project-2'), packagePaths[1],
  ])
})

test('ls --filter=not-exist --json should prints an empty array (#9672)', async () => {
  preparePackages([
    {
      location: 'packages/foo',
      package: {
        name: 'foo',
        version: '0.0.0',
        private: true,
      },
    },
  ])

  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['packages/*'],
  })

  const { stdout } = execPnpmSync(['ls', '--filter=project-that-does-not-exist', '--json'], { expectSuccess: true })
  expect(JSON.parse(stdout.toString())).toStrictEqual([])
})

test('ls should load a finder from .pnpmfile.cjs', async () => {
  prepare()
  const pnpmfile = `
module.exports = { finders: { hasPeerA } }
function hasPeerA (context) {
  const manifest = context.readManifest()
  if (manifest?.peerDependencies?.['@pnpm.e2e/peer-a'] == null) {
    return false
  }
  return \`@pnpm.e2e/peer-a@$\{manifest.peerDependencies['@pnpm.e2e/peer-a']}\`
}
`
  fs.writeFileSync('.pnpmfile.cjs', pnpmfile, 'utf8')
  await execPnpm(['add', 'is-positive@1.0.0', '@pnpm.e2e/abc@1.0.0'])
  const result = execPnpmSync(['list', '--find-by=hasPeerA'])
  expect(result.stdout.toString()).toMatch('@pnpm.e2e/abc@1.0.0')
  expect(result.stdout.toString()).toMatch('@pnpm.e2e/peer-a@^1.0.0')
})

test('pnpm list returns correct paths with global virtual store', async () => {
  prepare({
    dependencies: {
      '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
    },
  })
  writeYamlFileSync('pnpm-workspace.yaml', {
    enableGlobalVirtualStore: true,
    storeDir: path.resolve('store'),
    privateHoistPattern: '*',
  })
  await execPnpm(['install'])

  const { stdout } = execPnpmSync(['list', '--json', '--depth=Infinity'])
  const listResult = JSON.parse(stdout.toString())

  // pnpm list should return the same path as resolving the symlink
  const pkgPath = listResult[0].dependencies['@pnpm.e2e/pkg-with-1-dep'].path
  expect(pkgPath).toBe(fs.realpathSync('node_modules/@pnpm.e2e/pkg-with-1-dep'))

  // Subdependency path should also be a valid resolved path
  const subDepPath = listResult[0].dependencies['@pnpm.e2e/pkg-with-1-dep'].dependencies['@pnpm.e2e/dep-of-pkg-with-1-dep'].path
  expect(fs.existsSync(subDepPath)).toBe(true)
  expect(fs.existsSync(path.join(subDepPath, 'package.json'))).toBe(true)
})
