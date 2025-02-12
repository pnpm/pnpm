import fs from 'fs'
import path from 'path'
import { prepare, preparePackages } from '@pnpm/prepare'
import { type PackageManifest, type ProjectManifest } from '@pnpm/types'
import { sync as rimraf } from '@zkochan/rimraf'
import PATH from 'path-name'
import loadJsonFile from 'load-json-file'
import writeYamlFile from 'write-yaml-file'
import { execPnpm, execPnpmSync, pnpmBinLocation } from '../utils'
import { getIntegrity } from '@pnpm/registry-mock'

const pkgRoot = path.join(__dirname, '..', '..')
const pnpmPkg = loadJsonFile.sync<PackageManifest>(path.join(pkgRoot, 'package.json'))

test('installation fails if lifecycle script fails', () => {
  prepare({
    scripts: {
      preinstall: 'exit 1',
    },
  })

  const result = execPnpmSync(['install'])

  expect(result.status).toBe(1)
})

test('lifecycle script runs with the correct user agent', () => {
  prepare({
    scripts: {
      preinstall: 'node --eval "console.log(process.env.npm_config_user_agent)"',
    },
  })

  const result = execPnpmSync(['install'])

  expect(result.status).toBe(0)
  const expectedUserAgentPrefix = `${pnpmPkg.name}/${pnpmPkg.version} `
  expect(result.stdout.toString()).toContain(expectedUserAgentPrefix)
})

test('preinstall is executed before general installation', () => {
  prepare({
    scripts: {
      preinstall: 'echo "Hello world!"',
    },
  })

  const result = execPnpmSync(['install'])

  expect(result.status).toBe(0)
  expect(result.stdout.toString()).toContain('Hello world!')
})

test('postinstall is executed after general installation', () => {
  prepare({
    scripts: {
      postinstall: 'echo "Hello world!"',
    },
  })

  const result = execPnpmSync(['install'])

  expect(result.status).toBe(0)
  expect(result.stdout.toString()).toContain('Hello world!')
})

test('postinstall is not executed after named installation', () => {
  prepare({
    scripts: {
      postinstall: 'echo "Hello world!"',
    },
  })

  const result = execPnpmSync(['install', 'is-negative'])

  expect(result.status).toBe(0)
  expect(result.stdout.toString()).not.toContain('Hello world!')
})

test('prepare is not executed after installation with arguments', () => {
  prepare({
    scripts: {
      prepare: 'echo "Hello world!"',
    },
  })

  const result = execPnpmSync(['install', 'is-negative'])

  expect(result.status).toBe(0)
  expect(result.stdout.toString()).not.toContain('Hello world!')
})

test('prepare is executed after argumentless installation', () => {
  prepare({
    scripts: {
      prepare: 'echo "Hello world!"',
    },
  })

  const result = execPnpmSync(['install'])

  expect(result.status).toBe(0)
  expect(result.stdout.toString()).toContain('Hello world!')
})

test('dependency should not be added to package.json and lockfile if it was not built successfully', async () => {
  const initialPkg = {
    name: 'foo',
    version: '1.0.0',
    pnpm: { neverBuiltDependencies: [] },
  }
  const project = prepare(initialPkg)

  const result = execPnpmSync(['install', 'package-that-cannot-be-installed@0.0.0'])

  expect(typeof result.status).toBe('number')
  expect(result.status).not.toBe(0)

  expect(project.readCurrentLockfile()).toBeFalsy()
  expect(project.readLockfile()).toBeFalsy()

  const { default: pkg } = await import(path.resolve('package.json'))
  expect(pkg).toStrictEqual(initialPkg)
})

test('node-gyp is in the PATH', async () => {
  prepare({
    scripts: {
      test: 'echo $PATH && node-gyp --help',
    },
  })

  const result = execPnpmSync(['test'], {
    env: {
      // `npm test` adds node-gyp to the PATH
      // it is removed here to test that pnpm adds it
      [PATH]: process.env[PATH]!
        .split(path.delimiter)
        .filter((p: string) => !p.includes('node-gyp-bin'))
        .join(path.delimiter),
    },
  })

  expect(result.status).toBe(0)
})

test('selectively allow scripts in some dependencies by onlyBuiltDependenciesFile', async () => {
  prepare({
    pnpm: {
      configDependencies: {
        '@pnpm.e2e/build-allow-list': `1.0.0+${getIntegrity('@pnpm.e2e/build-allow-list', '1.0.0')}`,
      },
      onlyBuiltDependenciesFile: 'node_modules/.pnpm-config/@pnpm.e2e/build-allow-list/list.json',
    },
  })
  execPnpmSync(['add', '@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0', '@pnpm.e2e/install-script-example'])

  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')).toBeFalsy()
  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')).toBeFalsy()
  expect(fs.existsSync('node_modules/@pnpm.e2e/install-script-example/generated-by-install.js')).toBeTruthy()

  rimraf('node_modules')

  execPnpmSync(['install', '--frozen-lockfile'])

  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')).toBeFalsy()
  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')).toBeFalsy()
  expect(fs.existsSync('node_modules/@pnpm.e2e/install-script-example/generated-by-install.js')).toBeTruthy()

  execPnpmSync(['rebuild'])

  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')).toBeFalsy()
  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')).toBeFalsy()
  expect(fs.existsSync('node_modules/@pnpm.e2e/install-script-example/generated-by-install.js')).toBeTruthy()
})

test('use node versions specified by pnpm.executionEnv.nodeVersion in workspace packages', async () => {
  const projects = preparePackages([
    {
      location: '.',
      package: {
        name: 'root',
        version: '1.0.0',
        private: true,
      },
    },
    {
      name: 'node-version-unset',
      version: '1.0.0',
      scripts: {
        test: 'node -v > node-version.txt',
      },
    },
    {
      name: 'node-version-18',
      version: '1.0.0',
      scripts: {
        test: 'node -v > node-version.txt',
      },
      pnpm: {
        executionEnv: {
          nodeVersion: '18.0.0',
        },
      },
    },
    {
      name: 'node-version-20',
      version: '1.0.0',
      scripts: {
        test: 'node -v > node-version.txt',
      },
      pnpm: {
        executionEnv: {
          nodeVersion: '20.0.0',
        },
      },
    },
  ])

  await writeYamlFile(path.resolve('pnpm-workspace.yaml'), {
    packages: ['*'],
  })

  execPnpmSync(['-r', 'test'])
  expect(
    ['node-version-unset', 'node-version-18', 'node-version-20'].map(name => {
      const filePath = path.join(projects[name].dir(), 'node-version.txt')
      return fs.readFileSync(filePath, 'utf-8').trim()
    })
  ).toStrictEqual([process.version, 'v18.0.0', 'v20.0.0'])

  execPnpmSync(['--config.use-node-version=19.0.0', '-r', 'test'])
  expect(
    ['node-version-unset', 'node-version-18', 'node-version-20'].map(name => {
      const filePath = path.join(projects[name].dir(), 'node-version.txt')
      return fs.readFileSync(filePath, 'utf-8').trim()
    })
  ).toStrictEqual(['v19.0.0', 'v18.0.0', 'v20.0.0'])
})

test('ignores pnpm.executionEnv specified by dependencies', async () => {
  prepare({
    name: 'ignores-pnpm-use-node-version-from-dependencies',
    version: '1.0.0',
    dependencies: {
      // this package's package.json has pnpm.executionEnv.nodeVersion = '20.0.0'
      '@pnpm.e2e/has-execution-env': '1.0.0',
    },
    pnpm: {
      neverBuiltDependencies: [],
    },
  })

  await execPnpm(['install'])

  const nodeInfoFile = path.resolve('node_modules', '@pnpm.e2e', 'has-execution-env', 'node-info.json')
  const nodeInfoJson = fs.readFileSync(nodeInfoFile, 'utf-8')
  const nodeInfo = JSON.parse(nodeInfoJson)

  // pnpm should still use system's Node.js to execute the install script despite pnpm.executionEnv.nodeVersion specified by the dependency
  expect(nodeInfo).toMatchObject({
    execPath: process.execPath,
    versions: process.versions,
  })
})

test('preinstall script does not trigger verify-deps-before-run (#8954)', async () => {
  const pnpm = `${process.execPath} ${pnpmBinLocation}` // this would fail if either paths happen to contain spaces

  prepare({
    name: 'preinstall-script-does-not-trigger-verify-deps-before-run',
    version: '1.0.0',
    private: true,
    scripts: {
      sayHello: 'echo hello world',
      preinstall: `${pnpm} run sayHello`,
    },
    dependencies: {
      cowsay: '1.5.0', // to make the default state outdated, any dependency will do
    },
  })

  const output = execPnpmSync(['--config.verify-deps-before-run=error', 'install'], { expectSuccess: true })
  expect(output.status).toBe(0)
  expect(output.stdout.toString()).toContain('hello world')
})

test('throw an error when strict-dep-builds is true and there are ignored scripts', async () => {
  const project = prepare({})
  const result = execPnpmSync(['add', '@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0', '--config.strict-dep-builds=true'])

  expect(result.status).toBe(1)
  expect(result.stdout.toString()).toContain('Ignored build scripts:')

  project.has('@pnpm.e2e/pre-and-postinstall-scripts-example')

  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')).toBeFalsy()
  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')).toBeFalsy()
  expect(fs.existsSync('pnpm-lock.yaml')).toBeTruthy()

  const manifest = loadJsonFile.sync<ProjectManifest>('package.json')
  expect(manifest.dependencies).toStrictEqual({
    '@pnpm.e2e/pre-and-postinstall-scripts-example': '1.0.0',
  })
})

test('the list of ignored builds is preserved after a repeat install', async () => {
  const project = prepare({})
  execPnpmSync(['add', '@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0', '--config.optimistic-repeat-install=false'])

  const result = execPnpmSync(['install'])
  // The warning is printed on repeat install too
  expect(result.stdout.toString()).toContain('Ignored build scripts:')

  const modulesManifest = project.readModulesManifest()
  expect(modulesManifest?.ignoredBuilds).toStrictEqual(['@pnpm.e2e/pre-and-postinstall-scripts-example'])
})
