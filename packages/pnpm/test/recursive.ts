import {stripIndent, stripIndents} from 'common-tags'
import fs = require('mz/fs')
import isCI = require('is-ci')
import isWindows = require('is-windows')
import tape = require('tape')
import promisifyTape from 'tape-promise'
import path = require('path')
import exists = require('path-exists')
import writeJsonFile = require('write-json-file')
import writeYamlFile = require('write-yaml-file')
import {
  execPnpm,
  execPnpmSync,
  preparePackages,
  retryLoadJsonFile,
  spawn,
} from './utils'
import mkdirp = require('mkdirp-promise')
import normalizeNewline = require('normalize-newline')

const test = promisifyTape(tape)

test('recursive install/uninstall', async (t: tape.Test) => {
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

  await execPnpm('recursive', 'install')

  t.ok(projects['project-1'].requireModule('is-positive'))
  t.ok(projects['project-2'].requireModule('is-negative'))
  await projects['project-2'].has('is-negative')

  await execPnpm('recursive', 'install', 'noop')

  t.ok(projects['project-1'].requireModule('noop'))
  t.ok(projects['project-2'].requireModule('noop'))

  await execPnpm('recursive', 'uninstall', 'is-negative')

  await projects['project-2'].hasNot('is-negative')
})

test('recursive update', async (t: tape.Test) => {
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

  await execPnpm('recursive', 'install')

  await execPnpm('recursive', 'update', 'is-positive@2.0.0')

  t.equal(projects['project-1'].requireModule('is-positive/package.json').version, '2.0.0')
  projects['project-2'].hasNot('is-positive')
})

test('recursive installation with package-specific .npmrc', async t => {
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

  await fs.writeFile('project-1/.npmrc', 'shamefully-flatten = true', 'utf8')

  await execPnpm('recursive', 'install')

  t.ok(projects['project-1'].requireModule('is-positive'))
  t.ok(projects['project-2'].requireModule('is-negative'))

  const modulesYaml1 = await projects['project-1'].loadModules()
  t.ok(modulesYaml1 && modulesYaml1.shamefullyFlatten)

  const modulesYaml2 = await projects['project-2'].loadModules()
  t.notOk(modulesYaml2 && modulesYaml2.shamefullyFlatten)
})

test('recursive installation using server', async (t: tape.Test) => {
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

  const storeDir = path.resolve('store')
  const server = spawn(['server', 'start'], {storeDir})

  const serverJsonPath = path.resolve(storeDir, '2', 'server', 'server.json')
  const serverJson = await retryLoadJsonFile(serverJsonPath)
  t.ok(serverJson)
  t.ok(serverJson.connectionOptions)

  await execPnpm('recursive', 'install')

  t.ok(projects['project-1'].requireModule('is-positive'))
  t.ok(projects['project-2'].requireModule('is-negative'))

  await execPnpm('server', 'stop', '--store', storeDir)
})

test('recursive installation of packages with hooks', async t => {
  // This test hangs on Appveyor for some reason
  if (isCI && isWindows()) return
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

  process.chdir('project-1')
  const pnpmfile = `
    module.exports = { hooks: { readPackage } }
    function readPackage (pkg) {
      pkg.dependencies = pkg.dependencies || {}
      pkg.dependencies['dep-of-pkg-with-1-dep'] = '100.1.0'
      return pkg
    }
  `
  await fs.writeFile('pnpmfile.js', pnpmfile, 'utf8')

  process.chdir('../project-2')
  await fs.writeFile('pnpmfile.js', pnpmfile, 'utf8')

  process.chdir('..')

  await execPnpm('recursive', 'install')

  const shr1 = await projects['project-1'].loadShrinkwrap()
  t.ok(shr1.packages['/dep-of-pkg-with-1-dep/100.1.0'])

  const shr2 = await projects['project-2'].loadShrinkwrap()
  t.ok(shr2.packages['/dep-of-pkg-with-1-dep/100.1.0'])
})

test('ignores pnpmfile.js during recursive installation when --ignore-pnpmfile is used', async t => {
  // This test hangs on Appveyor for some reason
  if (isCI && isWindows()) return
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

  process.chdir('project-1')
  const pnpmfile = `
    module.exports = { hooks: { readPackage } }
    function readPackage (pkg) {
      pkg.dependencies = pkg.dependencies || {}
      pkg.dependencies['dep-of-pkg-with-1-dep'] = '100.1.0'
      return pkg
    }
  `
  await fs.writeFile('pnpmfile.js', pnpmfile, 'utf8')

  process.chdir('../project-2')
  await fs.writeFile('pnpmfile.js', pnpmfile, 'utf8')

  process.chdir('..')

  await execPnpm('recursive', 'install', '--ignore-pnpmfile')

  const shr1 = await projects['project-1'].loadShrinkwrap()
  t.notOk(shr1.packages['/dep-of-pkg-with-1-dep/100.1.0'])

  const shr2 = await projects['project-2'].loadShrinkwrap()
  t.notOk(shr2.packages['/dep-of-pkg-with-1-dep/100.1.0'])
})

test('recursive linking/unlinking', async (t: tape.Test) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',
      devDependencies: {
        'is-positive': '1.0.0',
      },
    },
    {
      name: 'is-positive',
      version: '1.0.0',
      dependencies: {
        'is-negative': '1.0.0',
      },
    },
  ])

  await execPnpm('recursive', 'link')

  t.ok(projects['is-positive'].requireModule('is-negative'))
  t.notOk(projects['project-1'].requireModule('is-positive/package.json').author, 'local package is linked')

  {
    const project1Shr = await projects['project-1'].loadShrinkwrap()
    t.equal(project1Shr.devDependencies['is-positive'], 'link:../is-positive')
  }

  await execPnpm('recursive', 'unlink')

  process.chdir('project-1')
  t.ok(await exists('node_modules', 'is-positive', 'index.js'), 'local package is unlinked')

  {
    const project1Shr = await projects['project-1'].loadShrinkwrap()
    t.equal(project1Shr.registry, 'http://localhost:4873/', 'project-1 has correct registry specified in shrinkwrap.yaml')
    t.equal(project1Shr.devDependencies['is-positive'], '1.0.0')
    t.ok(project1Shr.packages['/is-positive/1.0.0'])
  }

  const isPositiveShr = await projects['is-positive'].loadShrinkwrap()
  t.equal(isPositiveShr.registry, 'http://localhost:4873/', 'is-positive has correct registry specified in shrinkwrap.yaml')
})

test('running `pnpm recursive` on a subset of packages', async t => {
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

  await writeYamlFile('pnpm-workspace.yaml', {packages: ['project-1']})

  await execPnpm('recursive', 'install')

  await projects['project-1'].has('is-positive')
  await projects['project-2'].hasNot('is-negative')
})

test('running `pnpm recursive` only for packages in subdirectories of cwd', async t => {
  const projects = preparePackages(t, [
    {
      location: 'packages/project-1',
      package: {
        name: 'project-1',
        version: '1.0.0',
        dependencies: {
          'is-positive': '1.0.0',
        },
      },
    },
    {
      location: 'packages/project-2',
      package: {
        name: 'project-2',
        version: '1.0.0',
        dependencies: {
          'is-negative': '1.0.0',
        },
      }
    },
    {
      location: 'root-project',
      package: {
        name: 'root-project',
        version: '1.0.0',
        dependencies: {
          'debug': '*',
        },
      }
    }
  ])

  await mkdirp('node_modules')
  process.chdir('packages')

  await execPnpm('recursive', 'install')

  await projects['project-1'].has('is-positive')
  await projects['project-2'].has('is-negative')
  await projects['root-project'].hasNot('debug')
})

test('recursive installation fails when installation in one of the packages fails', async t => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',
      dependencies: {
        'this-pkg-does-not-exist': '100.100.100',
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

  try {
    await execPnpm('recursive', 'install')
    t.fail('The command should have failed')
  } catch (err) {
    t.ok(err, 'the command failed')
  }
})

test('second run of `recursive link` after package.json has been edited manually', async t => {
  const projects = preparePackages(t, [
    {
      name: 'is-negative',
      version: '1.0.0',
      dependencies: {
        'is-positive': '2.0.0',
      },
    },
    {
      name: 'is-positive',
      version: '1.0.0',
    },
  ])

  await execPnpm('recursive', 'link')

  await writeJsonFile('is-negative/package.json', {
    name: 'is-negative',
    version: '1.0.0',
    dependencies: {
      'is-positive': '1.0.0',
    },
  })

  await execPnpm('recursive', 'link')

  t.ok(projects['is-negative'].requireModule('is-positive/package.json'))
})

test('recursive list', async (t: tape.Test) => {
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
    {
      name: 'project-3',
      version: '1.0.0',
    },
  ])

  await execPnpm('recursive', 'install')

  const result = execPnpmSync('recursive', 'list')

  t.equal(result.status, 0)

  t.equal(result.stdout.toString(), stripIndent`
    project-1@1.0.0 ${path.resolve('project-1')}
    └── is-positive@1.0.0

    project-2@1.0.0 ${path.resolve('project-2')}
    └── is-negative@1.0.0
  ` + '\n\n')
})

test('recursive list --scope', async (t: tape.Test) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',
      dependencies: {
        'is-positive': '1.0.0',
        'project-2': '1.0.0',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',
      dependencies: {
        'is-negative': '1.0.0',
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',
      dependencies: {
        'is-negative': '1.0.0',
        'is-positive': '1.0.0',
      },
    },
  ])

  await execPnpm('recursive', 'link')

  const result = execPnpmSync('recursive', 'list', '--scope', 'project-1')

  t.equal(result.status, 0)

  t.equal(result.stdout.toString(), stripIndent`
    project-1@1.0.0 ${path.resolve('project-1')}
    ├── is-positive@1.0.0
    └── project-2@link:../project-2

    project-2@1.0.0 ${path.resolve('project-2')}
    └── is-negative@1.0.0
  ` + '\n\n')
})

test('pnpm recursive outdated', async (t: tape.Test) => {
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

  await execPnpm('recursive', 'install')

  {
    const result = execPnpmSync('recursive', 'outdated')

    t.equal(result.status, 0)

    t.equal(normalizeNewline(result.stdout.toString()), '           ' + stripIndents`
                Package      Current  Wanted  Latest
      project-1  is-positive  1.0.0    1.0.0   3.1.0
      project-2  is-negative  1.0.0    1.0.0   2.1.0
    ` + '\n')
  }

  {
    const result = execPnpmSync('recursive', 'outdated', 'is-positive')

    t.equal(result.status, 0)

    t.equal(normalizeNewline(result.stdout.toString()), '           ' + stripIndents`
                Package      Current  Wanted  Latest
      project-1  is-positive  1.0.0    1.0.0   3.1.0
    ` + '\n')
  }
})

test('pnpm recursive run', async (t: tape.Test) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',
      dependencies: {
        'json-append': '1',
      },
      scripts: {
        build: `node -e "process.stdout.write('project-1')" | json-append ../output.json`,
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',
      dependencies: {
        'json-append': '1',
        'project-1': '1'
      },
      scripts: {
        prebuild: `node -e "process.stdout.write('project-2-prebuild')" | json-append ../output.json`,
        build: `node -e "process.stdout.write('project-2')" | json-append ../output.json`,
        postbuild: `node -e "process.stdout.write('project-2-postbuild')" | json-append ../output.json`,
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',
      dependencies: {
        'json-append': '1',
        'project-1': '1'
      },
      scripts: {
        build: `node -e "process.stdout.write('project-3')" | json-append ../output.json`,
      },
    },
    {
      name: 'project-0',
      version: '1.0.0',
      dependencies: {
      },
    },
  ])

  await execPnpm('recursive', 'link')
  await execPnpm('recursive', 'run', 'build')

  const outputs = await import(path.resolve('output.json')) as string[]
  const p1 = outputs.indexOf('project-1')
  const p2 = outputs.indexOf('project-2')
  const p2pre = outputs.indexOf('project-2-prebuild')
  const p2post = outputs.indexOf('project-2-postbuild')
  const p3 = outputs.indexOf('project-3')

  t.ok(p1 < p2 && p1 < p3)
  t.ok(p1 < p2pre && p1 < p2post)
  t.ok(p2 < p2post && p2 > p2pre)
})

test('`pnpm recursive run` fails if none of the packaegs has the desired command', async (t: tape.Test) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',
      dependencies: {
        'json-append': '1',
      },
      scripts: {
        build: `node -e "process.stdout.write('project-1')" | json-append ../output.json`,
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',
      dependencies: {
        'json-append': '1',
        'project-1': '1'
      },
      scripts: {
        build: `node -e "process.stdout.write('project-2')" | json-append ../output.json`,
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',
      dependencies: {
        'json-append': '1',
        'project-1': '1'
      },
      scripts: {
        build: `node -e "process.stdout.write('project-3')" | json-append ../output.json`,
      },
    },
    {
      name: 'project-0',
      version: '1.0.0',
      dependencies: {
      },
    },
  ])

  await execPnpm('recursive', 'link')

  try {
    await execPnpm('recursive', 'run', 'this-command-does-not-exist')
    t.fail('should have failed')
  } catch (err) {
    t.pass('`recursive run` failed because none of the packages has the wanted script')
  }
})

test('pnpm recursive test', async (t: tape.Test) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',
      dependencies: {
        'json-append': '1',
      },
      scripts: {
        test: `node -e "process.stdout.write('project-1')" | json-append ../output.json`,
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',
      dependencies: {
        'json-append': '1',
        'project-1': '1'
      },
      scripts: {
        test: `node -e "process.stdout.write('project-2')" | json-append ../output.json`,
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',
      dependencies: {
        'json-append': '1',
        'project-1': '1'
      },
      scripts: {
        test: `node -e "process.stdout.write('project-3')" | json-append ../output.json`,
      },
    },
    {
      name: 'project-0',
      version: '1.0.0',
      dependencies: {
      },
    },
  ])

  await execPnpm('recursive', 'link')
  await execPnpm('recursive', 'test')

  const outputs = await import(path.resolve('output.json')) as string[]

  const p1 = outputs.indexOf('project-1')
  const p2 = outputs.indexOf('project-2')
  const p3 = outputs.indexOf('project-3')

  t.ok(p1 < p2 && p1 < p3)
})

test('`pnpm recursive test` does not fail if none of the packaegs has a test command', async (t: tape.Test) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',
    },
    {
      name: 'project-2',
      version: '1.0.0',
      dependencies: {
        'project-1': '1'
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',
      dependencies: {
        'project-1': '1'
      },
    },
    {
      name: 'project-0',
      version: '1.0.0',
      dependencies: {
      },
    },
  ])

  await execPnpm('recursive', 'link')

  await execPnpm('recursive', 'test')

  t.pass('command did not fail')
})

test('pnpm recursive rebuild', async (t: tape.Test) => {
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

  await execPnpm('recursive', 'install', '--ignore-scripts')

  await projects['project-1'].hasNot('pre-and-postinstall-scripts-example/generated-by-preinstall.js')
  await projects['project-1'].hasNot('pre-and-postinstall-scripts-example/generated-by-postinstall.js')
  await projects['project-2'].hasNot('pre-and-postinstall-scripts-example/generated-by-preinstall.js')
  await projects['project-2'].hasNot('pre-and-postinstall-scripts-example/generated-by-postinstall.js')

  await execPnpm('recursive', 'rebuild')

  await projects['project-1'].has('pre-and-postinstall-scripts-example/generated-by-preinstall.js')
  await projects['project-1'].has('pre-and-postinstall-scripts-example/generated-by-postinstall.js')
  await projects['project-2'].has('pre-and-postinstall-scripts-example/generated-by-preinstall.js')
  await projects['project-2'].has('pre-and-postinstall-scripts-example/generated-by-postinstall.js')
})

test('`pnpm recursive rebuild` specific dependencies', async (t: tape.Test) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',
      dependencies: {
        'pre-and-postinstall-scripts-example': '*',
        'install-scripts-example-for-pnpm': 'zkochan/install-scripts-example',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',
      dependencies: {
        'pre-and-postinstall-scripts-example': '*',
        'install-scripts-example-for-pnpm': 'zkochan/install-scripts-example',
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',
    },
  ])

  await execPnpm('recursive', 'install', '--ignore-scripts')

  await projects['project-1'].hasNot('pre-and-postinstall-scripts-example/generated-by-preinstall.js')
  await projects['project-1'].hasNot('pre-and-postinstall-scripts-example/generated-by-postinstall.js')
  await projects['project-2'].hasNot('pre-and-postinstall-scripts-example/generated-by-preinstall.js')
  await projects['project-2'].hasNot('pre-and-postinstall-scripts-example/generated-by-postinstall.js')

  await execPnpm('recursive', 'rebuild', 'install-scripts-example-for-pnpm')

  await projects['project-1'].hasNot('pre-and-postinstall-scripts-example/generated-by-preinstall.js')
  await projects['project-1'].hasNot('pre-and-postinstall-scripts-example/generated-by-postinstall.js')
  await projects['project-2'].hasNot('pre-and-postinstall-scripts-example/generated-by-preinstall.js')
  await projects['project-2'].hasNot('pre-and-postinstall-scripts-example/generated-by-postinstall.js')

  {
    const generatedByPreinstall = projects['project-1'].requireModule('install-scripts-example-for-pnpm/generated-by-preinstall')
    t.ok(typeof generatedByPreinstall === 'function', 'generatedByPreinstall() is available')

    const generatedByPostinstall = projects['project-1'].requireModule('install-scripts-example-for-pnpm/generated-by-postinstall')
    t.ok(typeof generatedByPostinstall === 'function', 'generatedByPostinstall() is available')
  }

  {
    const generatedByPreinstall = projects['project-2'].requireModule('install-scripts-example-for-pnpm/generated-by-preinstall')
    t.ok(typeof generatedByPreinstall === 'function', 'generatedByPreinstall() is available')

    const generatedByPostinstall = projects['project-2'].requireModule('install-scripts-example-for-pnpm/generated-by-postinstall')
    t.ok(typeof generatedByPostinstall === 'function', 'generatedByPostinstall() is available')
  }
})

test('recursive --scope', async (t: tape.Test) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',
      dependencies: {
        'is-positive': '1.0.0',
        'project-2': '1.0.0',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',
      dependencies: {
        'is-negative': '1.0.0',
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',
      dependencies: {
        minimatch: '*',
      },
    },
  ])

  await execPnpm('recursive', 'link', '--scope', 'project-1')

  projects['project-1'].has('is-positive')
  projects['project-2'].has('is-negative')
  projects['project-3'].hasNot('minimatch')
})

test('recursive --scope ignore excluded packages', async (t: tape.Test) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',
      dependencies: {
        'is-positive': '1.0.0',
        'project-2': '1.0.0',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',
      dependencies: {
        'is-negative': '1.0.0',
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',
      dependencies: {
        minimatch: '*',
      },
    },
  ])

  await writeYamlFile('pnpm-workspace.yaml', {
    packages: [
      '**',
      '!project-1'
    ],
  })

  await execPnpm('recursive', 'link', '--scope', 'project-1')

  projects['project-1'].hasNot('is-positive')
  projects['project-2'].hasNot('is-negative')
  projects['project-3'].hasNot('minimatch')
})

test('pnpm recursive exec', async (t: tape.Test) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',
      dependencies: {
        'json-append': '1',
      },
      scripts: {
        build: `node -e "process.stdout.write('project-1')" | json-append ../output.json`,
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',
      dependencies: {
        'json-append': '1',
        'project-1': '1'
      },
      scripts: {
        prebuild: `node -e "process.stdout.write('project-2-prebuild')" | json-append ../output.json`,
        build: `node -e "process.stdout.write('project-2')" | json-append ../output.json`,
        postbuild: `node -e "process.stdout.write('project-2-postbuild')" | json-append ../output.json`,
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',
      dependencies: {
        'json-append': '1',
        'project-1': '1'
      },
      scripts: {
        build: `node -e "process.stdout.write('project-3')" | json-append ../output.json`,
      },
    },
  ])

  await execPnpm('recursive', 'link')
  await execPnpm('recursive', 'exec', 'npm', 'run', 'build')

  const outputs = await import(path.resolve('output.json')) as string[]
  const p1 = outputs.indexOf('project-1')
  const p2 = outputs.indexOf('project-2')
  const p2pre = outputs.indexOf('project-2-prebuild')
  const p2post = outputs.indexOf('project-2-postbuild')
  const p3 = outputs.indexOf('project-3')

  t.ok(p1 < p2 && p1 < p3)
  t.ok(p1 < p2pre && p1 < p2post)
  t.ok(p2 < p2post && p2 > p2pre)
})
