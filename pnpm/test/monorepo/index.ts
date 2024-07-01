// cspell:ignore buildscript
import fs from 'fs'
import path from 'path'
import { LOCKFILE_VERSION, WANTED_LOCKFILE } from '@pnpm/constants'
import { findWorkspacePackages } from '@pnpm/workspace.find-packages'
import { type LockfileFileV9 as LockfileFile } from '@pnpm/lockfile-types'
import { readModulesManifest } from '@pnpm/modules-yaml'
import {
  prepare,
  prepareEmpty,
  preparePackages,
  tempDir as makeTempDir,
} from '@pnpm/prepare'
import { readPackageJsonFromDir } from '@pnpm/read-package-json'
import { sync as readYamlFile } from 'read-yaml-file'
import execa from 'execa'
import { sync as rimraf } from '@zkochan/rimraf'
import tempy from 'tempy'
import symlink from 'symlink-dir'
import { sync as writeYamlFile } from 'write-yaml-file'
import { execPnpm, execPnpmSync } from '../utils'
import { addDistTag } from '@pnpm/registry-mock'
import { createTestIpcServer } from '@pnpm/test-ipc-server'
import { type ProjectManifest } from '@pnpm/types'

test('no projects matched the filters', async () => {
  preparePackages([
    {
      name: 'project',
      version: '1.0.0',
    },
  ])

  writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  {
    const { stdout } = execPnpmSync(['list', '--filter=not-exists'])
    expect(stdout.toString()).toMatch(/^No projects matched the filters in/)
  }
  {
    const { stdout, status } = execPnpmSync(['list', '--filter=not-exists', '--fail-if-no-match'])
    expect(stdout.toString()).toMatch(/^No projects matched the filters in/)
    expect(status).toBe(1)
  }
  {
    const { stdout } = execPnpmSync(['list', '--filter=not-exists', '--parseable'])
    expect(stdout.toString()).toBe('') // don't print anything if --parseable is used
  }
})

test('no projects found', async () => {
  prepareEmpty()

  {
    const { stdout } = execPnpmSync(['list', '-r'])
    expect(stdout.toString()).toMatch(/^No projects found in/)
  }
  {
    const { stdout } = execPnpmSync(['list', '-r', '--parseable'])
    expect(stdout.toString()).toBe('') // don't print anything if --parseable is used
  }
})

test('incorrect workspace manifest', async () => {
  preparePackages([
    {
      name: 'project',
      version: '1.0.0',
    },
  ])

  writeYamlFile('pnpm-workspace.yml', { packages: ['**', '!store/**'] })

  const { status, stdout } = execPnpmSync(['install'])
  expect(stdout.toString()).toMatch(/The workspace manifest file should be named "pnpm-workspace.yaml"/)
  expect(status).toBe(1)
})

test('linking a package inside a monorepo', async () => {
  const projects = preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',
    },
    {
      name: 'project-2',
      version: '2.0.0',
    },
    {
      name: 'project-3',
      version: '3.0.0',
    },
    {
      name: 'project-4',
      version: '4.0.0',
    },
  ])

  writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  process.chdir('project-1')

  await execPnpm(['link', 'project-2'])

  await execPnpm(['link', 'project-3', '--save-dev'])

  await execPnpm(['link', 'project-4', '--save-optional'])

  const { default: pkg } = await import(path.resolve('package.json'))

  expect(pkg?.dependencies).toStrictEqual({ 'project-2': '^2.0.0' }) // spec of linked package added to dependencies
  expect(pkg?.devDependencies).toStrictEqual({ 'project-3': '^3.0.0' }) // spec of linked package added to devDependencies
  expect(pkg?.optionalDependencies).toStrictEqual({ 'project-4': '^4.0.0' }) // spec of linked package added to optionalDependencies

  projects['project-1'].has('project-2')
  projects['project-1'].has('project-3')
  projects['project-1'].has('project-4')
})

test('linking a package inside a monorepo with --link-workspace-packages when installing new dependencies', async () => {
  const projects = preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',
    },
    {
      name: 'project-2',
      version: '2.0.0',
    },
    {
      name: 'project-3',
      version: '3.0.0',
    },
    {
      name: 'project-4',
      version: '4.0.0',
    },
  ])

  fs.writeFileSync('.npmrc', 'link-workspace-packages = true', 'utf8')
  writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  process.chdir('project-1')

  await execPnpm(['add', 'project-2'])

  await execPnpm(['add', 'project-3', '--save-dev'])

  await execPnpm(['add', 'project-4', '--save-optional', '--no-save-workspace-protocol'])

  const { default: pkg } = await import(path.resolve('package.json'))

  expect(pkg?.dependencies).toStrictEqual({ 'project-2': 'workspace:^' }) // spec of linked package added to dependencies
  expect(pkg?.devDependencies).toStrictEqual({ 'project-3': 'workspace:^' }) // spec of linked package added to devDependencies
  expect(pkg?.optionalDependencies).toStrictEqual({ 'project-4': '^4.0.0' }) // spec of linked package added to optionalDependencies

  projects['project-1'].has('project-2')
  projects['project-1'].has('project-3')
  projects['project-1'].has('project-4')
})

test('linking a package inside a monorepo with --link-workspace-packages when installing new dependencies and save-workspace-protocol is "rolling"', async () => {
  const projects = preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',
    },
    {
      name: 'project-2',
      version: '2.0.0',
    },
    {
      name: 'project-3',
      version: '3.0.0',
    },
    {
      name: 'project-4',
      version: '4.0.0',
    },
  ])

  fs.writeFileSync(
    '.npmrc',
    [
      'link-workspace-packages = true',
      'save-workspace-protocol = "rolling"',
    ].join('\n'),
    'utf8')
  writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  process.chdir('project-1')

  await execPnpm(['add', 'project-2'])

  await execPnpm(['add', 'project-3', '--save-dev'])

  await execPnpm(['add', 'project-4', '--save-optional', '--no-save-workspace-protocol'])

  const { default: pkg } = await import(path.resolve('package.json'))

  expect(pkg?.dependencies).toStrictEqual({ 'project-2': 'workspace:^' }) // spec of linked package added to dependencies
  expect(pkg?.devDependencies).toStrictEqual({ 'project-3': 'workspace:^' }) // spec of linked package added to devDependencies
  expect(pkg?.optionalDependencies).toStrictEqual({ 'project-4': '^4.0.0' }) // spec of linked package added to optionalDependencies

  projects['project-1'].has('project-2')
  projects['project-1'].has('project-3')
  projects['project-1'].has('project-4')
})

test('linking a package inside a monorepo with --link-workspace-packages', async () => {
  await using server = await createTestIpcServer()

  const projects = preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'project-2': '2.0.0',
      },
      devDependencies: {
        'is-negative': '100.0.0',
      },
      optionalDependencies: {
        'is-positive': '1.0.0',
      },
      scripts: {
        install: server.sendLineScript('project-1'),
      },
    },
    {
      name: 'project-2',
      version: '2.0.0',

      scripts: {
        install: server.sendLineScript('project-2'),
      },
    },
    {
      name: 'is-negative',
      version: '100.0.0',
    },
    {
      name: 'is-positive',
      version: '1.0.0',
    },
  ])

  fs.writeFileSync('.npmrc', `
link-workspace-packages = true
shared-workspace-lockfile=false
save-workspace-protocol=false
`, 'utf8')
  writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  process.chdir('project-1')

  await execPnpm(['install'])

  expect(server.getLines()).toStrictEqual(['project-2', 'project-1'])

  projects['project-1'].has('project-2')
  projects['project-1'].has('is-negative')
  projects['project-1'].has('is-positive')

  {
    const lockfile = projects['project-1'].readLockfile()
    expect(lockfile.importers['.'].dependencies?.['project-2'].version).toBe('link:../project-2')
    expect(lockfile.importers['.'].devDependencies?.['is-negative'].version).toBe('link:../is-negative')
    expect(lockfile.importers['.'].optionalDependencies?.['is-positive'].version).toBe('link:../is-positive')
  }

  projects['is-positive'].writePackageJson({
    name: 'is-positive',
    version: '2.0.0',
  })

  await execPnpm(['install'])

  {
    const lockfile = projects['project-1'].readLockfile()
    expect(lockfile.importers['.'].optionalDependencies?.['is-positive'].version).toBe('1.0.0') // is-positive is unlinked and installed from registry
  }

  await execPnpm(['update', 'is-negative@2.0.0'])

  {
    const lockfile = projects['project-1'].readLockfile()
    expect(lockfile.importers['.'].devDependencies?.['is-negative'].version).toBe('2.0.0')
  }
})

test('topological order of packages with self-dependencies in monorepo is correct', async () => {
  await using server1 = await createTestIpcServer()
  await using server2 = await createTestIpcServer()

  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: { 'project-2': '1.0.0', 'project-3': '1.0.0' },
      scripts: {
        install: server1.sendLineScript('project-1'),
        test: server2.sendLineScript('project-1'),
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: { 'project-2': '1.0.0' },
      scripts: {
        install: server1.sendLineScript('project-2'),
        test: server2.sendLineScript('project-2'),
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',

      dependencies: { 'project-2': '1.0.0', 'project-3': '1.0.0' },
      scripts: {
        install: server1.sendLineScript('project-3'),
        test: server2.sendLineScript('project-3'),
      },
    },
  ])
  fs.writeFileSync('.npmrc', 'link-workspace-packages = true', 'utf8')
  writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  process.chdir('project-1')

  await execPnpm(['install'])

  expect(server1.getLines()).toStrictEqual(['project-2', 'project-3', 'project-1'])

  await execPnpm(['recursive', 'test'])

  expect(server2.getLines()).toStrictEqual(['project-2', 'project-3', 'project-1'])
})

test('test-pattern is respected by the test script', async () => {
  await using server = await createTestIpcServer()

  const remote = tempy.directory()

  const projects: Array<ProjectManifest & { name: string }> = [
    {
      name: 'project-1',
      version: '1.0.0',
      dependencies: { 'project-2': 'workspace:*', 'project-3': 'workspace:*' },
      scripts: {
        test: server.sendLineScript('project-1'),
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',
      dependencies: {},
      scripts: {
        test: server.sendLineScript('project-2'),
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',
      dependencies: { 'project-2': 'workspace:*' },
      scripts: {
        test: server.sendLineScript('project-3'),
      },
    },
    {
      name: 'project-4',
      version: '1.0.0',
      dependencies: {},
      scripts: {
        test: server.sendLineScript('project-4'),
      },
    },
  ]
  preparePackages(projects)

  await execa('git', ['init', '--initial-branch=main'])
  await execa('git', ['config', 'user.email', 'x@y.z'])
  await execa('git', ['config', 'user.name', 'xyz'])
  await execa('git', ['init', '--bare'], { cwd: remote })
  await execa('git', ['add', '*'])
  await execa('git', ['commit', '-m', 'init', '--no-gpg-sign'])
  await execa('git', ['remote', 'add', 'origin', remote])
  await execa('git', ['push', '-u', 'origin', 'main'])

  fs.writeFileSync('project-2/file.js', '')
  fs.writeFileSync('project-4/different-pattern.js', '')
  fs.writeFileSync('.npmrc', 'test-pattern[]=*/file.js', 'utf8')
  writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  await execa('git', ['add', '.'])
  await execa('git', ['commit', '--allow-empty-message', '-m', '', '--no-gpg-sign'])

  await execPnpm(['install'])

  await execPnpm(['recursive', 'test', '--filter', '...[origin/main]'])

  // Expecting only project-2 and project-4 to run since they were changed above.
  expect(server.getLines().sort()).toEqual(['project-2', 'project-4'])
})

test('changed-files-ignore-pattern is respected', async () => {
  const remote = tempy.directory()

  preparePackages([
    {
      name: 'project-1-no-changes',
      version: '1.0.0',
    },
    {
      name: 'project-2-change-is-never-ignored',
      version: '1.0.0',
    },
    {
      name: 'project-3-ignored-by-pattern',
      version: '1.0.0',
    },
    {
      name: 'project-4-ignored-by-pattern',
      version: '1.0.0',
    },
    {
      name: 'project-5-ignored-by-pattern',
      version: '1.0.0',
    },
  ])

  await execa('git', ['init', '--initial-branch=main'])
  await execa('git', ['config', 'user.email', 'x@y.z'])
  await execa('git', ['config', 'user.name', 'xyz'])
  await execa('git', ['init', '--bare'], { cwd: remote })
  await execa('git', ['add', '*'])
  await execa('git', ['commit', '-m', 'init', '--no-gpg-sign'])
  await execa('git', ['remote', 'add', 'origin', remote])
  await execa('git', ['push', '-u', 'origin', 'main'])

  const npmrcLines = []
  fs.writeFileSync('project-2-change-is-never-ignored/index.js', '')

  npmrcLines.push('changed-files-ignore-pattern[]=**/{*.spec.js,*.md}')
  fs.writeFileSync('project-3-ignored-by-pattern/index.spec.js', '')
  fs.writeFileSync('project-3-ignored-by-pattern/README.md', '')

  npmrcLines.push('changed-files-ignore-pattern[]=**/buildscript.js')
  fs.mkdirSync('project-4-ignored-by-pattern/a/b/c', {
    recursive: true,
  })
  fs.writeFileSync('project-4-ignored-by-pattern/a/b/c/buildscript.js', '')

  npmrcLines.push('changed-files-ignore-pattern[]=**/cache/**')
  fs.mkdirSync('project-5-ignored-by-pattern/cache/a/b', {
    recursive: true,
  })
  fs.writeFileSync('project-5-ignored-by-pattern/cache/a/b/index.js', '')

  writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  await execa('git', ['add', '.'])
  await execa('git', [
    'commit',
    '--allow-empty-message',
    '-m',
    '',
    '--no-gpg-sign',
  ])

  fs.writeFileSync('.npmrc', npmrcLines.join('\n'), 'utf8')
  await execPnpm(['install'])

  const getChangedProjects = async (opts?: {
    overrideChangedFilesIgnorePatternWithNoPattern: boolean
  }) => {
    const result = execPnpmSync(
      [
        '--filter',
        '[origin/main]',
        opts?.overrideChangedFilesIgnorePatternWithNoPattern
          ? '--changed-files-ignore-pattern='
          : '',
        'ls',
        '--depth',
        '-1',
        '--json',
      ].filter(Boolean)
    )
    return JSON.parse(result.stdout.toString())
      .map((p: { name: string }) => p.name)
      .sort()
  }

  expect(await getChangedProjects()).toStrictEqual([
    'project-2-change-is-never-ignored',
  ])

  expect(
    await getChangedProjects({
      overrideChangedFilesIgnorePatternWithNoPattern: true,
    })
  ).toStrictEqual([
    'project-2-change-is-never-ignored',
    'project-3-ignored-by-pattern',
    'project-4-ignored-by-pattern',
    'project-5-ignored-by-pattern',
  ])
})

test('do not get confused by filtered dependencies when searching for dependents in monorepo', async () => {
  /*
   In this test case, we are filtering for 'project-2' and its dependents with
   two projects in the dependency hierarchy, that can be ignored for this query,
   as they do not depend on 'project-2'.
  */
  preparePackages([
    {
      name: 'unused-project-1',
      version: '1.0.0',
    },
    {
      name: 'unused-project-2',
      version: '1.0.0',
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: { 'unused-project-1': '1.0.0', 'unused-project-2': '1.0.0' },
      scripts: {
        test: 'node -e "process.stdout.write(\'printed\' + \' by project-2\')"',
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',

      dependencies: { 'project-2': '1.0.0' },
      scripts: {
        test: 'node -e "process.stdout.write(\'printed\' + \' by project-3\')"',
      },
    },
    {
      name: 'project-4',
      version: '1.0.0',

      dependencies: { 'project-2': '1.0.0', 'unused-project-1': '1.0.0', 'unused-project-2': '1.0.0' },
      scripts: {
        test: 'node -e "process.stdout.write(\'printed\' + \' by project-4\')"',
      },
    },
  ])
  fs.writeFileSync('.npmrc', 'link-workspace-packages = true', 'utf8')
  writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  process.chdir('project-2')

  const { stdout } = execPnpmSync(['--filter=...project-2', 'run', 'test'])

  const output = stdout.toString()
  const project2Output = output.indexOf('printed by project-2')
  const project3Output = output.indexOf('printed by project-3')
  const project4Output = output.indexOf('printed by project-4')
  expect(project2Output < project3Output).toBeTruthy()
  expect(project2Output < project4Output).toBeTruthy()
})

test('installation with --link-workspace-packages links packages even if they were previously installed from registry', async () => {
  const projects = preparePackages([
    {
      name: 'project',
      version: '1.0.0',

      dependencies: {
        'is-positive': '2.0.0',
        negative: 'npm:is-negative@1.0.0',
      },
    },
    {
      name: 'is-positive',
      version: '2.0.0',
    },
    {
      name: 'is-negative',
      version: '1.0.0',
    },
  ])

  await execPnpm(['recursive', 'install', '--no-link-workspace-packages'])

  {
    const lockfile = projects.project.readLockfile()
    expect(lockfile.importers['.'].dependencies?.['is-positive'].version).toBe('2.0.0')
    expect(lockfile.importers['.'].dependencies?.negative.version).toBe('is-negative@1.0.0')
  }

  await execPnpm(['recursive', 'install', '--link-workspace-packages'])

  {
    const lockfile = projects.project.readLockfile()
    expect(lockfile.importers['.'].dependencies?.['is-positive'].version).toBe('link:../is-positive')
    expect(lockfile.importers['.'].dependencies?.negative.version).toBe('link:../is-negative')
  }
})

test('shared-workspace-lockfile: installation with --link-workspace-packages links packages even if they were previously installed from registry', async () => {
  const projects = preparePackages([
    {
      name: 'project',
      version: '1.0.0',

      dependencies: {
        'is-positive': '2.0.0',
        negative: 'npm:is-negative@1.0.0',
      },
    },
    {
      name: 'is-positive',
      version: '3.0.0',
    },
    {
      name: 'is-negative',
      version: '3.0.0',
    },
  ])

  writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })
  fs.writeFileSync('.npmrc', 'shared-workspace-lockfile = true\nlink-workspace-packages = true', 'utf8')

  await execPnpm(['recursive', 'install'])

  {
    const lockfile = readYamlFile<LockfileFile>(WANTED_LOCKFILE)
    expect(lockfile.importers!.project!.dependencies!['is-positive'].version).toBe('2.0.0')
    expect(lockfile.importers!.project!.dependencies!.negative.version).toBe('is-negative@1.0.0')
  }

  projects['is-positive'].writePackageJson({
    name: 'is-positive',
    version: '2.0.0',
  })

  projects['is-negative'].writePackageJson({
    name: 'is-negative',
    version: '1.0.0',
  })

  await execPnpm(['recursive', 'install'])

  {
    const lockfile = readYamlFile<LockfileFile>(WANTED_LOCKFILE)
    expect(lockfile.importers!.project!.dependencies!['is-positive'].version).toBe('link:../is-positive')
    expect(lockfile.importers!.project!.dependencies!.negative.version).toBe('link:../is-negative')
  }
})

test('recursive install with link-workspace-packages and shared-workspace-lockfile', async () => {
  await using server = await createTestIpcServer()
  await addDistTag({ package: '@pnpm.e2e/pkg-with-1-dep', version: '100.0.0', distTag: 'latest' })
  const projects = preparePackages([
    {
      name: 'is-positive',
      version: '1.0.0',

      dependencies: {
        'is-negative': '1.0.0',
      },
      scripts: {
        install: server.sendLineScript('is-positive'),
      },
    },
    // This empty package is added to the workspace only to verify
    // that empty package does not remove .pendingBuild from .modules.yaml
    {
      name: 'is-positive2',
      version: '1.0.0',
    },
    {
      name: 'project-1',
      version: '1.0.0',

      devDependencies: {
        'is-positive': '1.0.0',
      },
      scripts: {
        install: server.sendLineScript('project-1'),
      },
    },
  ])

  writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })
  fs.writeFileSync(
    'is-positive/.npmrc',
    'save-exact = true',
    'utf8'
  )
  fs.writeFileSync(
    'project-1/.npmrc',
    'save-prefix = ~',
    'utf8'
  )

  await execPnpm(['recursive', 'install', '--link-workspace-packages', '--shared-workspace-lockfile=true', '--store-dir', 'store'])

  expect(projects['is-positive'].requireModule('is-negative')).toBeTruthy()
  expect(projects['project-1'].requireModule('is-positive/package.json').author).toBeFalsy()

  const sharedLockfile = readYamlFile<LockfileFile>(WANTED_LOCKFILE)
  expect(sharedLockfile.importers!['project-1']!.devDependencies!['is-positive'].version).toBe('link:../is-positive')

  expect(server.getLines()).toStrictEqual(['is-positive', 'project-1'])

  await execPnpm(['recursive', 'install', '@pnpm.e2e/pkg-with-1-dep', '--link-workspace-packages', '--shared-workspace-lockfile=true', '--store-dir', 'store'])

  {
    const pkg = await readPackageJsonFromDir(path.resolve('is-positive'))
    expect(pkg.dependencies!['@pnpm.e2e/pkg-with-1-dep']).toBe('100.0.0')
  }

  {
    const pkg = await readPackageJsonFromDir(path.resolve('project-1'))
    expect(pkg.dependencies!['@pnpm.e2e/pkg-with-1-dep']).toBe('~100.0.0')
  }

  {
    const pkg = await readPackageJsonFromDir(path.resolve('is-positive2'))
    expect(pkg.dependencies!['@pnpm.e2e/pkg-with-1-dep']).toBe('^100.0.0')
  }
})

test('recursive install with shared-workspace-lockfile builds workspace projects in correct order', async () => {
  await using server1 = await createTestIpcServer()
  await using server2 = await createTestIpcServer()

  preparePackages([
    {
      name: 'project-999',
      version: '1.0.0',

      scripts: {
        install: `${server1.sendLineScript('project-999-install')} && ${server2.sendLineScript('project-999-install')}`,
        postinstall: `${server1.sendLineScript('project-999-postinstall')} && ${server2.sendLineScript('project-999-postinstall')}`,
        prepare: `${server1.sendLineScript('project-999-prepare')} && ${server2.sendLineScript('project-999-prepare')}`,
        prepublish: `${server1.sendLineScript('project-999-prepublish')} && ${server2.sendLineScript('project-999-prepublish')}`,
      },
    },
    {
      name: 'project-1',
      version: '1.0.0',

      devDependencies: {
        'project-999': '1.0.0',
      },
      scripts: {
        install: server1.sendLineScript('project-1-install'),
        postinstall: server1.sendLineScript('project-1-postinstall'),
        prepare: server1.sendLineScript('project-1-prepare'),
        prepublish: server1.sendLineScript('project-1-prepublish'),
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      devDependencies: {
        'project-999': '1.0.0',
      },
      scripts: {
        install: server2.sendLineScript('project-2-install'),
        postinstall: server2.sendLineScript('project-2-postinstall'),
        prepare: server2.sendLineScript('project-2-prepare'),
        prepublish: server2.sendLineScript('project-2-prepublish'),
      },
    },
  ], { manifestFormat: 'YAML' })

  writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  await execPnpm(['recursive', 'install', '--link-workspace-packages', '--shared-workspace-lockfile=true', '--store-dir', 'store'])

  expect(server1.getLines()).toStrictEqual([
    'project-999-install',
    'project-999-postinstall',
    'project-999-prepare',
    'project-1-install',
    'project-1-postinstall',
    'project-1-prepare',
  ])

  expect(server2.getLines()).toStrictEqual([
    'project-999-install',
    'project-999-postinstall',
    'project-999-prepare',
    'project-2-install',
    'project-2-postinstall',
    'project-2-prepare',
  ])

  rimraf('node_modules')
  server1.clear()
  server2.clear()

  // TODO: duplicate this test in @pnpm/headless
  await execPnpm(['recursive', 'install', '--frozen-lockfile', '--link-workspace-packages', '--shared-workspace-lockfile=true'])

  expect(server1.getLines()).toStrictEqual([
    'project-999-install',
    'project-999-postinstall',
    'project-999-prepare',
    'project-1-install',
    'project-1-postinstall',
    'project-1-prepare',
  ])

  expect(server2.getLines()).toStrictEqual([
    'project-999-install',
    'project-999-postinstall',
    'project-999-prepare',
    'project-2-install',
    'project-2-postinstall',
    'project-2-prepare',
  ])
})

test('recursive installation with shared-workspace-lockfile and a readPackage hook', async () => {
  const projects = preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'is-positive': '1.0.0',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'is-negative': '1.0.0',
      },
    },
  ])

  const pnpmfile = `
    module.exports = { hooks: { readPackage } }
    function readPackage (pkg) {
      pkg.dependencies = pkg.dependencies || {}
      pkg.dependencies['@pnpm.e2e/dep-of-pkg-with-1-dep'] = '100.1.0'
      return pkg
    }
  `
  fs.writeFileSync('.pnpmfile.cjs', pnpmfile, 'utf8')
  writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  await execPnpm(['recursive', 'install', '--shared-workspace-lockfile', '--store-dir', 'store'])

  const lockfile = readYamlFile<LockfileFile>(`./${WANTED_LOCKFILE}`)
  expect(lockfile.packages).toHaveProperty(['@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0'])

  await execPnpm(['recursive', 'install', '--shared-workspace-lockfile', '--store-dir', 'store', '--filter', 'project-1'])

  projects['project-1'].hasNot('project-1')
})

test('local packages should be preferred when running "pnpm install" inside a workspace', async () => {
  const projects = preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'is-positive': '1.0.0',
      },
    },
    {
      name: 'is-positive',
      version: '1.0.0',
    },
  ])

  writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })
  fs.writeFileSync('.npmrc', 'link-workspace-packages = true\nshared-workspace-lockfile=false', 'utf8')

  process.chdir('project-1')

  await execPnpm(['link', '.'])

  const lockfile = projects['project-1'].readLockfile()

  expect(lockfile?.importers['.'].dependencies?.['is-positive'].version).toBe('link:../is-positive')
})

// covers https://github.com/pnpm/pnpm/issues/1437
test('shared-workspace-lockfile: create shared lockfile format when installation is inside workspace', async () => {
  prepare({
    dependencies: {
      'is-positive': '1.0.0',
    },
  })

  writeYamlFile('pnpm-workspace.yaml', { packages: ['**', 'project', '!store/**'] })
  fs.writeFileSync('.npmrc', 'shared-workspace-lockfile = true', 'utf8')

  await execPnpm(['install', '--store-dir', 'store'])

  const lockfile = readYamlFile<LockfileFile>(WANTED_LOCKFILE)

  expect(lockfile.importers).toHaveProperty(['.'])
  expect(lockfile.lockfileVersion).toBe(LOCKFILE_VERSION)
})

// covers https://github.com/pnpm/pnpm/issues/1451
test("shared-workspace-lockfile: don't install dependencies in projects that are outside of the current workspace", async () => {
  preparePackages([
    {
      location: 'workspace-1/package-1',
      package: {
        name: 'package-1',
        version: '1.0.0',

        dependencies: {
          'is-positive': '1.0.0',
          'package-2': 'workspace:*',
        },
      },
    },
    {
      location: 'workspace-2/package-2',
      package: {
        name: 'package-2',
        version: '1.0.0',

        dependencies: {
          'is-negative': '1.0.0',
        },
      },
    },
  ])

  await symlink('workspace-2/package-2', 'workspace-1/package-2')

  writeYamlFile('workspace-1/pnpm-workspace.yaml', { packages: ['**', '!store/**'] })
  writeYamlFile('workspace-2/pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  process.chdir('workspace-1')

  await execPnpm(['recursive', 'install', '--store-dir', 'store', '--shared-workspace-lockfile', '--link-workspace-packages'])

  const lockfile = readYamlFile<LockfileFile>(WANTED_LOCKFILE)

  expect(lockfile).toStrictEqual({
    settings: {
      autoInstallPeers: true,
      excludeLinksFromLockfile: false,
    },
    importers: {
      'package-1': {
        dependencies: {
          'is-positive': {
            specifier: '1.0.0',
            version: '1.0.0',
          },
          'package-2': {
            specifier: 'workspace:*',
            version: 'link:../package-2',
          },
        },
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      'is-positive@1.0.0': {
        engines: {
          node: '>=0.10.0',
        },
        resolution: {
          integrity: 'sha512-xxzPGZ4P2uN6rROUa5N9Z7zTX6ERuE0hs6GUOc/cKBLF2NqKc16UwqHMt3tFg4CO6EBTE5UecUasg+3jZx3Ckg==',
        },
      },
    },
    snapshots: {
      'is-positive@1.0.0': {},
    },
  })
})

test('shared-workspace-lockfile: install dependencies in projects that are relative to the workspace directory', async () => {
  preparePackages([
    {
      location: 'monorepo/workspace',
      package: {
        name: 'root-package',
        version: '1.0.0',

        dependencies: {
          'package-1': '1.0.0',
          'package-2': '1.0.0',
        },
      },
    },
    {
      location: 'monorepo/package-1',
      package: {
        name: 'package-1',
        version: '1.0.0',

        dependencies: {
          'is-positive': '1.0.0',
          'package-2': '1.0.0',
        },
      },
    },
    {
      location: 'monorepo/package-2',
      package: {
        name: 'package-2',
        version: '1.0.0',

        dependencies: {
          'is-negative': '1.0.0',
        },
      },
    },
  ])

  writeYamlFile('monorepo/workspace/pnpm-workspace.yaml', { packages: ['../**', '!store/**'] })

  process.chdir('monorepo/workspace')

  await execPnpm(['-r', 'install', '--store-dir', 'store', '--shared-workspace-lockfile', '--link-workspace-packages', '--no-save-workspace-protocol'])

  const lockfile = readYamlFile<LockfileFile>(WANTED_LOCKFILE)

  expect(lockfile).toStrictEqual({
    settings: {
      autoInstallPeers: true,
      excludeLinksFromLockfile: false,
    },
    importers: {
      '.': {
        dependencies: {
          'package-1': {
            specifier: '1.0.0',
            version: 'link:../package-1',
          },
          'package-2': {
            specifier: '1.0.0',
            version: 'link:../package-2',
          },
        },
      },
      '../package-1': {
        dependencies: {
          'is-positive': {
            specifier: '1.0.0',
            version: '1.0.0',
          },
          'package-2': {
            specifier: '1.0.0',
            version: 'link:../package-2',
          },
        },
      },
      '../package-2': {
        dependencies: {
          'is-negative': {
            specifier: '1.0.0',
            version: '1.0.0',
          },
        },
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      'is-negative@1.0.0': {
        engines: {
          node: '>=0.10.0',
        },
        resolution: {
          integrity: 'sha512-1aKMsFUc7vYQGzt//8zhkjRWPoYkajY/I5MJEvrc0pDoHXrW7n5ri8DYxhy3rR+Dk0QFl7GjHHsZU1sppQrWtw==',
        },
      },
      'is-positive@1.0.0': {
        engines: {
          node: '>=0.10.0',
        },
        resolution: {
          integrity: 'sha512-xxzPGZ4P2uN6rROUa5N9Z7zTX6ERuE0hs6GUOc/cKBLF2NqKc16UwqHMt3tFg4CO6EBTE5UecUasg+3jZx3Ckg==',
        },
      },
    },
    snapshots: {
      'is-negative@1.0.0': {},
      'is-positive@1.0.0': {},
    },
  })
})

test('shared-workspace-lockfile: entries of removed projects should be removed from shared lockfile', async () => {
  preparePackages([
    {
      name: 'package-1',
      version: '1.0.0',

      dependencies: {
        'is-positive': '1.0.0',
      },
    },
    {
      name: 'package-2',
      version: '1.0.0',

      dependencies: {
        'is-negative': '1.0.0',
      },
    },
  ])

  writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  await execPnpm(['recursive', 'install', '--store-dir', 'store', '--shared-workspace-lockfile', '--link-workspace-packages'])

  {
    const lockfile = readYamlFile<LockfileFile>(WANTED_LOCKFILE)
    expect(Object.keys(lockfile.importers!)).toStrictEqual(['package-1', 'package-2'])
  }

  rimraf('package-2')

  await execPnpm(['recursive', 'install', '--store-dir', 'store', '--shared-workspace-lockfile', '--link-workspace-packages'])

  {
    const lockfile = readYamlFile<LockfileFile>(WANTED_LOCKFILE)
    expect(Object.keys(lockfile.importers!)).toStrictEqual(['package-1'])
  }
})

// Covers https://github.com/pnpm/pnpm/issues/1482
test('shared-workspace-lockfile config is ignored if no pnpm-workspace.yaml is found', async () => {
  const project = prepare({
    dependencies: {
      'is-positive': '1.0.0',
    },
  })

  fs.writeFileSync('.npmrc', 'shared-workspace-lockfile=true', 'utf8')

  await execPnpm(['install'])

  project.has('is-positive')
})

test('shared-workspace-lockfile: removing a package recursively', async () => {
  preparePackages([
    {
      name: 'project1',
      version: '1.0.0',

      dependencies: {
        'is-positive': '2.0.0',
      },
    },
    {
      name: 'project2',
      version: '1.0.0',

      dependencies: {
        'is-negative': '1.0.0',
        'is-positive': '1.0.0',
      },
    },
    {
      name: 'project3',
      version: '1.0.0',
    },
  ])

  writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })
  fs.writeFileSync('.npmrc', 'shared-workspace-lockfile = true\nlink-workspace-packages = true', 'utf8')

  await execPnpm(['recursive', 'install'])

  await execPnpm(['recursive', 'remove', 'is-positive'])

  {
    const pkg = await readPackageJsonFromDir('project1')

    expect(pkg.dependencies).toBeFalsy() // is-positive removed from project1
  }

  {
    const pkg = await readPackageJsonFromDir('project2')

    expect(pkg.dependencies).toStrictEqual({ 'is-negative': '1.0.0' }) // is-positive removed from project2')
  }

  const lockfile = readYamlFile<LockfileFile>(WANTED_LOCKFILE)

  expect(Object.keys(lockfile.packages ?? {})).toStrictEqual(['is-negative@1.0.0']) // is-positive removed from ${WANTED_LOCKFILE}
})

// Covers https://github.com/pnpm/pnpm/issues/1506
test('peer dependency is grouped with dependent when the peer is a top dependency and external node_modules is used', async () => {
  preparePackages([
    {
      name: 'foo',
      version: '1.0.0',

      dependencies: {
        bar: 'workspace:1.0.0',
      },
    },
    {
      name: 'bar',
      version: '1.0.0',
    },
  ])

  writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })
  fs.writeFileSync('.npmrc', `shared-workspace-lockfile = true
link-workspace-packages = true
auto-install-peers=false`, 'utf8')

  process.chdir('foo')

  await execPnpm(['install', 'ajv@4.10.4', 'ajv-keywords@1.5.0'])

  {
    const lockfile = readYamlFile<LockfileFile>(path.resolve('..', WANTED_LOCKFILE))
    expect(lockfile.importers!.foo).toStrictEqual({
      dependencies: {
        ajv: {
          specifier: '4.10.4',
          version: '4.10.4',
        },
        'ajv-keywords': {
          specifier: '1.5.0',
          version: '1.5.0(ajv@4.10.4)',
        },
        bar: {
          specifier: 'workspace:1.0.0',
          version: 'link:../bar',
        },
      },
    })
  }

  await execPnpm(['uninstall', 'ajv', '--no-strict-peer-dependencies'])

  {
    const lockfile = readYamlFile<LockfileFile>(path.resolve('..', WANTED_LOCKFILE))
    expect(lockfile.importers!.foo).toStrictEqual({
      dependencies: {
        'ajv-keywords': {
          specifier: '1.5.0',
          version: '1.5.0',
        },
        bar: {
          specifier: 'workspace:1.0.0',
          version: 'link:../bar',
        },
      },
    })
  }
})

test('dependencies of workspace projects are built during headless installation', async () => {
  const projects = preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        '@pnpm.e2e/pre-and-postinstall-scripts-example': '1.0.0',
      },
    },
  ])

  fs.writeFileSync('.npmrc', 'shared-workspace-lockfile=false', 'utf8')
  writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  await execPnpm(['recursive', 'install', '--lockfile-only'])
  await execPnpm(['recursive', 'install', '--frozen-lockfile'])

  {
    const generatedByPreinstall = projects['project-1'].requireModule('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall')
    expect(typeof generatedByPreinstall).toBe('function')

    const generatedByPostinstall = projects['project-1'].requireModule('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall')
    expect(typeof generatedByPostinstall).toBe('function')
  }
})

test("linking the package's bin to another workspace package in a monorepo", async () => {
  const projects = preparePackages([
    {
      name: 'hello',
      version: '1.0.0',

      bin: 'index.js',
    },
    {
      name: 'main',
      version: '2.0.0',

      dependencies: {
        hello: 'workspace:*',
      },
    },
  ], { manifestFormat: 'YAML' })

  fs.writeFileSync('./hello/index.js', '#!/usr/bin/env node', 'utf8')

  writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  await execPnpm(['recursive', 'install'])

  projects.main.isExecutable('.bin/hello')

  expect(fs.existsSync('main/node_modules')).toBeTruthy()
  rimraf('main/node_modules')

  await execPnpm(['recursive', 'install', '--frozen-lockfile'])

  projects.main.isExecutable('.bin/hello')
})

test('pnpm sees the bins from the root of the workspace', async () => {
  preparePackages([
    {
      location: '.',
      package: {
        dependencies: {
          '@pnpm.e2e/print-version': '2',
        },
      },
    },
    {
      name: 'project-1',
      version: '1.0.0',
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        '@pnpm.e2e/print-version': '1',
      },
    },
  ])

  writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  await execPnpm(['install'])

  process.chdir('project-1')

  const result = execPnpmSync(['print-version'])

  expect(result.stdout.toString()).toContain('2.0.0')

  process.chdir('../project-2')

  expect(execPnpmSync(['print-version']).stdout.toString()).toContain('1.0.0')
})

test('root package is included when not specified', async () => {
  const tempDir = makeTempDir()
  Object.assign(
    {
      '.': prepare(undefined, { tempDir }),
    },
    preparePackages(
      [
        {
          name: 'project-1',
          version: '1.0.0',
        },
        {
          name: 'project-2',
          version: '2.0.0',
        },
        {
          name: 'project-3',
          version: '3.0.0',
        },
        {
          name: 'project-4',
          version: '4.0.0',
        },
      ],
      { tempDir: `${tempDir}/project` }
    )
  )
  const workspacePackagePatterns = ['project-', '!store/**']
  writeYamlFile('pnpm-workspace.yaml', { packages: workspacePackagePatterns })
  const workspacePackages = await findWorkspacePackages(tempDir, { engineStrict: false, patterns: workspacePackagePatterns })

  expect(workspacePackages.some(project => {
    const relativePath = path.join('.', path.relative(tempDir, project.rootDir))
    return relativePath === '.' && project.manifest.name === 'project'
  })).toBeTruthy() // root project is present even if not specified
})

test("root package can't be ignored using '!.' (or any other such glob)", async () => {
  const tempDir = makeTempDir()
  Object.assign(
    {
      '.': prepare(undefined, { tempDir }),
    },
    preparePackages(
      [
        {
          name: 'project-1',
          version: '1.0.0',
        },
        {
          name: 'project-2',
          version: '2.0.0',
        },
        {
          name: 'project-3',
          version: '3.0.0',
        },
        {
          name: 'project-4',
          version: '4.0.0',
        },
      ],
      { tempDir: `${tempDir}/project` }
    )
  )
  const workspacePackagePatterns = ['project-', '!.', '!./', '!store/**']
  writeYamlFile('pnpm-workspace.yaml', { packages: workspacePackagePatterns })
  const workspacePackages = await findWorkspacePackages(tempDir, { engineStrict: false, patterns: workspacePackagePatterns })

  expect(workspacePackages.some(project => {
    const relativePath = path.join('.', path.relative(tempDir, project.rootDir))
    return relativePath === '.' && project.manifest.name === 'project'
  })).toBeTruthy() // root project is present even when explicitly ignored
})

test('custom virtual store directory in a workspace with not shared lockfile', async () => {
  const projects = preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'is-positive': '1.0.0',
      },
    },
  ])

  fs.writeFileSync('.npmrc', 'virtual-store-dir=virtual-store\nshared-workspace-lockfile=false', 'utf8')
  writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  await execPnpm(['install'])

  {
    const modulesManifest = projects['project-1'].readModulesManifest()
    const virtualStoreDir = modulesManifest!.virtualStoreDir
    if (path.isAbsolute(virtualStoreDir)) {
      expect(virtualStoreDir).toBe(path.resolve('project-1/virtual-store'))
    } else {
      expect(virtualStoreDir).toBe('../virtual-store')
    }
  }

  rimraf('project-1/virtual-store')
  rimraf('project-1/node_modules')

  await execPnpm(['install', '--frozen-lockfile'])

  {
    const modulesManifest = projects['project-1'].readModulesManifest()
    const virtualStoreDir = modulesManifest!.virtualStoreDir
    if (path.isAbsolute(virtualStoreDir)) {
      expect(virtualStoreDir).toBe(path.resolve('project-1/virtual-store'))
    } else {
      expect(virtualStoreDir).toBe('../virtual-store')
    }
  }
})

test('custom virtual store directory in a workspace with shared lockfile', async () => {
  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'is-positive': '1.0.0',
      },
    },
  ])

  fs.writeFileSync('.npmrc', 'virtual-store-dir=virtual-store\nshared-workspace-lockfile=true', 'utf8')
  writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  await execPnpm(['install'])

  {
    const modulesManifest = await readModulesManifest(path.resolve('node_modules'))
    expect(modulesManifest?.virtualStoreDir).toBe(path.resolve('virtual-store'))
  }

  rimraf('virtual-store')
  rimraf('node_modules')

  await execPnpm(['install', '--frozen-lockfile'])

  {
    const modulesManifest = await readModulesManifest(path.resolve('node_modules'))
    expect(modulesManifest?.virtualStoreDir).toBe(path.resolve('virtual-store'))
  }
})

test('pnpm run should ignore the root project', async () => {
  preparePackages([
    {
      location: '.',
      package: {
        scripts: {
          test: 'exit 1',
        },
      },
    },
    {
      name: 'project',
      version: '1.0.0',
      scripts: {
        test: "node -e \"require('fs').writeFileSync('test','','utf8')\"",
      },
    },
  ])

  writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  await execPnpm(['-r', '--config.use-beta-cli=true', 'test'])

  expect(fs.existsSync('project/test')).toBeTruthy()
})

test('pnpm run should include the workspace root when --workspace-root option is used', async () => {
  preparePackages([
    {
      location: '.',
      package: {
        scripts: {
          test: "node -e \"require('fs').writeFileSync('test','','utf8')\"",
        },
      },
    },
    {
      name: 'project',
      version: '1.0.0',
      scripts: {
        test: "node -e \"require('fs').writeFileSync('test','','utf8')\"",
      },
    },
  ])

  writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  await execPnpm(['--filter=project', '--workspace-root', 'test'])

  expect(fs.existsSync('test')).toBeTruthy()
  expect(fs.existsSync('project/test')).toBeTruthy()
})

test('pnpm run should include the workspace root when include-workspace-root is set to true', async () => {
  preparePackages([
    {
      location: '.',
      package: {
        scripts: {
          test: "node -e \"require('fs').writeFileSync('test','','utf8')\"",
        },
      },
    },
    {
      name: 'project',
      version: '1.0.0',
      scripts: {
        test: "node -e \"require('fs').writeFileSync('test','','utf8')\"",
      },
    },
  ])

  fs.writeFileSync('.npmrc', 'include-workspace-root', 'utf8')
  writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  await execPnpm(['-r', 'test'])

  expect(fs.existsSync('test')).toBeTruthy()
  expect(fs.existsSync('project/test')).toBeTruthy()
})

test('legacy directory filtering', async () => {
  preparePackages([
    {
      location: 'packages/project-1',
      package: {
        name: 'project-1',
        version: '1.0.0',
      },
    },
    {
      location: 'packages/project-2',
      package: {
        name: 'project-2',
        version: '1.0.0',
      },
    },
  ])

  writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })
  fs.writeFileSync('.npmrc', 'legacy-dir-filtering=true', 'utf8')

  const { stdout } = execPnpmSync(['list', '--filter=./packages', '--parseable', '--depth=-1'])
  const output = stdout.toString()
  expect(output).toContain('project-1')
  expect(output).toContain('project-2')
})

test('directory filtering', async () => {
  preparePackages([
    {
      location: 'packages/project-1',
      package: {
        name: 'project-1',
        version: '1.0.0',
      },
    },
    {
      location: 'packages/project-2',
      package: {
        name: 'project-2',
        version: '1.0.0',
      },
    },
  ])

  writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  {
    const { stdout } = execPnpmSync(['list', '--filter=./packages', '--parseable', '--depth=-1'])
    expect(stdout.toString()).toEqual('')
  }
  {
    const { stdout } = execPnpmSync(['list', '--filter=./packages/**', '--parseable', '--depth=-1'])
    const output = stdout.toString()
    expect(output).toContain('project-1')
    expect(output).toContain('project-2')
  }
})

test('run --stream should prefix with dir name', async () => {
  preparePackages([
    {
      location: '.',
      package: {
        name: 'root',
        version: '0.0.0',
        private: true,
      },
    },
    {
      location: 'packages/alfa',
      package: {
        name: 'alfa',
        version: '1.0.0',
        scripts: {
          test: "node -e \"console.log('OK')\"",
        },
      },
    },
    {
      location: 'packages/beta',
      package: {
        name: 'beta',
        version: '1.0.0',
        scripts: {
          test: "node -e \"console.log('OK')\"",
        },
      },
    },
  ])

  writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  const result = execPnpmSync([
    '--stream',
    '--filter',
    'alfa',
    '--filter',
    'beta',
    'run',
    'test',
  ])
  expect(
    result.stdout
      .toString()
      .trim()
      .split('\n')
      .sort()
      .join('\n')
  ).toBe(
    `Scope: 2 of 3 workspace projects
packages/alfa test$ node -e "console.log('OK')"
packages/alfa test: Done
packages/alfa test: OK
packages/beta test$ node -e "console.log('OK')"
packages/beta test: Done
packages/beta test: OK`
  )
  const singleResult = execPnpmSync([
    '--stream',
    '--filter',
    'alfa',
    'run',
    'test',
  ])
  expect(
    singleResult.stdout
      .toString()
      .trim()
      .split('\n')
      .sort()
      .join('\n')
  ).toBe(
    `packages/alfa test$ node -e "console.log('OK')"
packages/alfa test: Done
packages/alfa test: OK`
  )
})

test('run --reporter-hide-prefix should hide prefix', async () => {
  preparePackages([
    {
      location: '.',
      package: {
        name: 'root',
        version: '0.0.0',
        private: true,
      },
    },
    {
      location: 'packages/alfa',
      package: {
        name: 'alfa',
        version: '1.0.0',
        scripts: {
          test: "node -e \"console.log('OK')\"",
        },
      },
    },
    {
      location: 'packages/beta',
      package: {
        name: 'beta',
        version: '1.0.0',
        scripts: {
          test: "node -e \"console.log('OK')\"",
        },
      },
    },
  ])

  writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  const result = execPnpmSync([
    '--stream',
    '--reporter-hide-prefix',
    '--filter',
    'alfa',
    '--filter',
    'beta',
    'run',
    'test',
  ])
  expect(
    result.stdout
      .toString()
      .trim()
      .split('\n')
      .sort()
      .join('\n')
  ).toBe(
    `OK
OK
Scope: 2 of 3 workspace projects
packages/alfa test$ node -e "console.log('OK')"
packages/alfa test: Done
packages/beta test$ node -e "console.log('OK')"
packages/beta test: Done`
  )
  const singleResult = execPnpmSync([
    '--stream',
    '--reporter-hide-prefix',
    '--filter',
    'alfa',
    'run',
    'test',
  ])

  console.log(singleResult.stdout
    .toString())

  expect(
    singleResult.stdout
      .toString()
      .trim()
      .split('\n')
      .sort()
      .join('\n')
  ).toBe(
    `OK
packages/alfa test$ node -e "console.log('OK')"
packages/alfa test: Done`
  )
})

test('peer dependencies are resolved from the root of the workspace when a new dependency is added to a workspace project', async () => {
  const projects = preparePackages([
    {
      location: '.',
      package: {
        name: 'project-1',
        version: '1.0.0',

        dependencies: {
          ajv: '4.10.4',
        },
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',
    },
  ])

  writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  process.chdir('project-2')

  await execPnpm(['add', 'ajv-keywords@1.5.0', '--strict-peer-dependencies', '--config.resolve-peers-from-workspace-root=true'])

  const lockfile = projects['project-1'].readLockfile()
  expect(lockfile.snapshots).toHaveProperty(['ajv-keywords@1.5.0(ajv@4.10.4)'])
})

test('overrides in workspace project should be taken into account when shared-workspace-lockfiles is false', async () => {
  const projects = preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',

      pnpm: {
        overrides: {
          'is-odd': '1.0.0',
        },
      },
    },
    {
      name: 'project-2',
      version: '2.0.0',
    },
  ])

  fs.writeFileSync('.npmrc', `
shared-workspace-lockfile=false
`, 'utf8')
  writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  await execPnpm(['install'])

  const lockfile = projects['project-1'].readLockfile()
  expect(lockfile.overrides).toStrictEqual({
    'is-odd': '1.0.0',
  })
})
