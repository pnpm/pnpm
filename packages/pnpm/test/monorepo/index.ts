import prepare, { preparePackages } from '@pnpm/prepare'
import { fromDir as readPackageJsonFromDir } from '@pnpm/read-package-json'
import loadJsonFile from 'load-json-file'
import fs = require('mz/fs')
import path = require('path')
import { Shrinkwrap } from 'pnpm-shrinkwrap'
import readYamlFile from 'read-yaml-file'
import rimraf = require('rimraf-then')
import symlink from 'symlink-dir'
import tape = require('tape')
import promisifyTape from 'tape-promise'
import writeYamlFile = require('write-yaml-file')
import { execPnpm } from '../utils'

const test = promisifyTape(tape)
const testOnly = promisifyTape(tape.only)

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

  await execPnpm('link', 'project-2')

  await execPnpm('link', 'project-3', '--save-dev')

  await execPnpm('link', 'project-4', '--save-optional')

  const pkg = await import(path.resolve('package.json'))

  t.deepEqual(pkg && pkg.dependencies, { 'project-2': '^2.0.0' }, 'spec of linked package added to dependencies')
  t.deepEqual(pkg && pkg.devDependencies, { 'project-3': '^3.0.0' }, 'spec of linked package added to devDependencies')
  t.deepEqual(pkg && pkg.optionalDependencies, { 'project-4': '^4.0.0' }, 'spec of linked package added to optionalDependencies')

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

  await execPnpm('install', 'project-2')

  await execPnpm('install', 'project-3', '--save-dev')

  await execPnpm('install', 'project-4', '--save-optional')

  const pkg = await import(path.resolve('package.json'))

  t.deepEqual(pkg && pkg.dependencies, { 'project-2': '^2.0.0' }, 'spec of linked package added to dependencies')
  t.deepEqual(pkg && pkg.devDependencies, { 'project-3': '^3.0.0' }, 'spec of linked package added to devDependencies')
  t.deepEqual(pkg && pkg.optionalDependencies, { 'project-4': '^4.0.0' }, 'spec of linked package added to optionalDependencies')

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
        install: `node -e "process.stdout.write('project-1')" | json-append ../output.json`,
      },
    },
    {
      name: 'project-2',
      version: '2.0.0',

      dependencies: {
        'json-append': '1',
      },
      scripts: {
        install: `node -e "process.stdout.write('project-2')" | json-append ../output.json`,
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

  await fs.writeFile('.npmrc', 'link-workspace-packages = true', 'utf8')
  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  process.chdir('project-1')

  await execPnpm('install')

  const outputs = await import(path.resolve('..', 'output.json')) as string[]
  t.deepEqual(outputs, ['project-2', 'project-1'])

  await projects['project-1'].has('project-2')
  await projects['project-1'].has('is-negative')
  await projects['project-1'].has('is-positive')

  {
    const shr = await projects['project-1'].loadShrinkwrap()
    t.equal(shr.dependencies['project-2'], 'link:../project-2')
    t.equal(shr.devDependencies['is-negative'], 'link:../is-negative')
    t.equal(shr.optionalDependencies['is-positive'], 'link:../is-positive')
  }

  projects['is-positive'].writePackageJson({
    name: 'is-positive',
    version: '2.0.0',
  })

  await execPnpm('install')

  {
    const shr = await projects['project-1'].loadShrinkwrap()
    t.equal(shr.optionalDependencies['is-positive'], '1.0.0', 'is-positive is unlinked and installed from registry')
  }

  await execPnpm('update', 'is-negative@2.0.0')

  {
    const shr = await projects['project-1'].loadShrinkwrap()
    t.equal(shr.devDependencies['is-negative'], '2.0.0')
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
        install: `node -e "process.stdout.write('project-1')" | json-append ../output.json`,
        test: `node -e "process.stdout.write('project-1')" | json-append ../output2.json`,
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: { 'project-2': '1.0.0' },
      devDependencies: { 'json-append': '1' },
      scripts: {
        install: `node -e "process.stdout.write('project-2')" | json-append ../output.json`,
        test: `node -e "process.stdout.write('project-2')" | json-append ../output2.json`,
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',

      dependencies: { 'project-2': '1.0.0', 'project-3': '1.0.0' },
      devDependencies: { 'json-append': '1' },
      scripts: {
        install: `node -e "process.stdout.write('project-3')" | json-append ../output.json`,
        test: `node -e "process.stdout.write('project-3')" | json-append ../output2.json`,
      },
    },
  ]);
  await fs.writeFile('.npmrc', 'link-workspace-packages = true', 'utf8')
  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  process.chdir('project-1')

  await execPnpm('install')

  const outputs = await import(path.resolve('..', 'output.json')) as string[]
  t.deepEqual(outputs, ['project-2', 'project-3', 'project-1'])

  await execPnpm('recursive', 'test')

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
      devDependencies: { 'json-append': '1' },
      scripts: {
        test: `node -e "process.stdout.write('project-2')" | json-append ../output.json`,
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',

      dependencies: { 'project-2': '1.0.0' },
      devDependencies: { 'json-append': '1' },
      scripts: {
        test: `node -e "process.stdout.write('project-3')" | json-append ../output.json`,
      },
    },
    {
      name: 'project-4',
      version: '1.0.0',

      dependencies: { 'project-2': '1.0.0', 'unused-project-1': '1.0.0', 'unused-project-2': '1.0.0' },
      devDependencies: { 'json-append': '1' },
      scripts: {
        test: `node -e "process.stdout.write('project-4')" | json-append ../output.json`,
      },
    },
  ]);
  await fs.writeFile('.npmrc', 'link-workspace-packages = true', 'utf8')
  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  await execPnpm('install')

  process.chdir('project-2')

  await execPnpm('recursive', '--filter=...project-2', 'run', 'test')

  const outputs = await import(path.resolve('..', 'output.json')) as string[]
  // project-2 should be executed first, we cannot say anything about the order
  // of the last two packages.
  t.equal(outputs[0], 'project-2')

})

test('installation with --link-workspace-packages links packages even if they were previously installed from registry', async (t: tape.Test) => {
  const projects = preparePackages(t, [
    {
      name: 'project',
      version: '1.0.0',

      dependencies: {
        'is-positive': '2.0.0',
        'negative': 'npm:is-negative@1.0.0',
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

  await execPnpm('recursive', 'install', '--no-link-workspace-packages')

  {
    const shr = await projects['project'].loadShrinkwrap()
    t.equal(shr.dependencies['is-positive'], '2.0.0')
    t.equal(shr.dependencies['negative'], '/is-negative/1.0.0')
  }

  await execPnpm('recursive', 'install', '--link-workspace-packages')

  {
    const shr = await projects['project'].loadShrinkwrap()
    t.equal(shr.dependencies['is-positive'], 'link:../is-positive')
    t.equal(shr.dependencies['negative'], 'link:../is-negative')
  }
})

test('shared-workspace-shrinkwrap: installation with --link-workspace-packages links packages even if they were previously installed from registry', async (t: tape.Test) => {
  const projects = preparePackages(t, [
    {
      name: 'project',
      version: '1.0.0',

      dependencies: {
        'is-positive': '2.0.0',
        'negative': 'npm:is-negative@1.0.0',
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
  await fs.writeFile('.npmrc', 'shared-workspace-shrinkwrap = true\nlink-workspace-packages = true', 'utf8')

  await execPnpm('recursive', 'install')

  {
    const shr = await readYamlFile<Shrinkwrap>('shrinkwrap.yaml')
    t.equal(shr!.importers!.project!.dependencies!['is-positive'], '2.0.0')
    t.equal(shr!.importers!.project!.dependencies!['negative'], '/is-negative/1.0.0')
  }

  await projects['is-positive'].writePackageJson({
    name: 'is-positive',
    version: '2.0.0',
  })

  await projects['is-negative'].writePackageJson({
    name: 'is-negative',
    version: '1.0.0',
  })

  await execPnpm('recursive', 'install')

  {
    const shr = await readYamlFile<Shrinkwrap>('shrinkwrap.yaml')
    t.equal(shr.importers!.project!.dependencies!['is-positive'], 'link:../is-positive')
    t.equal(shr.importers!.project!.dependencies!['negative'], 'link:../is-negative')
  }
})

test('recursive install with link-workspace-packages and shared-workspace-shrinkwrap', async (t: tape.Test) => {
  const projects = preparePackages(t, [
    {
      name: 'is-positive',
      version: '1.0.0',

      dependencies: {
        'is-negative': '1.0.0',
        'json-append': '1',
      },
      scripts: {
        install: `node -e "process.stdout.write('is-positive')" | json-append ../output.json`,
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
        install: `node -e "process.stdout.write('project-1')" | json-append ../output.json`,
      },
    },
  ])

  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })
  await fs.writeFile(
    'is-positive/.npmrc',
    'shamefully-flatten = true\nsave-exact = true',
    'utf8',
  )
  await fs.writeFile(
    'project-1/.npmrc',
    'save-prefix = ~',
    'utf8',
  )

  await execPnpm('recursive', 'install', '--link-workspace-packages', '--shared-workspace-shrinkwrap=true', '--store', 'store')

  t.ok(projects['is-positive'].requireModule('is-negative'))
  t.ok(projects['is-positive'].requireModule('concat-stream'), 'dependencies flattened in is-positive')
  t.notOk(projects['project-1'].requireModule('is-positive/package.json').author, 'local package is linked')

  const sharedShr = await readYamlFile<Shrinkwrap>('shrinkwrap.yaml')
  t.equal(sharedShr.importers['project-1']!.devDependencies!['is-positive'], 'link:../is-positive')

  const outputs = await import(path.resolve('output.json')) as string[]
  t.deepEqual(outputs, ['is-positive', 'project-1'])

  const storeJson = await loadJsonFile<object>(path.resolve('store', '2', 'store.json'))
  t.deepEqual(storeJson['localhost+4873/is-negative/1.0.0'].length, 1, 'new connections saved in store.json')

  await execPnpm('recursive', 'install', 'pkg-with-1-dep@100.0.0', '--link-workspace-packages', '--shared-workspace-shrinkwrap=true', '--store', 'store')

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

test('recursive installation with shared-workspace-shrinkwrap and a readPackage hook', async (t) => {
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

  await execPnpm('recursive', 'install', '--shared-workspace-shrinkwrap', '--store', 'store')

  const shr = await readYamlFile<Shrinkwrap>('./shrinkwrap.yaml')
  t.ok(shr.packages!['/dep-of-pkg-with-1-dep/100.1.0'], 'new dependency added by hook')

  await execPnpm('recursive', 'install', '--shared-workspace-shrinkwrap', '--store', 'store', '--', 'project-1')

  await projects['project-1'].hasNot('project-1')
})

test('local packages should be preferred when running "pnpm link" inside a workspace', async (t) => {
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
  await fs.writeFile('.npmrc', 'link-workspace-packages = true', 'utf8')

  process.chdir('project-1')

  await execPnpm('link', '.')

  const shr = await projects['project-1'].loadShrinkwrap()

  t.equal(shr && shr.dependencies && shr.dependencies['is-positive'], 'link:../is-positive')
})

// covers https://github.com/pnpm/pnpm/issues/1437
test('shared-workspace-shrinkwrap: create shared shrinkwrap format when installation is inside workspace', async (t) => {
  const projects = prepare(t, {
    dependencies: {
      'is-positive': '1.0.0',
    },
  })

  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', 'project', '!store/**'] })
  await fs.writeFile('.npmrc', 'shared-workspace-shrinkwrap = true', 'utf8')

  await execPnpm('install', '--store', 'store')

  const shr = await readYamlFile<Shrinkwrap>('shrinkwrap.yaml')

  t.ok(shr['importers'] && shr['importers']['.'], 'correct shrinkwrap.yaml format')
  t.equal(shr['shrinkwrapVersion'], 4, 'correct shrinkwrap.yaml version')
})

// covers https://github.com/pnpm/pnpm/issues/1451
test("shared-workspace-shrinkwrap: don't install dependencies in projects that are outside of the current workspace", async (t) => {
  const projects = preparePackages(t, [
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
        name:  'package-2',
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

  await execPnpm('install', '--store', 'store', '--shared-workspace-shrinkwrap', '--link-workspace-packages')

  const shr = await readYamlFile<Shrinkwrap>('shrinkwrap.yaml')

  t.deepEqual(shr, {
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
    shrinkwrapVersion: 4,
  }, 'correct shrinkwrap.yaml created')
})

test('shared-workspace-shrinkwrap: entries of removed projects should be removed from shared shrinkwrap', async (t) => {
  const projects = preparePackages(t, [
    {
      name: 'package-1',
      version: '1.0.0',

      dependencies: {
        'is-positive': '1.0.0',
      },
    },
    {
      name:  'package-2',
      version: '1.0.0',

      dependencies: {
        'is-negative': '1.0.0',
      },
    },
  ])

  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  await execPnpm('install', '--store', 'store', '--shared-workspace-shrinkwrap', '--link-workspace-packages')

  {
    const shr = await readYamlFile<Shrinkwrap>('shrinkwrap.yaml')
    t.deepEqual(Object.keys(shr.importers), ['package-1', 'package-2'])
  }

  await rimraf('package-2')

  await execPnpm('install', '--store', 'store', '--shared-workspace-shrinkwrap', '--link-workspace-packages')

  {
    const shr = await readYamlFile<Shrinkwrap>('shrinkwrap.yaml')
    t.deepEqual(Object.keys(shr.importers), ['package-1'])
  }
})

// Covers https://github.com/pnpm/pnpm/issues/1482
test('shared-workspace-shrinkwrap config is ignored if no pnpm-workspace.yaml is found', async (t) => {
  const project = prepare(t, {
    dependencies: {
      'is-positive': '1.0.0',
    },
  })

  await fs.writeFile('.npmrc', 'shared-workspace-shrinkwrap=true', 'utf8')

  await execPnpm('install')

  t.pass('install did not fail')
  await project.has('is-positive')
})

test('shared-workspace-shrinkwrap: uninstalling a package recursively', async (t: tape.Test) => {
  const projects = preparePackages(t, [
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
  await fs.writeFile('.npmrc', 'shared-workspace-shrinkwrap = true\nlink-workspace-packages = true', 'utf8')

  await execPnpm('recursive', 'install')

  await execPnpm('recursive', 'uninstall', 'is-positive')

  {
    const pkg = await readPackageJsonFromDir('project1')

    t.notOk(pkg.dependencies, 'is-positive removed from project1')
  }

  {
    const pkg = await readPackageJsonFromDir('project2')

    t.deepEqual(pkg.dependencies, { 'is-negative': '1.0.0' }, 'is-positive removed from project2')
  }

  const shr = await readYamlFile<Shrinkwrap>('shrinkwrap.yaml')

  t.deepEqual(Object.keys(shr.packages || {}), ['/is-negative/1.0.0'], 'is-positive removed from shrinkwrap.yaml')
})
