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

  const questionMark = execPnpmSync(['-r', '--filter', 'project-?', 'list', '--json', '--depth', '-1'], { expectSuccess: true })
  expect(JSON.parse(questionMark.stdout.toString())).toMatchObject([
    { name: 'project-1' },
    { name: 'project-2' },
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

test('ls inside a workspace package outputs information for that package (pnpm/pnpm#14494)', async () => {
  preparePackages([
    {
      location: '.',
      package: {
        name: 'root',
        version: '1.0.0',
        private: true,
      },
    },
    {
      location: 'packages/foo',
      package: {
        name: 'foo',
        version: '1.0.0',
        dependencies: { '@pnpm.e2e/pkg-with-1-dep': '100.0.0' },
      },
    },
    {
      location: 'packages/bar',
      package: {
        name: 'bar',
        version: '1.0.0',
        dependencies: { '@pnpm.e2e/hello-world-js-bin': '1.0.0' },
      },
    },
  ])
  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['packages/*'],
  })
  await execPnpm(['install'])

  const initialCwd = process.cwd()
  try {
    process.chdir('packages/foo')
    const { stdout } = execPnpmSync(['ls', '--json'])
    const projects = JSON.parse(stdout.toString())
    expect(projects).toHaveLength(1)
    expect(projects[0].name).toBe('foo')

    const textOutput = execPnpmSync(['ls']).stdout.toString()
    expect(textOutput).toContain('foo@1.0.0')
    expect(textOutput).toContain('@pnpm.e2e/pkg-with-1-dep')
    expect(textOutput).not.toContain('bar@1.0.0')

    const recursiveInPkg = JSON.parse(execPnpmSync(['ls', '-r', '--json']).stdout.toString())
    expect(recursiveInPkg).toHaveLength(3)

    const filteredInPkg = JSON.parse(execPnpmSync(['ls', '--filter', 'bar', '--json']).stdout.toString())
    expect(filteredInPkg).toHaveLength(1)
    expect(filteredInPkg[0].name).toBe('bar')
  } finally {
    process.chdir(initialCwd)
  }

  const rootList = JSON.parse(execPnpmSync(['ls', '--json']).stdout.toString())
  expect(rootList).toHaveLength(3)
})

test('root-level list with symlinked workspace directory remains recursive (pnpm/pnpm#14494)', async () => {
  preparePackages([
    {
      location: '.',
      package: {
        name: 'root',
        version: '1.0.0',
        private: true,
      },
    },
    {
      location: 'packages/foo',
      package: {
        name: 'foo',
        version: '1.0.0',
        dependencies: { '@pnpm.e2e/pkg-with-1-dep': '100.0.0' },
      },
    },
    {
      location: 'packages/bar',
      package: {
        name: 'bar',
        version: '1.0.0',
        dependencies: { '@pnpm.e2e/hello-world-js-bin': '1.0.0' },
      },
    },
  ])
  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['packages/*'],
  })
  await execPnpm(['install'])

  const workspaceRealDir = fs.realpathSync.native(process.cwd())
  const symlinkWorkspaceDir = path.resolve('..', `${path.basename(process.cwd())}-symlink`)
  fs.symlinkSync(workspaceRealDir, symlinkWorkspaceDir, 'junction')

  try {
    const rootListWithEnv = JSON.parse(execPnpmSync(['ls', '--json'], {
      env: {
        NPM_CONFIG_WORKSPACE_DIR: symlinkWorkspaceDir,
      },
    }).stdout.toString())
    expect(rootListWithEnv).toHaveLength(3)

    const initialCwd = process.cwd()
    try {
      process.chdir(symlinkWorkspaceDir)
      const listFromSymlinkCwd = JSON.parse(execPnpmSync(['ls', '--json']).stdout.toString())
      expect(listFromSymlinkCwd).toHaveLength(3)
    } finally {
      process.chdir(initialCwd)
    }
  } finally {
    fs.rmSync(symlinkWorkspaceDir, { force: true, recursive: true })
  }
})

test('configured filter applies when listing from workspace subdirectory (pnpm/pnpm#14494)', async () => {
  preparePackages([
    {
      location: '.',
      package: {
        name: 'root',
        version: '1.0.0',
        private: true,
      },
    },
    {
      location: 'packages/foo',
      package: {
        name: 'foo',
        version: '1.0.0',
        dependencies: { '@pnpm.e2e/pkg-with-1-dep': '100.0.0' },
      },
    },
    {
      location: 'packages/bar',
      package: {
        name: 'bar',
        version: '1.0.0',
        dependencies: { '@pnpm.e2e/hello-world-js-bin': '1.0.0' },
      },
    },
  ])
  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['packages/*'],
  })
  await execPnpm(['install'])

  const initialCwd = process.cwd()
  try {
    process.chdir('packages/foo')
    const { stdout } = execPnpmSync(['ls', '--json'], {
      env: {
        pnpm_config_filter: 'bar',
      },
    })
    const projects = JSON.parse(stdout.toString())
    expect(projects).toHaveLength(1)
    expect(projects[0].name).toBe('bar')
  } finally {
    process.chdir(initialCwd)
  }
})

test('fails with WORKSPACE_DIR_NOT_FOUND when workspace directory does not exist', () => {
  prepare()
  const result = execPnpmSync(['ls'], {
    env: {
      NPM_CONFIG_WORKSPACE_DIR: path.resolve('non-existent-workspace-dir'),
    },
  })
  expect(result.status).toBe(1)
  expect(result.stdout.toString()).toContain('ERR_PNPM_WORKSPACE_DIR_NOT_FOUND')
})

test('list shows correct package path when nodeLinker is hoisted', async () => {
  prepare({
    dependencies: {
      '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
    },
  })
  await execPnpm(['install', '--config.node-linker=hoisted'])

  const { stdout } = execPnpmSync(['list', '--json', '--depth', '1'])
  const result = JSON.parse(stdout.toString())
  expect(result[0].dependencies['@pnpm.e2e/pkg-with-1-dep'].path).toBe(
    path.resolve('node_modules/@pnpm.e2e/pkg-with-1-dep')
  )
  expect(
    result[0].dependencies['@pnpm.e2e/pkg-with-1-dep'].dependencies['@pnpm.e2e/dep-of-pkg-with-1-dep'].path
  ).toBe(
    path.resolve('node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep')
  )
})

test('why from workspace root without -r inspects projects with separate lockfiles', async () => {
  preparePackages([
    {
      location: 'packages/project-1',
      package: { name: 'project-1', version: '1.0.0', dependencies: { '@pnpm.e2e/pkg-with-1-dep': '100.0.0' } },
    },
    {
      location: 'packages/project-2',
      package: { name: 'project-2', version: '1.0.0', dependencies: { '@pnpm.e2e/pkg-with-1-dep': '100.0.0' } },
    },
  ])
  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['packages/*'],
    sharedWorkspaceLockfile: false,
  })
  await execPnpm(['install'])

  const { stdout } = execPnpmSync(['why', '@pnpm.e2e/dep-of-pkg-with-1-dep'], { expectSuccess: true })
  const output = stdout.toString()
  expect(output).toContain('@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0')
  expect(output).toContain('project-1@1.0.0')
  expect(output).toContain('project-2@1.0.0')
  expect(output).toContain('Found 1 version of @pnpm.e2e/dep-of-pkg-with-1-dep')
})

test('list formats aliased file and scoped link dependencies consistently', async () => {
  preparePackages([
    {
      location: 'local-pkg',
      package: { name: 'my-local-pkg', version: '1.0.0' },
    },
    {
      location: '@scoped-dir/local-scoped',
      package: { name: '@example/local-scoped', version: '1.0.0' },
    },
    {
      location: 'packages/@example/lib',
      package: { name: '@example/lib', version: '1.0.0' },
    },
    {
      location: 'packages/@example/other',
      package: { name: '@example/other', version: '1.0.0' },
    },
    {
      location: 'packages/unscoped-lib',
      package: { name: 'unscoped-lib', version: '1.0.0' },
    },
    {
      location: 'packages/app',
      package: {
        name: 'my-app',
        version: '1.0.0',
        dependencies: {
          '@example/lib': 'workspace:*',
          'unscoped-lib': 'workspace:*',
          'ws-alias': 'workspace:@example/other@*',
          'my-alias': 'file:../../local-pkg',
          'scoped-alias': 'file:../../@scoped-dir/local-scoped',
        },
      },
    },
  ])
  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['packages/*', 'packages/@example/*'],
  })
  await execPnpm(['install'])

  const treeOut = execPnpmSync(['--dir', 'packages/app', 'list'], { expectSuccess: true }).stdout.toString()
  expect(treeOut).toContain('@example/lib@link:../@example/lib')
  expect(treeOut).toContain('scoped-alias@@example/local-scoped@file:@scoped-dir/local-scoped')
  expect(treeOut).toContain('my-alias@my-local-pkg@file:local-pkg')
  expect(treeOut).not.toContain('npm:my-local-pkg')
  expect(treeOut).toContain('unscoped-lib@link:../unscoped-lib')
  expect(treeOut).toContain('ws-alias@link:../@example/other')

  const parseableLongOut = execPnpmSync(['--dir', 'packages/app', 'list', '--parseable', '--long'], { expectSuccess: true }).stdout.toString()
  const lines = parseableLongOut.trim().split(/\r?\n/)
  const appPath = fs.realpathSync('packages/app')
  const exampleLibPath = fs.realpathSync('packages/@example/lib')
  const exampleOtherPath = fs.realpathSync('packages/@example/other')
  const unscopedLibPath = fs.realpathSync('packages/unscoped-lib')
  const myAliasPath = fs.realpathSync('packages/app/node_modules/my-alias')
  const scopedAliasPath = fs.realpathSync('packages/app/node_modules/scoped-alias')
  expect(lines).toEqual([
    `${appPath}:my-app@1.0.0`,
    `${exampleLibPath}:@example/lib@link:../@example/lib`,
    `${scopedAliasPath}:scoped-alias @example/local-scoped@file:@scoped-dir/local-scoped`,
    `${myAliasPath}:my-alias my-local-pkg@file:local-pkg`,
    `${unscopedLibPath}:unscoped-lib@link:../unscoped-lib`,
    `${exampleOtherPath}:ws-alias@link:../@example/other`,
  ])
})

test('why with dedicated lockfiles keeps distinct local file directories separate', async () => {
  preparePackages([
    {
      location: 'vendor/foo-v1',
      package: { name: 'foo', version: '1.0.0', description: 'first local copy' },
    },
    {
      location: 'vendor/foo-v2',
      package: { name: 'foo', version: '2.0.0', description: 'second local copy' },
    },
    {
      location: 'packages/project-1',
      package: {
        name: 'project-1',
        version: '1.0.0',
        dependencies: { foo: 'file:../../vendor/foo-v1' },
      },
    },
    {
      location: 'packages/project-2',
      package: {
        name: 'project-2',
        version: '1.0.0',
        dependencies: { foo: 'file:../../vendor/foo-v2' },
      },
    },
    {
      location: 'packages/project-3',
      package: {
        name: 'project-3',
        version: '1.0.0',
        dependencies: { foo: 'file:../../vendor/foo-v1' },
      },
    },
  ])
  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['packages/*'],
    sharedWorkspaceLockfile: false,
  })
  await execPnpm(['install'])

  const whyJsonOut = execPnpmSync(['why', '-r', 'foo', '--json', '--long'], { expectSuccess: true }).stdout.toString()
  const trees = JSON.parse(whyJsonOut)
  expect(trees).toHaveLength(2)
  expect(trees[0]).toMatchObject({
    name: 'foo',
    description: 'first local copy',
    dependents: [{ name: 'project-1' }, { name: 'project-3' }],
  })
  expect(trees[1]).toMatchObject({
    name: 'foo',
    description: 'second local copy',
    dependents: [{ name: 'project-2' }],
  })

  const whyLongOut = execPnpmSync(['why', '-r', 'foo', '--long'], { expectSuccess: true }).stdout.toString()
  expect(whyLongOut).toContain('first local copy')
  expect(whyLongOut).toContain('second local copy')
})


test('lockfile-only finder reads cached manifests without node_modules', async () => {
  prepare({ dependencies: { '@pnpm.e2e/abc': '1.0.0' } })
  fs.writeFileSync('.pnpmfile.cjs', `
module.exports = { finders: {
  cached: ctx => ctx.readManifest()?.peerDependencies?.['@pnpm.e2e/peer-a'] === '^1.0.0'
} }
`)
  await execPnpm(['install', '--lockfile-only'])
  await execPnpm(['fetch'])
  fs.rmSync('node_modules', { recursive: true, force: true })
  const { stdout } = execPnpmSync(['list', '--lockfile-only', '--find-by=cached', '--json'], { expectSuccess: true })
  expect(JSON.parse(stdout.toString())[0].dependencies['@pnpm.e2e/abc'].version).toBe('1.0.0')
  expect(fs.existsSync('node_modules')).toBe(false)
})


test.each([true, false])('child project finder uses the workspace relative store (shared lockfile: %s)', async (sharedWorkspaceLockfile) => {
  preparePackages([{ location: 'packages/example', package: {
    name: 'example', version: '1.0.0', dependencies: { '@pnpm.e2e/abc': '1.0.0' },
  } }])
  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['packages/*'], storeDir: '.cache-store', sharedWorkspaceLockfile,
  })
  fs.writeFileSync('.pnpmfile.cjs', `module.exports = { finders: {
    cached: ctx => ctx.readManifest()?.peerDependencies?.['@pnpm.e2e/peer-a'] === '^1.0.0'
  } }`)
  fs.copyFileSync('.pnpmfile.cjs', 'packages/example/.pnpmfile.cjs')
  await execPnpm(['install'])
  fs.rmSync('node_modules', { recursive: true, force: true })
  fs.rmSync('packages/example/node_modules', { recursive: true, force: true })
  const { stdout } = execPnpmSync(['--dir=packages/example', 'list', '--lockfile-only', '--find-by=cached', '--json'], { expectSuccess: true })
  expect(JSON.parse(stdout.toString())[0].dependencies['@pnpm.e2e/abc'].version).toBe('1.0.0')
})
