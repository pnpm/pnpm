import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { prepare } from '@pnpm/prepare'
import type { PackageManifest, ProjectManifest } from '@pnpm/types'
import { readWorkspaceManifest } from '@pnpm/workspace.workspace-manifest-reader'
import { loadJsonFileSync } from 'load-json-file'
import PATH from 'path-name'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpmSync, pnpmBinLocation } from '../utils/index.js'

const pkgRoot = path.join(import.meta.dirname, '..', '..')
const pnpmPkg = loadJsonFileSync<PackageManifest>(path.join(pkgRoot, 'package.json'))

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
  }
  const project = prepare(initialPkg)
  writeYamlFileSync('pnpm-workspace.yaml', { allowBuilds: { 'package-that-cannot-be-installed': true } })

  const result = execPnpmSync(['install', 'package-that-cannot-be-installed@0.0.0'])

  expect(typeof result.status).toBe('number')
  expect(result.status).not.toBe(0)

  expect(project.readCurrentLockfile()).toBeFalsy()
  expect(project.readLockfile()).toBeFalsy()

  const { default: pkg } = await import(path.resolve('package.json'))
  expect(pkg).toEqual(initialPkg)
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

test('selectively allow scripts in some dependencies by --allow-build flag', async () => {
  const project = prepare({})
  execPnpmSync(['add', '--allow-build=@pnpm.e2e/install-script-example', '@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0', '@pnpm.e2e/install-script-example'])

  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')).toBeFalsy()
  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')).toBeFalsy()
  expect(fs.existsSync('node_modules/@pnpm.e2e/install-script-example/generated-by-install.js')).toBeTruthy()

  const modulesManifest = await readWorkspaceManifest(project.dir())
  expect(modulesManifest?.allowBuilds).toStrictEqual({
    '@pnpm.e2e/install-script-example': true,
    '@pnpm.e2e/pre-and-postinstall-scripts-example': 'set this to true or false',
  })
})

test('--allow-build flag should specify the package', async () => {
  const project = prepare({})
  const result = execPnpmSync(['add', '@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0', '--allow-build'])

  expect(result.status).toBe(1)
  expect(result.stdout.toString()).toContain('The --allow-build flag is missing a package name. Please specify the package name(s) that are allowed to run installation scripts.')

  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')).toBeFalsy()
  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')).toBeFalsy()
  expect(fs.existsSync('node_modules/@pnpm.e2e/install-script-example/generated-by-install.js')).toBeFalsy()

  const modulesManifest = await readWorkspaceManifest(project.dir())
  expect(modulesManifest?.allowBuilds).toBeUndefined()
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

test('preinstall and postinstall scripts do not trigger verify-deps-before-run when using settings from a config file (#10060)', async () => {
  const pnpm = `${process.execPath} ${pnpmBinLocation}` // this would fail if either paths happen to contain spaces

  prepare({
    name: 'preinstall-script-does-not-trigger-verify-deps-before-run-config-file',
    version: '1.0.0',
    private: true,
    scripts: {
      sayHello: 'echo hello world',
      preinstall: `${pnpm} run sayHello`,
      postinstall: `${pnpm} run sayHello`,
    },
    dependencies: {
      cowsay: '1.5.0', // to make the default state outdated, any dependency will do
    },
  })

  writeYamlFileSync('pnpm-workspace.yaml', { verifyDepsBeforeRun: 'install' })

  // 20s timeout because if it fails it will run for 3 minutes instead
  const output = execPnpmSync(['install'], { expectSuccess: true, timeout: 20_000 })

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

  const manifest = loadJsonFileSync<ProjectManifest>('package.json')
  expect(manifest.dependencies).toStrictEqual({
    '@pnpm.e2e/pre-and-postinstall-scripts-example': '1.0.0',
  })
})

test('the list of ignored builds is preserved after a repeat install', async () => {
  const project = prepare({})
  execPnpmSync(['add', '@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0', 'esbuild@0.25.0', '--config.optimistic-repeat-install=false'])

  const result = execPnpmSync(['install', '--config.optimistic-repeat-install=false'])
  // The warning is printed on repeat install too
  expect(result.stdout.toString()).toContain('Ignored build scripts:')

  const modulesManifest = project.readModulesManifest()
  expect(Array.from(modulesManifest!.ignoredBuilds!).sort()).toStrictEqual([
    '@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0',
    'esbuild@0.25.0',
  ])
})

test('ignored builds are auto-populated as placeholders in allowBuilds', async () => {
  prepare({})
  execPnpmSync(['add', '@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0'])

  const manifest = await readWorkspaceManifest(process.cwd())
  expect(manifest?.allowBuilds?.['@pnpm.e2e/pre-and-postinstall-scripts-example']).toBe('set this to true or false')
})

test('auto-populated placeholders are merged with existing allowBuilds', async () => {
  prepare({})
  writeYamlFileSync('pnpm-workspace.yaml', {
    allowBuilds: {
      '@pnpm.e2e/install-script-example': true,
    },
  })
  execPnpmSync(['add', '@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0'])

  const manifest = await readWorkspaceManifest(process.cwd())
  expect(manifest?.allowBuilds?.['@pnpm.e2e/install-script-example']).toBe(true)
  expect(manifest?.allowBuilds?.['@pnpm.e2e/pre-and-postinstall-scripts-example']).toBe('set this to true or false')
})

test('selective rebuild preserves ignoredBuilds for packages not being rebuilt', async () => {
  const project = prepare({})
  writeYamlFileSync('pnpm-workspace.yaml', {
    allowBuilds: {
      '@pnpm.e2e/pre-and-postinstall-scripts-example': true,
    },
  })
  execPnpmSync(['add', '@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0', '@pnpm.e2e/install-script-example'])

  // install-script-example should be in ignoredBuilds
  const beforeRebuild = project.readModulesManifest()
  expect(beforeRebuild!.ignoredBuilds).toBeDefined()

  // Selectively rebuild only the approved package
  execPnpmSync(['rebuild', '@pnpm.e2e/pre-and-postinstall-scripts-example'])

  // install-script-example should still be in ignoredBuilds after selective rebuild
  const afterRebuild = project.readModulesManifest()
  expect(afterRebuild!.ignoredBuilds).toBeDefined()
})

test('strictDepBuilds fails for packages with cached side-effects (#11035)', async () => {
  prepare({
    dependencies: {
      '@pnpm.e2e/pre-and-postinstall-scripts-example': '1.0.0',
    },
  })
  const storeDir = path.resolve('isolated-store')

  // First install: allow the build so side-effects get cached in the store
  writeYamlFileSync('pnpm-workspace.yaml', {
    storeDir,
    enableGlobalVirtualStore: false,
    allowBuilds: {
      '@pnpm.e2e/pre-and-postinstall-scripts-example': true,
    },
  })
  const firstResult = execPnpmSync(['install'])
  expect(firstResult.status).toBe(0)
  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')).toBeTruthy()

  // Second install: remove the approval. Side-effects are cached in the store
  // but strictDepBuilds should still fail.
  writeYamlFileSync('pnpm-workspace.yaml', {
    storeDir,
    enableGlobalVirtualStore: false,
    strictDepBuilds: true,
    optimisticRepeatInstall: false,
  })
  const secondResult = execPnpmSync(['install'])
  expect(secondResult.status).toBe(1)
  expect(secondResult.stdout.toString()).toContain('Ignored build scripts:')
})

test('git dependencies with preparation scripts should be installed when dangerouslyAllowAllBuilds is true', async () => {
  prepare({})
  writeYamlFileSync('pnpm-workspace.yaml', { dangerouslyAllowAllBuilds: true })

  // 'test-git-fetch' has a prepare script that builds the package.
  const result = execPnpmSync(['add', 'https://github.com/pnpm/test-git-fetch.git#8b333f12d5357f4f25a654c305c826294cb073bf'])

  expect(result.status).toBe(0)
  expect(fs.existsSync('node_modules/test-git-fetch/dist/index.js')).toBeTruthy()
})

test('--allow-build flag should error when conflicting with allowBuilds: false', async () => {
  prepare({})
  writeYamlFileSync('pnpm-workspace.yaml', {
    allowBuilds: { '@pnpm.e2e/install-script-example': false },
  })
  const result = execPnpmSync(['add', '--allow-build=@pnpm.e2e/install-script-example', '@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0', '@pnpm.e2e/install-script-example'])

  expect(result.status).toBe(1)
  expect(result.stdout.toString()).toContain('The following dependencies are ignored by the root project, but are allowed to be built by the current command: @pnpm.e2e/install-script-example')
})
