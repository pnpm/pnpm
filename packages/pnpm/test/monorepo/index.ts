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
import promisifyTape from 'tape-promise'
import { execPnpm, execPnpmSync, execPnpxSync } from '../utils'
import path = require('path')
import rimraf = require('@zkochan/rimraf')
import fs = require('mz/fs')
import exists = require('path-exists')
import symlink = require('symlink-dir')
import tape = require('tape')
import writeYamlFile = require('write-yaml-file')

const test = promisifyTape(tape)

test('no projects matched the filters', async (t) => {
  preparePackages(t, [
    {
      name: 'project',
      version: '1.0.0',
    },
  ])

  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  {
    const { stdout } = execPnpmSync(['list', '--filter=not-exists'])
    t.ok(stdout.toString().startsWith('No projects matched the filters in'), // eslint-disable-line
      'print info message')
  }
  {
    const { stdout } = execPnpmSync(['list', '--filter=not-exists', '--parseable'])
    t.equal(stdout.toString(), '', "don't print anything if --parseable is used") // eslint-disable-line
  }
})

test('no projects found', async (t) => {
  prepareEmpty(t)

  {
    const { stdout } = execPnpmSync(['list', '-r'])
    t.ok(stdout.toString().startsWith('No projects found in'),
      'print info message')
  }
  {
    const { stdout } = execPnpmSync(['list', '-r', '--parseable'])
    t.equal(stdout.toString(), '', "don't print anything if --parseable is used")
  }
})

test('linking a package inside a monorepo', async (t: tape.Test) => {
  const projects = preparePackages(t, [
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

  const pkg = await import(path.resolve('package.json'))

  t.deepEqual(pkg?.dependencies, { 'project-2': '^2.0.0' }, 'spec of linked package added to dependencies')
  t.deepEqual(pkg?.devDependencies, { 'project-3': '^3.0.0' }, 'spec of linked package added to devDependencies')
  t.deepEqual(pkg?.optionalDependencies, { 'project-4': '^4.0.0' }, 'spec of linked package added to optionalDependencies')

  await projects['project-1'].has('project-2')
  await projects['project-1'].has('project-3')
  await projects['project-1'].has('project-4')
})

test('linking a package inside a monorepo with --link-workspace-packages when installing new dependencies', async (t: tape.Test) => {
  const projects = preparePackages(t, [
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

  const pkg = await import(path.resolve('package.json'))

  t.deepEqual(pkg?.dependencies, { 'project-2': 'workspace:^2.0.0' }, 'spec of linked package added to dependencies')
  t.deepEqual(pkg?.devDependencies, { 'project-3': 'workspace:^3.0.0' }, 'spec of linked package added to devDependencies')
  t.deepEqual(pkg?.optionalDependencies, { 'project-4': '^4.0.0' }, 'spec of linked package added to optionalDependencies')

  await projects['project-1'].has('project-2')
  await projects['project-1'].has('project-3')
  await projects['project-1'].has('project-4')
})

test('linking a package inside a monorepo with --link-workspace-packages', async (t: tape.Test) => {
  const projects = preparePackages(t, [
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

  const outputs = await import(path.resolve('..', 'output.json')) as string[]
  t.deepEqual(outputs, ['project-2', 'project-1'])

  await projects['project-1'].has('project-2')
  await projects['project-1'].has('is-negative')
  await projects['project-1'].has('is-positive')

  {
    const lockfile = await projects['project-1'].readLockfile()
    t.equal(lockfile.dependencies['project-2'], 'link:../project-2')
    t.equal(lockfile.devDependencies['is-negative'], 'link:../is-negative')
    t.equal(lockfile.optionalDependencies['is-positive'], 'link:../is-positive')
  }

  await projects['is-positive'].writePackageJson({
    name: 'is-positive',
    version: '2.0.0',
  })

  await execPnpm(['install'])

  {
    const lockfile = await projects['project-1'].readLockfile()
    t.equal(lockfile.optionalDependencies['is-positive'], '1.0.0', 'is-positive is unlinked and installed from registry')
  }

  await execPnpm(['update', 'is-negative@2.0.0'])

  {
    const lockfile = await projects['project-1'].readLockfile()
    t.equal(lockfile.devDependencies['is-negative'], '2.0.0')
  }
})

test('topological order of packages with self-dependencies in monorepo is correct', async (t: tape.Test) => {
  preparePackages(t, [
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

  const outputs = await import(path.resolve('..', 'output.json')) as string[]
  t.deepEqual(outputs, ['project-2', 'project-3', 'project-1'])

  await execPnpm(['recursive', 'test'])

  const outputs2 = await import(path.resolve('..', 'output2.json')) as string[]
  t.deepEqual(outputs2, ['project-2', 'project-3', 'project-1'])
})

test('do not get confused by filtered dependencies when searching for dependents in monorepo', async (t: tape.Test) => {
  /*
   In this test case, we are filtering for 'project-2' and its dependents with
   two projects in the dependency hierarchy, that can be ignored for this query,
   as they do not depend on 'project-2'.
  */
  preparePackages(t, [
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
  t.ok(project2Output < project3Output)
  t.ok(project2Output < project4Output)
})

test('installation with --link-workspace-packages links packages even if they were previously installed from registry', async (t: tape.Test) => {
  const projects = preparePackages(t, [
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
    t.equal(lockfile.dependencies['is-positive'], '2.0.0')
    t.equal(lockfile.dependencies.negative, '/is-negative/1.0.0')
  }

  await execPnpm(['recursive', 'install', '--link-workspace-packages'])

  {
    const lockfile = await projects.project.readLockfile()
    t.equal(lockfile.dependencies['is-positive'], 'link:../is-positive')
    t.equal(lockfile.dependencies.negative, 'link:../is-negative')
  }
})

test('shared-workspace-lockfile: installation with --link-workspace-packages links packages even if they were previously installed from registry', async (t: tape.Test) => {
  const projects = preparePackages(t, [
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
    t.equal(lockfile.importers.project!.dependencies!['is-positive'], '2.0.0')
    t.equal(lockfile.importers.project!.dependencies!.negative, '/is-negative/1.0.0')
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
    t.equal(lockfile.importers.project!.dependencies!['is-positive'], 'link:../is-positive')
    t.equal(lockfile.importers.project!.dependencies!.negative, 'link:../is-negative')
  }
})

test('recursive install with link-workspace-packages and shared-workspace-lockfile', async (t: tape.Test) => {
  const projects = preparePackages(t, [
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

  t.ok(projects['is-positive'].requireModule('is-negative'))
  t.notOk(projects['project-1'].requireModule('is-positive/package.json').author, 'local package is linked')

  const sharedLockfile = await readYamlFile<Lockfile>(WANTED_LOCKFILE)
  t.equal(sharedLockfile.importers['project-1']!.devDependencies!['is-positive'], 'link:../is-positive')

  const outputs = await import(path.resolve('output.json')) as string[]
  t.deepEqual(outputs, ['is-positive', 'project-1'])

  await execPnpm(['recursive', 'install', 'pkg-with-1-dep', '--link-workspace-packages', '--shared-workspace-lockfile=true', '--store-dir', 'store'])

  {
    const pkg = await readPackageJsonFromDir(path.resolve('is-positive'))
    t.equal(pkg.dependencies!['pkg-with-1-dep'], '100.0.0')
  }

  {
    const pkg = await readPackageJsonFromDir(path.resolve('project-1'))
    t.equal(pkg.dependencies!['pkg-with-1-dep'], '~100.0.0')
  }

  {
    const pkg = await readPackageJsonFromDir(path.resolve('is-positive2'))
    t.equal(pkg.dependencies!['pkg-with-1-dep'], '^100.0.0')
  }
})

test('recursive install with shared-workspace-lockfile builds workspace projects in correct order', async (t: tape.Test) => {
  const jsonAppend = (append: string, target: string) => `node -e "process.stdout.write('${append}')" | json-append ${target}`
  preparePackages(t, [
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
    const outputs1 = await import(path.resolve('output1.json')) as string[]
    t.deepEqual(
      outputs1,
      [
        'project-999-install',
        'project-999-postinstall',
        'project-999-prepublish',
        'project-999-prepare',
        'project-1-install',
        'project-1-postinstall',
        'project-1-prepublish',
        'project-1-prepare',
      ]
    )

    const outputs2 = await import(path.resolve('output2.json')) as string[]
    t.deepEqual(
      outputs2,
      [
        'project-999-install',
        'project-999-postinstall',
        'project-999-prepublish',
        'project-999-prepare',
        'project-2-install',
        'project-2-postinstall',
        'project-2-prepublish',
        'project-2-prepare',
      ]
    )
  }

  await rimraf('node_modules')
  await rimraf('output1.json')
  await rimraf('output2.json')

  // TODO: duplicate this test in @pnpm/headless
  await execPnpm(['recursive', 'install', '--frozen-lockfile', '--link-workspace-packages', '--shared-workspace-lockfile=true'])

  {
    const outputs1 = await import(path.resolve('output1.json')) as string[]
    t.deepEqual(
      outputs1,
      [
        'project-999-install',
        'project-999-postinstall',
        'project-999-prepublish',
        'project-999-prepare',
        'project-1-install',
        'project-1-postinstall',
        'project-1-prepublish',
        'project-1-prepare',
      ]
    )

    const outputs2 = await import(path.resolve('output2.json')) as string[]
    t.deepEqual(
      outputs2,
      [
        'project-999-install',
        'project-999-postinstall',
        'project-999-prepublish',
        'project-999-prepare',
        'project-2-install',
        'project-2-postinstall',
        'project-2-prepublish',
        'project-2-prepare',
      ]
    )
  }
})

test('recursive installation with shared-workspace-lockfile and a readPackage hook', async (t) => {
  const projects = preparePackages(t, [
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
  await fs.writeFile('pnpmfile.js', pnpmfile, 'utf8')
  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  await execPnpm(['recursive', 'install', '--shared-workspace-lockfile', '--store-dir', 'store'])

  const lockfile = await readYamlFile<Lockfile>(`./${WANTED_LOCKFILE}`)
  t.ok(lockfile.packages!['/dep-of-pkg-with-1-dep/100.1.0'], 'new dependency added by hook')

  await execPnpm(['recursive', 'install', '--shared-workspace-lockfile', '--store-dir', 'store', '--filter', 'project-1'])

  await projects['project-1'].hasNot('project-1')
})

test('local packages should be preferred when running "pnpm install" inside a workspace', async (t) => {
  const projects = preparePackages(t, [
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

  t.equal(lockfile?.dependencies?.['is-positive'], 'link:../is-positive')
})

// covers https://github.com/pnpm/pnpm/issues/1437
test('shared-workspace-lockfile: create shared lockfile format when installation is inside workspace', async (t) => {
  prepare(t, {
    dependencies: {
      'is-positive': '1.0.0',
    },
  })

  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', 'project', '!store/**'] })
  await fs.writeFile('.npmrc', 'shared-workspace-lockfile = true', 'utf8')

  await execPnpm(['install', '--store-dir', 'store'])

  const lockfile = await readYamlFile<Lockfile>(WANTED_LOCKFILE)

  t.ok(lockfile.importers?.['.'], `correct ${WANTED_LOCKFILE} format`)
  t.equal(lockfile.lockfileVersion, LOCKFILE_VERSION, `correct ${WANTED_LOCKFILE} version`)
})

// covers https://github.com/pnpm/pnpm/issues/1451
test("shared-workspace-lockfile: don't install dependencies in projects that are outside of the current workspace", async (t) => {
  preparePackages(t, [
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

  t.deepEqual(lockfile, {
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
  }, `correct ${WANTED_LOCKFILE} created`)
})

test('shared-workspace-lockfile: install dependencies in projects that are relative to the workspace directory', async (t) => {
  preparePackages(t, [
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

  t.deepEqual(lockfile, {
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
  }, `correct ${WANTED_LOCKFILE} created`)
})

test('shared-workspace-lockfile: entries of removed projects should be removed from shared lockfile', async (t) => {
  preparePackages(t, [
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
    t.deepEqual(Object.keys(lockfile.importers), ['package-1', 'package-2'])
  }

  await rimraf('package-2')

  await execPnpm(['recursive', 'install', '--store-dir', 'store', '--shared-workspace-lockfile', '--link-workspace-packages'])

  {
    const lockfile = await readYamlFile<Lockfile>(WANTED_LOCKFILE)
    t.deepEqual(Object.keys(lockfile.importers), ['package-1'])
  }
})

// Covers https://github.com/pnpm/pnpm/issues/1482
test('shared-workspace-lockfile config is ignored if no pnpm-workspace.yaml is found', async (t) => {
  const project = prepare(t, {
    dependencies: {
      'is-positive': '1.0.0',
    },
  })

  await fs.writeFile('.npmrc', 'shared-workspace-lockfile=true', 'utf8')

  await execPnpm(['install'])

  t.pass('install did not fail')
  await project.has('is-positive')
})

test('shared-workspace-lockfile: removing a package recursively', async (t: tape.Test) => {
  preparePackages(t, [
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

    t.notOk(pkg.dependencies, 'is-positive removed from project1')
  }

  {
    const pkg = await readPackageJsonFromDir('project2')

    t.deepEqual(pkg.dependencies, { 'is-negative': '1.0.0' }, 'is-positive removed from project2')
  }

  const lockfile = await readYamlFile<Lockfile>(WANTED_LOCKFILE)

  t.deepEqual(Object.keys(lockfile.packages ?? {}), ['/is-negative/1.0.0'], `is-positive removed from ${WANTED_LOCKFILE}`)
})

// Covers https://github.com/pnpm/pnpm/issues/1506
test('peer dependency is grouped with dependent when the peer is a top dependency and external node_modules is used', async (t: tape.Test) => {
  preparePackages(t, [
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
    t.deepEqual(lockfile.importers.foo, {
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
    t.deepEqual(lockfile.importers.foo, {
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

test('dependencies of workspace projects are built during headless installation', async (t: tape.Test) => {
  const projects = preparePackages(t, [
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
    t.ok(typeof generatedByPreinstall === 'function', 'generatedByPreinstall() is available')

    const generatedByPostinstall = projects['project-1'].requireModule('pre-and-postinstall-scripts-example/generated-by-postinstall')
    t.ok(typeof generatedByPostinstall === 'function', 'generatedByPostinstall() is available')
  }
})

test("linking the package's bin to another workspace package in a monorepo", async (t: tape.Test) => {
  const projects = preparePackages(t, [
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

  t.ok(await exists('main/node_modules'))
  await rimraf('main/node_modules')

  await execPnpm(['recursive', 'install', '--frozen-lockfile'])

  await projects.main.isExecutable('.bin/hello')
})

test('pnpx sees the bins from the root of the workspace', async (t: tape.Test) => {
  preparePackages(t, [
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

  t.ok(result.stdout.toString().includes('2.0.0'), 'bin from workspace root is found')

  process.chdir('../project-2')

  t.ok(execPnpxSync(['print-version']).stdout.toString().includes('1.0.0'), "workspace package's bin has priority")
})

test('root package is included when not specified', async (t: tape.Test) => {
  const tempDir = makeTempDir(t)
  Object.assign(
    {
      '.': prepare(t, undefined, { tempDir }),
    },
    preparePackages(
      t,
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

  t.ok(workspacePackages.some(project => {
    const relativePath = path.join('.', path.relative(tempDir, project.dir))
    return relativePath === '.' && project.manifest.name === 'project'
  }), 'root project is present even if not specified')
})

test("root package can't be ignored using '!.' (or any other such glob)", async (t: tape.Test) => {
  const tempDir = makeTempDir(t)
  Object.assign(
    {
      '.': prepare(t, undefined, { tempDir }),
    },
    preparePackages(
      t,
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

  t.ok(workspacePackages.some(project => {
    const relativePath = path.join('.', path.relative(tempDir, project.dir))
    return relativePath === '.' && project.manifest.name === 'project'
  }), 'root project is present even when explicitly ignored')
})

test('custom virtual store directory in a workspace with not shared lockfile', async (t: tape.Test) => {
  const projects = preparePackages(t, [
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
    t.equal(modulesManifest?.virtualStoreDir, path.resolve('project-1/virtual-store'))
  }

  await rimraf('project-1/virtual-store')
  await rimraf('project-1/node_modules')

  await execPnpm(['install', '--frozen-lockfile'])

  {
    const modulesManifest = await projects['project-1'].readModulesManifest()
    t.equal(modulesManifest?.virtualStoreDir, path.resolve('project-1/virtual-store'))
  }
})

test('custom virtual store directory in a workspace with shared lockfile', async (t: tape.Test) => {
  preparePackages(t, [
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
    t.equal(modulesManifest?.virtualStoreDir, path.resolve('virtual-store'))
  }

  await rimraf('virtual-store')
  await rimraf('node_modules')

  await execPnpm(['install', '--frozen-lockfile'])

  {
    const modulesManifest = await readModulesManifest(path.resolve('node_modules'))
    t.equal(modulesManifest?.virtualStoreDir, path.resolve('virtual-store'))
  }
})
