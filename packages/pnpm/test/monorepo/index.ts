import { promises as fs } from 'fs'
import path from 'path'
import { LOCKFILE_VERSION, WANTED_LOCKFILE } from '@pnpm/constants'
import findWorkspacePackages from '@pnpm/find-workspace-packages'
import { Lockfile } from '@pnpm/lockfile-types'
import { read as readModulesManifest } from '@pnpm/modules-yaml'
import prepare, {
  prepareEmpty,
  preparePackages,
  tempDir as makeTempDir,
} from '@pnpm/prepare'
import { fromDir as readPackageJsonFromDir } from '@pnpm/read-package-json'
import readYamlFile from 'read-yaml-file'
import execa from 'execa'
import rimraf from '@zkochan/rimraf'
import exists from 'path-exists'
import tempy from 'tempy'
import symlink from 'symlink-dir'
import writeYamlFile from 'write-yaml-file'
import { execPnpm, execPnpmSync, execPnpxSync } from '../utils'

test('no projects matched the filters', async () => {
  preparePackages([
    {
      name: 'project',
      version: '1.0.0',
    },
  ])

  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  {
    const { stdout } = execPnpmSync(['list', '--filter=not-exists'])
    expect(stdout.toString()).toMatch(/^No projects matched the filters in/)
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

  await writeYamlFile('pnpm-workspace.yml', { packages: ['**', '!store/**'] })

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

  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  process.chdir('project-1')

  await execPnpm(['link', 'project-2'])

  await execPnpm(['link', 'project-3', '--save-dev'])

  await execPnpm(['link', 'project-4', '--save-optional'])

  const { default: pkg } = await import(path.resolve('package.json'))

  expect(pkg?.dependencies).toStrictEqual({ 'project-2': '^2.0.0' }) // spec of linked package added to dependencies
  expect(pkg?.devDependencies).toStrictEqual({ 'project-3': '^3.0.0' }) // spec of linked package added to devDependencies
  expect(pkg?.optionalDependencies).toStrictEqual({ 'project-4': '^4.0.0' }) // spec of linked package added to optionalDependencies

  await projects['project-1'].has('project-2')
  await projects['project-1'].has('project-3')
  await projects['project-1'].has('project-4')
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

  await fs.writeFile('.npmrc', 'link-workspace-packages = true', 'utf8')
  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  process.chdir('project-1')

  await execPnpm(['add', 'project-2'])

  await execPnpm(['add', 'project-3', '--save-dev'])

  await execPnpm(['add', 'project-4', '--save-optional', '--no-save-workspace-protocol'])

  const { default: pkg } = await import(path.resolve('package.json'))

  expect(pkg?.dependencies).toStrictEqual({ 'project-2': 'workspace:^2.0.0' }) // spec of linked package added to dependencies
  expect(pkg?.devDependencies).toStrictEqual({ 'project-3': 'workspace:^3.0.0' }) // spec of linked package added to devDependencies
  expect(pkg?.optionalDependencies).toStrictEqual({ 'project-4': '^4.0.0' }) // spec of linked package added to optionalDependencies

  await projects['project-1'].has('project-2')
  await projects['project-1'].has('project-3')
  await projects['project-1'].has('project-4')
})

test('linking a package inside a monorepo with --link-workspace-packages', async () => {
  const projects = preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'json-append': '1',
        'project-2': '2.0.0',
      },
      devDependencies: {
        'is-negative': '100.0.0',
      },
      optionalDependencies: {
        'is-positive': '1.0.0',
      },
      scripts: {
        install: 'node -e "process.stdout.write(\'project-1\')" | json-append ../output.json',
      },
    },
    {
      name: 'project-2',
      version: '2.0.0',

      dependencies: {
        'json-append': '1',
      },
      scripts: {
        install: 'node -e "process.stdout.write(\'project-2\')" | json-append ../output.json',
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

  await fs.writeFile('.npmrc', 'link-workspace-packages = true\nshared-workspace-lockfile=false', 'utf8')
  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  process.chdir('project-1')

  await execPnpm(['install'])

  const { default: outputs } = await import(path.resolve('..', 'output.json'))
  expect(outputs).toStrictEqual(['project-2', 'project-1'])

  await projects['project-1'].has('project-2')
  await projects['project-1'].has('is-negative')
  await projects['project-1'].has('is-positive')

  {
    const lockfile = await projects['project-1'].readLockfile()
    expect(lockfile.dependencies['project-2']).toBe('link:../project-2')
    expect(lockfile.devDependencies['is-negative']).toBe('link:../is-negative')
    expect(lockfile.optionalDependencies['is-positive']).toBe('link:../is-positive')
  }

  await projects['is-positive'].writePackageJson({
    name: 'is-positive',
    version: '2.0.0',
  })

  await execPnpm(['install'])

  {
    const lockfile = await projects['project-1'].readLockfile()
    expect(lockfile.optionalDependencies['is-positive']).toBe('1.0.0') // is-positive is unlinked and installed from registry
  }

  await execPnpm(['update', 'is-negative@2.0.0'])

  {
    const lockfile = await projects['project-1'].readLockfile()
    expect(lockfile.devDependencies['is-negative']).toBe('2.0.0')
  }
})

test('topological order of packages with self-dependencies in monorepo is correct', async () => {
  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: { 'project-2': '1.0.0', 'project-3': '1.0.0' },
      devDependencies: { 'json-append': '1' },
      scripts: {
        install: 'node -e "process.stdout.write(\'project-1\')" | json-append ../output.json',
        test: 'node -e "process.stdout.write(\'project-1\')" | json-append ../output2.json',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: { 'project-2': '1.0.0' },
      devDependencies: { 'json-append': '1' },
      scripts: {
        install: 'node -e "process.stdout.write(\'project-2\')" | json-append ../output.json',
        test: 'node -e "process.stdout.write(\'project-2\')" | json-append ../output2.json',
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',

      dependencies: { 'project-2': '1.0.0', 'project-3': '1.0.0' },
      devDependencies: { 'json-append': '1' },
      scripts: {
        install: 'node -e "process.stdout.write(\'project-3\')" | json-append ../output.json',
        test: 'node -e "process.stdout.write(\'project-3\')" | json-append ../output2.json',
      },
    },
  ])
  await fs.writeFile('.npmrc', 'link-workspace-packages = true', 'utf8')
  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  process.chdir('project-1')

  await execPnpm(['install'])

  const { default: outputs } = await import(path.resolve('..', 'output.json'))
  expect(outputs).toStrictEqual(['project-2', 'project-3', 'project-1'])

  await execPnpm(['recursive', 'test'])

  const { default: outputs2 } = await import(path.resolve('..', 'output2.json'))
  expect(outputs2).toStrictEqual(['project-2', 'project-3', 'project-1'])
})

test('test-pattern is respected by the test script', async () => {
  const remote = tempy.directory()

  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',
      dependencies: { 'project-2': '1.0.0', 'project-3': '1.0.0' },
      devDependencies: { 'json-append': '1' },
      scripts: {
        test: 'node -e "process.stdout.write(\'project-1\')" | json-append ../output.json',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',
      dependencies: {},
      devDependencies: { 'json-append': '1' },
      scripts: {
        test: 'node -e "process.stdout.write(\'project-2\')" | json-append ../output.json',
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',
      dependencies: { 'project-2': '1.0.0' },
      devDependencies: { 'json-append': '1' },
      scripts: {
        test: 'node -e "process.stdout.write(\'project-3\')" | json-append ../output.json',
      },
    },
    {
      name: 'project-4',
      version: '1.0.0',
      dependencies: {},
      devDependencies: { 'json-append': '1' },
      scripts: {
        test: 'node -e "process.stdout.write(\'project-4\')" | json-append ../output.json',
      },
    },
  ])

  await execa('git', ['init'])
  await execa('git', ['config', 'user.email', 'x@y.z'])
  await execa('git', ['config', 'user.name', 'xyz'])
  await execa('git', ['init', '--bare'], { cwd: remote })
  await execa('git', ['add', '*'])
  await execa('git', ['commit', '-m', 'init', '--no-gpg-sign'])
  await execa('git', ['remote', 'add', 'origin', remote])
  await execa('git', ['push', '-u', 'origin', 'master'])

  await fs.writeFile('project-2/file.js', '')
  await fs.writeFile('project-4/different-pattern.js', '')
  await fs.writeFile('.npmrc', 'test-pattern[]=*/file.js', 'utf8')
  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  await execa('git', ['add', '.'])
  await execa('git', ['commit', '--allow-empty-message', '-m', '', '--no-gpg-sign'])

  process.chdir('project-1')

  await execPnpm(['install'])

  await execPnpm(['recursive', 'test', '--filter', '...[origin/master]'])

  const { default: output } = await import(path.resolve('..', 'output.json'))
  expect(output.sort()).toStrictEqual(['project-2', 'project-4'])
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

  await execa('git', ['init'])
  await execa('git', ['config', 'user.email', 'x@y.z'])
  await execa('git', ['config', 'user.name', 'xyz'])
  await execa('git', ['init', '--bare'], { cwd: remote })
  await execa('git', ['add', '*'])
  await execa('git', ['commit', '-m', 'init', '--no-gpg-sign'])
  await execa('git', ['remote', 'add', 'origin', remote])
  await execa('git', ['push', '-u', 'origin', 'master'])

  const npmrcLines = []
  await fs.writeFile('project-2-change-is-never-ignored/index.js', '')

  npmrcLines.push('changed-files-ignore-pattern[]=**/{*.spec.js,*.md}')
  await fs.writeFile('project-3-ignored-by-pattern/index.spec.js', '')
  await fs.writeFile('project-3-ignored-by-pattern/README.md', '')

  npmrcLines.push('changed-files-ignore-pattern[]=**/buildscript.js')
  await fs.mkdir('project-4-ignored-by-pattern/a/b/c', {
    recursive: true,
  })
  await fs.writeFile('project-4-ignored-by-pattern/a/b/c/buildscript.js', '')

  npmrcLines.push('changed-files-ignore-pattern[]=**/cache/**')
  await fs.mkdir('project-5-ignored-by-pattern/cache/a/b', {
    recursive: true,
  })
  await fs.writeFile('project-5-ignored-by-pattern/cache/a/b/index.js', '')

  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  await execa('git', ['add', '.'])
  await execa('git', [
    'commit',
    '--allow-empty-message',
    '-m',
    '',
    '--no-gpg-sign',
  ])

  await fs.writeFile('.npmrc', npmrcLines.join('\n'), 'utf8')
  await execPnpm(['install'])

  const getChangedProjects = async (opts?: {
    overrideChangedFilesIgnorePatternWithNoPattern: boolean
  }) => {
    const result = await execPnpmSync(
      [
        '--filter',
        '[origin/master]',
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
  await fs.writeFile('.npmrc', 'link-workspace-packages = true', 'utf8')
  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

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
    const lockfile = await projects.project.readLockfile()
    expect(lockfile.dependencies['is-positive']).toBe('2.0.0')
    expect(lockfile.dependencies.negative).toBe('/is-negative/1.0.0')
  }

  await execPnpm(['recursive', 'install', '--link-workspace-packages'])

  {
    const lockfile = await projects.project.readLockfile()
    expect(lockfile.dependencies['is-positive']).toBe('link:../is-positive')
    expect(lockfile.dependencies.negative).toBe('link:../is-negative')
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

  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })
  await fs.writeFile('.npmrc', 'shared-workspace-lockfile = true\nlink-workspace-packages = true', 'utf8')

  await execPnpm(['recursive', 'install'])

  {
    const lockfile = await readYamlFile<Lockfile>(WANTED_LOCKFILE)
    expect(lockfile.importers.project!.dependencies!['is-positive']).toBe('2.0.0')
    expect(lockfile.importers.project!.dependencies!.negative).toBe('/is-negative/1.0.0')
  }

  await projects['is-positive'].writePackageJson({
    name: 'is-positive',
    version: '2.0.0',
  })

  await projects['is-negative'].writePackageJson({
    name: 'is-negative',
    version: '1.0.0',
  })

  await execPnpm(['recursive', 'install'])

  {
    const lockfile = await readYamlFile<Lockfile>(WANTED_LOCKFILE)
    expect(lockfile.importers.project!.dependencies!['is-positive']).toBe('link:../is-positive')
    expect(lockfile.importers.project!.dependencies!.negative).toBe('link:../is-negative')
  }
})

test('recursive install with link-workspace-packages and shared-workspace-lockfile', async () => {
  const projects = preparePackages([
    {
      name: 'is-positive',
      version: '1.0.0',

      dependencies: {
        'is-negative': '1.0.0',
        'json-append': '1',
      },
      scripts: {
        install: 'node -e "process.stdout.write(\'is-positive\')" | json-append ../output.json',
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
        'json-append': '1',
      },
      scripts: {
        install: 'node -e "process.stdout.write(\'project-1\')" | json-append ../output.json',
      },
    },
  ])

  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })
  await fs.writeFile(
    'is-positive/.npmrc',
    'save-exact = true',
    'utf8'
  )
  await fs.writeFile(
    'project-1/.npmrc',
    'save-prefix = ~',
    'utf8'
  )

  await execPnpm(['recursive', 'install', '--link-workspace-packages', '--shared-workspace-lockfile=true', '--store-dir', 'store'])

  expect(projects['is-positive'].requireModule('is-negative')).toBeTruthy()
  expect(projects['project-1'].requireModule('is-positive/package.json').author).toBeFalsy()

  const sharedLockfile = await readYamlFile<Lockfile>(WANTED_LOCKFILE)
  expect(sharedLockfile.importers['project-1']!.devDependencies!['is-positive']).toBe('link:../is-positive')

  const { default: outputs } = await import(path.resolve('output.json'))
  expect(outputs).toStrictEqual(['is-positive', 'project-1'])

  await execPnpm(['recursive', 'install', 'pkg-with-1-dep', '--link-workspace-packages', '--shared-workspace-lockfile=true', '--store-dir', 'store'])

  {
    const pkg = await readPackageJsonFromDir(path.resolve('is-positive'))
    expect(pkg.dependencies!['pkg-with-1-dep']).toBe('100.0.0')
  }

  {
    const pkg = await readPackageJsonFromDir(path.resolve('project-1'))
    expect(pkg.dependencies!['pkg-with-1-dep']).toBe('~100.0.0')
  }

  {
    const pkg = await readPackageJsonFromDir(path.resolve('is-positive2'))
    expect(pkg.dependencies!['pkg-with-1-dep']).toBe('^100.0.0')
  }
})

test('recursive install with shared-workspace-lockfile builds workspace projects in correct order', async () => {
  const jsonAppend = (append: string, target: string) => `node -e "process.stdout.write('${append}')" | json-append ${target}`
  preparePackages([
    {
      name: 'project-999',
      version: '1.0.0',

      dependencies: {
        'json-append': '1',
      },
      scripts: {
        install: `${jsonAppend('project-999-install', '../output1.json')} && ${jsonAppend('project-999-install', '../output2.json')}`,
        postinstall: `${jsonAppend('project-999-postinstall', '../output1.json')} && ${jsonAppend('project-999-postinstall', '../output2.json')}`,
        prepare: `${jsonAppend('project-999-prepare', '../output1.json')} && ${jsonAppend('project-999-prepare', '../output2.json')}`,
        prepublish: `${jsonAppend('project-999-prepublish', '../output1.json')} && ${jsonAppend('project-999-prepublish', '../output2.json')}`,
      },
    },
    {
      name: 'project-1',
      version: '1.0.0',

      devDependencies: {
        'json-append': '1',
        'project-999': '1.0.0',
      },
      scripts: {
        install: jsonAppend('project-1-install', '../output1.json'),
        postinstall: jsonAppend('project-1-postinstall', '../output1.json'),
        prepare: jsonAppend('project-1-prepare', '../output1.json'),
        prepublish: jsonAppend('project-1-prepublish', '../output1.json'),
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      devDependencies: {
        'json-append': '1',
        'project-999': '1.0.0',
      },
      scripts: {
        install: jsonAppend('project-2-install', '../output2.json'),
        postinstall: jsonAppend('project-2-postinstall', '../output2.json'),
        prepare: jsonAppend('project-2-prepare', '../output2.json'),
        prepublish: jsonAppend('project-2-prepublish', '../output2.json'),
      },
    },
  ], { manifestFormat: 'YAML' })

  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  await execPnpm(['recursive', 'install', '--link-workspace-packages', '--shared-workspace-lockfile=true', '--store-dir', 'store'])

  {
    const { default: outputs1 } = await import(path.resolve('output1.json'))
    expect(outputs1).toStrictEqual([
      'project-999-install',
      'project-999-postinstall',
      'project-999-prepare',
      'project-1-install',
      'project-1-postinstall',
      'project-1-prepare',
    ])

    const { default: outputs2 } = await import(path.resolve('output2.json'))
    expect(outputs2).toStrictEqual([
      'project-999-install',
      'project-999-postinstall',
      'project-999-prepare',
      'project-2-install',
      'project-2-postinstall',
      'project-2-prepare',
    ])
  }

  await rimraf('node_modules')
  await rimraf('output1.json')
  await rimraf('output2.json')

  // TODO: duplicate this test in @pnpm/headless
  await execPnpm(['recursive', 'install', '--frozen-lockfile', '--link-workspace-packages', '--shared-workspace-lockfile=true'])

  {
    const { default: outputs1 } = await import(path.resolve('output1.json'))
    expect(outputs1).toStrictEqual([
      'project-999-install',
      'project-999-postinstall',
      'project-999-prepare',
      'project-1-install',
      'project-1-postinstall',
      'project-1-prepare',
    ])

    const { default: outputs2 } = await import(path.resolve('output2.json'))
    expect(outputs2).toStrictEqual([
      'project-999-install',
      'project-999-postinstall',
      'project-999-prepare',
      'project-2-install',
      'project-2-postinstall',
      'project-2-prepare',
    ])
  }
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
      pkg.dependencies['dep-of-pkg-with-1-dep'] = '100.1.0'
      return pkg
    }
  `
  await fs.writeFile('.pnpmfile.cjs', pnpmfile, 'utf8')
  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  await execPnpm(['recursive', 'install', '--shared-workspace-lockfile', '--store-dir', 'store'])

  const lockfile = await readYamlFile<Lockfile>(`./${WANTED_LOCKFILE}`)
  expect(lockfile.packages).toHaveProperty(['/dep-of-pkg-with-1-dep/100.1.0'])

  await execPnpm(['recursive', 'install', '--shared-workspace-lockfile', '--store-dir', 'store', '--filter', 'project-1'])

  await projects['project-1'].hasNot('project-1')
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

  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })
  await fs.writeFile('.npmrc', 'link-workspace-packages = true\nshared-workspace-lockfile=false', 'utf8')

  process.chdir('project-1')

  await execPnpm(['link', '.'])

  const lockfile = await projects['project-1'].readLockfile()

  expect(lockfile?.dependencies?.['is-positive']).toBe('link:../is-positive')
})

// covers https://github.com/pnpm/pnpm/issues/1437
test('shared-workspace-lockfile: create shared lockfile format when installation is inside workspace', async () => {
  prepare({
    dependencies: {
      'is-positive': '1.0.0',
    },
  })

  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', 'project', '!store/**'] })
  await fs.writeFile('.npmrc', 'shared-workspace-lockfile = true', 'utf8')

  await execPnpm(['install', '--store-dir', 'store'])

  const lockfile = await readYamlFile<Lockfile>(WANTED_LOCKFILE)

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
          'package-2': '1.0.0',
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

  process.chdir('..')

  await symlink('workspace-2/package-2', 'workspace-1/package-2')

  await writeYamlFile('workspace-1/pnpm-workspace.yaml', { packages: ['**', '!store/**'] })
  await writeYamlFile('workspace-2/pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  process.chdir('workspace-1')

  await execPnpm(['recursive', 'install', '--store-dir', 'store', '--shared-workspace-lockfile', '--link-workspace-packages'])

  const lockfile = await readYamlFile<Lockfile>(WANTED_LOCKFILE)

  expect(lockfile).toStrictEqual({
    importers: {
      'package-1': {
        dependencies: {
          'is-positive': '1.0.0',
          'package-2': 'link:../package-2',
        },
        specifiers: {
          'is-positive': '1.0.0',
          'package-2': '1.0.0',
        },
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      '/is-positive/1.0.0': {
        dev: false,
        engines: {
          node: '>=0.10.0',
        },
        resolution: {
          integrity: 'sha1-iACYVrZKLx632LsBeUGEJK4EUss=',
        },
      },
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

  process.chdir('..')

  await writeYamlFile('monorepo/workspace/pnpm-workspace.yaml', { packages: ['../**', '!store/**'] })

  process.chdir('monorepo/workspace')

  await execPnpm(['recursive', 'install', '--store-dir', 'store', '--shared-workspace-lockfile', '--link-workspace-packages'])

  const lockfile = await readYamlFile<Lockfile>(WANTED_LOCKFILE)

  expect(lockfile).toStrictEqual({
    importers: {
      '.': {
        dependencies: {
          'package-1': 'link:../package-1',
          'package-2': 'link:../package-2',
        },
        specifiers: {
          'package-1': '1.0.0',
          'package-2': '1.0.0',
        },
      },
      '../package-1': {
        dependencies: {
          'is-positive': '1.0.0',
          'package-2': 'link:../package-2',
        },
        specifiers: {
          'is-positive': '1.0.0',
          'package-2': '1.0.0',
        },
      },
      '../package-2': {
        dependencies: {
          'is-negative': '1.0.0',
        },
        specifiers: {
          'is-negative': '1.0.0',
        },
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      '/is-negative/1.0.0': {
        dev: false,
        engines: {
          node: '>=0.10.0',
        },
        resolution: {
          integrity: 'sha1-clmHeoPIAKwxkd17nZ+80PdS1P4=',
        },
      },
      '/is-positive/1.0.0': {
        dev: false,
        engines: {
          node: '>=0.10.0',
        },
        resolution: {
          integrity: 'sha1-iACYVrZKLx632LsBeUGEJK4EUss=',
        },
      },
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

  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  await execPnpm(['recursive', 'install', '--store-dir', 'store', '--shared-workspace-lockfile', '--link-workspace-packages'])

  {
    const lockfile = await readYamlFile<Lockfile>(WANTED_LOCKFILE)
    expect(Object.keys(lockfile.importers)).toStrictEqual(['package-1', 'package-2'])
  }

  await rimraf('package-2')

  await execPnpm(['recursive', 'install', '--store-dir', 'store', '--shared-workspace-lockfile', '--link-workspace-packages'])

  {
    const lockfile = await readYamlFile<Lockfile>(WANTED_LOCKFILE)
    expect(Object.keys(lockfile.importers)).toStrictEqual(['package-1'])
  }
})

// Covers https://github.com/pnpm/pnpm/issues/1482
test('shared-workspace-lockfile config is ignored if no pnpm-workspace.yaml is found', async () => {
  const project = prepare({
    dependencies: {
      'is-positive': '1.0.0',
    },
  })

  await fs.writeFile('.npmrc', 'shared-workspace-lockfile=true', 'utf8')

  await execPnpm(['install'])

  await project.has('is-positive')
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

  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })
  await fs.writeFile('.npmrc', 'shared-workspace-lockfile = true\nlink-workspace-packages = true', 'utf8')

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

  const lockfile = await readYamlFile<Lockfile>(WANTED_LOCKFILE)

  expect(Object.keys(lockfile.packages ?? {})).toStrictEqual(['/is-negative/1.0.0']) // is-positive removed from ${WANTED_LOCKFILE}
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

  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })
  await fs.writeFile('.npmrc', 'shared-workspace-lockfile = true\nlink-workspace-packages = true', 'utf8')

  process.chdir('foo')

  await execPnpm(['install', 'ajv@4.10.4', 'ajv-keywords@1.5.0'])

  {
    const lockfile = await readYamlFile<Lockfile>(path.resolve('..', WANTED_LOCKFILE))
    expect(lockfile.importers.foo).toStrictEqual({
      dependencies: {
        ajv: '4.10.4',
        'ajv-keywords': '1.5.0_ajv@4.10.4',
        bar: 'link:../bar',
      },
      specifiers: {
        ajv: '4.10.4',
        'ajv-keywords': '1.5.0',
        bar: 'workspace:1.0.0',
      },
    })
  }

  await execPnpm(['uninstall', 'ajv'])

  {
    const lockfile = await readYamlFile<Lockfile>(path.resolve('..', WANTED_LOCKFILE))
    expect(lockfile.importers.foo).toStrictEqual({
      dependencies: {
        'ajv-keywords': '1.5.0',
        bar: 'link:../bar',
      },
      specifiers: {
        'ajv-keywords': '1.5.0',
        bar: 'workspace:1.0.0',
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
        'pre-and-postinstall-scripts-example': '1.0.0',
      },
    },
  ])

  await fs.writeFile('.npmrc', 'shared-workspace-lockfile=false', 'utf8')
  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  await execPnpm(['recursive', 'install', '--lockfile-only'])
  await execPnpm(['recursive', 'install', '--frozen-lockfile'])

  {
    const generatedByPreinstall = projects['project-1'].requireModule('pre-and-postinstall-scripts-example/generated-by-preinstall')
    expect(typeof generatedByPreinstall).toBe('function')

    const generatedByPostinstall = projects['project-1'].requireModule('pre-and-postinstall-scripts-example/generated-by-postinstall')
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
        hello: '1.0.0',
      },
    },
  ], { manifestFormat: 'YAML' })

  await fs.writeFile('./hello/index.js', '#!/usr/bin/env node', 'utf8')

  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  await execPnpm(['recursive', 'install'])

  await projects.main.isExecutable('.bin/hello')

  expect(await exists('main/node_modules')).toBeTruthy()
  await rimraf('main/node_modules')

  await execPnpm(['recursive', 'install', '--frozen-lockfile'])

  await projects.main.isExecutable('.bin/hello')
})

test('pnpx sees the bins from the root of the workspace', async () => {
  preparePackages([
    {
      location: '.',
      package: {
        dependencies: {
          'print-version': '2',
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
        'print-version': '1',
      },
    },
  ])

  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  await execPnpm(['recursive', 'install'])

  process.chdir('project-1')

  const result = execPnpxSync(['print-version'])

  expect(result.stdout.toString()).toContain('2.0.0')

  process.chdir('../project-2')

  expect(execPnpxSync(['print-version']).stdout.toString()).toContain('1.0.0')
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
  await writeYamlFile('pnpm-workspace.yaml', { packages: ['project-', '!store/**'] })
  const workspacePackages = await findWorkspacePackages(tempDir, { engineStrict: false })

  expect(workspacePackages.some(project => {
    const relativePath = path.join('.', path.relative(tempDir, project.dir))
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
  await writeYamlFile('pnpm-workspace.yaml', { packages: ['project-', '!.', '!./', '!store/**'] })
  const workspacePackages = await findWorkspacePackages(tempDir, { engineStrict: false })

  expect(workspacePackages.some(project => {
    const relativePath = path.join('.', path.relative(tempDir, project.dir))
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

  await fs.writeFile('.npmrc', 'virtual-store-dir=virtual-store\nshared-workspace-lockfile=false', 'utf8')
  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  await execPnpm(['install'])

  {
    const modulesManifest = await projects['project-1'].readModulesManifest()
    expect(modulesManifest?.virtualStoreDir).toBe(path.resolve('project-1/virtual-store'))
  }

  await rimraf('project-1/virtual-store')
  await rimraf('project-1/node_modules')

  await execPnpm(['install', '--frozen-lockfile'])

  {
    const modulesManifest = await projects['project-1'].readModulesManifest()
    expect(modulesManifest?.virtualStoreDir).toBe(path.resolve('project-1/virtual-store'))
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

  await fs.writeFile('.npmrc', 'virtual-store-dir=virtual-store\nshared-workspace-lockfile=true', 'utf8')
  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  await execPnpm(['install'])

  {
    const modulesManifest = await readModulesManifest(path.resolve('node_modules'))
    expect(modulesManifest?.virtualStoreDir).toBe(path.resolve('virtual-store'))
  }

  await rimraf('virtual-store')
  await rimraf('node_modules')

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

  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  await execPnpm(['-r', '--config.use-beta-cli=true', 'test'])

  expect(await exists('project/test')).toBeTruthy()
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

  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  await execPnpm(['--filter=project', '--workspace-root', 'test'])

  expect(await exists('test')).toBeTruthy()
  expect(await exists('project/test')).toBeTruthy()
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

  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  process.chdir('project-2')

  await execPnpm(['add', 'ajv-keywords@1.5.0', '--strict-peer-dependencies'])

  const lockfile = await projects['project-1'].readLockfile()
  expect(lockfile.packages).toHaveProperty(['/ajv-keywords/1.5.0_ajv@4.10.4'])
})
