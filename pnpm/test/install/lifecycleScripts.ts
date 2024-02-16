import fs from 'fs'
import path from 'path'
import { prepare } from '@pnpm/prepare'
import { type PackageManifest } from '@pnpm/types'
import rimraf from '@zkochan/rimraf'
import PATH from 'path-name'
import loadJsonFile from 'load-json-file'
import { execPnpmSync } from '../utils'

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
  const project = prepare({ name: 'foo', version: '1.0.0' })

  const result = execPnpmSync(['install', 'package-that-cannot-be-installed@0.0.0'])

  expect(result.status).toBe(1)

  expect(project.readCurrentLockfile()).toBeFalsy()
  expect(project.readLockfile()).toBeFalsy()

  const { default: pkg } = await import(path.resolve('package.json'))
  expect(pkg).toStrictEqual({ name: 'foo', version: '1.0.0' })
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
      onlyBuiltDependenciesFile: 'node_modules/@pnpm.e2e/build-allow-list/list.json',
    },
  })
  execPnpmSync(['add', '@pnpm.e2e/build-allow-list', '@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0', '@pnpm.e2e/install-script-example'])

  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')).toBeFalsy()
  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')).toBeFalsy()
  expect(fs.existsSync('node_modules/@pnpm.e2e/install-script-example/generated-by-install.js')).toBeTruthy()

  await rimraf('node_modules')

  execPnpmSync(['install', '--frozen-lockfile'])

  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')).toBeFalsy()
  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')).toBeFalsy()
  expect(fs.existsSync('node_modules/@pnpm.e2e/install-script-example/generated-by-install.js')).toBeTruthy()

  execPnpmSync(['rebuild'])

  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')).toBeFalsy()
  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')).toBeFalsy()
  expect(fs.existsSync('node_modules/@pnpm.e2e/install-script-example/generated-by-install.js')).toBeTruthy()
})
