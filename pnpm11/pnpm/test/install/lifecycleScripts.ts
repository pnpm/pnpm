import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { parse } from '@pnpm/deps.path'
import { prepare, preparePackages } from '@pnpm/prepare'
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

test('lifecycle script runs with the correct user agent during headless install', () => {
  prepare({
    dependencies: {
      'is-positive': '1.0.0',
    },
    scripts: {
      preinstall: 'node --eval "console.log(process.env.npm_config_user_agent)"',
    },
  })
  writeYamlFileSync('pnpm-workspace.yaml', {
    optimisticRepeatInstall: true,
  })

  execPnpmSync(['install', '--lockfile-only'], { expectSuccess: true })
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

test('--allow-build flag keeps the packages already listed in allowBuilds', async () => {
  const project = prepare({})
  writeYamlFileSync('pnpm-workspace.yaml', {
    allowBuilds: {
      '@pnpm.e2e/install-script-example': true,
      'some-string-package': 'reason',
    },
  })
  execPnpmSync(['add', '--allow-build=@pnpm.e2e/pre-and-postinstall-scripts-example', '@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0'], { expectSuccess: true })

  const workspaceManifest = await readWorkspaceManifest(project.dir())
  expect(workspaceManifest?.allowBuilds).toStrictEqual({
    '@pnpm.e2e/install-script-example': true,
    'some-string-package': 'reason',
    '@pnpm.e2e/pre-and-postinstall-scripts-example': true,
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

test('allowBuilds false resolves a strict ignored-build failure on repeat install', async () => {
  const project = prepare({})
  writeYamlFileSync('pnpm-workspace.yaml', {
    optimisticRepeatInstall: true,
    strictDepBuilds: true,
  })

  const firstResult = execPnpmSync(['add', '@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0'])

  expect(firstResult.status).toBe(1)
  expect(firstResult.stdout.toString()).toContain('Ignored build scripts:')
  expect(fs.existsSync('node_modules/.modules.yaml')).toBeTruthy()

  writeYamlFileSync('pnpm-workspace.yaml', {
    allowBuilds: {
      '@pnpm.e2e/pre-and-postinstall-scripts-example': false,
    },
    optimisticRepeatInstall: true,
    strictDepBuilds: true,
  })

  const secondResult = execPnpmSync(['install'])

  expect(secondResult.status).toBe(0)
  expect(secondResult.stdout.toString()).not.toContain('Ignored build scripts:')
  const modulesManifest = project.readModulesManifest()
  expect(modulesManifest?.allowBuilds).toStrictEqual({
    '@pnpm.e2e/pre-and-postinstall-scripts-example': false,
  })
  expect(Array.from(modulesManifest?.ignoredBuilds ?? [])).toStrictEqual([])
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

test('install --ignore-workspace does not overwrite allowBuilds in pnpm-workspace.yaml', () => {
  prepare({
    dependencies: {
      '@pnpm.e2e/pre-and-postinstall-scripts-example': '1.0.0',
    },
  })
  writeYamlFileSync('pnpm-workspace.yaml', {
    allowBuilds: {
      '@pnpm.e2e/pre-and-postinstall-scripts-example': false,
    },
  })
  const manifestBefore = fs.readFileSync('pnpm-workspace.yaml', 'utf8')

  const { status, stdout, stderr } = execPnpmSync(['install', '--ignore-workspace'])

  // The build is ignored (--ignore-workspace skips the allowBuilds entry), so the
  // install ends in ERR_PNPM_IGNORED_BUILDS — the same code path that would have
  // written the placeholder. The manifest must stay untouched regardless.
  expect(status).toBe(1)
  expect(`${stdout}${stderr}`).toContain('ERR_PNPM_IGNORED_BUILDS')
  expect(fs.readFileSync('pnpm-workspace.yaml', 'utf8')).toBe(manifestBefore)
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

test('approve-builds works after stashing and re-adding a dependency (#12221)', async () => {
  const project = prepare({})

  const pkgName = '@pnpm.e2e/pre-and-postinstall-scripts-example'

  const firstAdd = execPnpmSync(['add', `${pkgName}@1.0.0`])
  expect(firstAdd.status).toBe(1)
  expect(firstAdd.stdout.toString()).toContain('Ignored build scripts:')

  const firstApprove = execPnpmSync(['approve-builds', '--all'])
  expect(firstApprove.status).toBe(0)

  const wsManifest = await readWorkspaceManifest(process.cwd())
  expect(wsManifest!.allowBuilds?.[pkgName]).toBe(true)

  fs.rmSync('package.json', { force: true })
  fs.rmSync('pnpm-workspace.yaml', { force: true })
  fs.rmSync('pnpm-lock.yaml', { force: true })

  fs.writeFileSync('package.json', '{}')
  writeYamlFileSync('pnpm-workspace.yaml', {})

  const secondAdd = execPnpmSync(['add', `${pkgName}@1.0.0`])
  expect(secondAdd.status).toBe(1)
  expect(secondAdd.stdout.toString()).toContain('Ignored build scripts:')

  const modulesManifest = project.readModulesManifest()
  expect(modulesManifest?.ignoredBuilds).toBeDefined()
  expect(Array.from(modulesManifest!.ignoredBuilds!).some((dp) => parse(dp).name === pkgName)).toBe(true)

  const secondApprove = execPnpmSync(['approve-builds', '--all'])
  expect(secondApprove.status).toBe(0)
  expect(secondApprove.stdout.toString()).not.toContain('No packages awaiting approval')
})

test('approve-builds works after removing an unrelated dependency (#13891)', async () => {
  const project = prepare({})

  const pendingPkg = '@pnpm.e2e/pre-and-postinstall-scripts-example'
  const removedPkg = '@pnpm.e2e/install-script-example'

  const firstAdd = execPnpmSync(['add', `${pendingPkg}@1.0.0`])
  expect(firstAdd.status).toBe(1)
  expect(firstAdd.stdout.toString()).toContain('Ignored build scripts:')

  const secondAdd = execPnpmSync(['add', `${removedPkg}@1.0.0`])
  expect(secondAdd.status).toBe(1)
  expect(secondAdd.stdout.toString()).toContain('Ignored build scripts:')

  const remove = execPnpmSync(['remove', removedPkg])
  expect(remove.status).toBe(0)

  const modulesManifest = project.readModulesManifest()
  const ignoredNames = Array.from(modulesManifest?.ignoredBuilds ?? []).map((depPath) => parse(depPath).name)
  expect(ignoredNames).toContain(pendingPkg)
  expect(ignoredNames).not.toContain(removedPkg)
  expect(fs.existsSync(`node_modules/${pendingPkg}/generated-by-preinstall.js`)).toBeFalsy()
  expect(fs.existsSync(`node_modules/${pendingPkg}/generated-by-postinstall.js`)).toBeFalsy()
  expect(fs.existsSync(`node_modules/${removedPkg}/generated-by-install.js`)).toBeFalsy()

  const approve = execPnpmSync(['approve-builds', '--all'])
  expect(approve.status).toBe(0)
  expect(approve.stdout.toString()).not.toContain('There are no packages awaiting approval')
  expect(fs.existsSync(`node_modules/${pendingPkg}/generated-by-preinstall.js`)).toBeTruthy()
  expect(fs.existsSync(`node_modules/${pendingPkg}/generated-by-postinstall.js`)).toBeTruthy()
  expect(fs.existsSync(`node_modules/${removedPkg}/generated-by-install.js`)).toBeFalsy()

  const wsManifest = await readWorkspaceManifest(process.cwd())
  expect(wsManifest!.allowBuilds?.[pendingPkg]).toBe(true)
})

// Which projects run their own lifecycle scripts is decided by the
// mutated-importer list the command layer builds: the projects the
// command was pointed at, plus the workspace root, which the recursive
// dispatch pushes in as a plain `mutation: 'install'` whenever the
// selection leaves it out. A project runs its scripts when that list
// covers only part of the workspace, or — when it covers all of it —
// when its own mutation is a full install.

const DEP = '@pnpm.e2e/dep-of-pkg-with-1-dep' // published at 100.0.0, 100.1.0 and 101.0.0

test('postinstall is not executed after a targeted update', () => {
  prepare({
    dependencies: { [DEP]: '^100.0.0' },
    scripts: {
      postinstall: 'echo "Hello world!"',
    },
  })
  execPnpmSync(['install'], { expectSuccess: true })

  const result = execPnpmSync(['update', DEP])

  expect(result.status).toBe(0)
  expect(result.stdout.toString()).not.toContain('Hello world!')
})

test('postinstall is executed after an argumentless update', () => {
  prepare({
    dependencies: { [DEP]: '^100.0.0' },
    scripts: {
      postinstall: 'echo "Hello world!"',
    },
  })
  execPnpmSync(['install'], { expectSuccess: true })

  const result = execPnpmSync(['update'])

  expect(result.status).toBe(0)
  expect(result.stdout.toString()).toContain('Hello world!')
})

test('postinstall is not executed after update --latest, which rewrites every direct dependency spec', () => {
  prepare({
    dependencies: { [DEP]: '^100.0.0' },
    scripts: {
      postinstall: 'echo "Hello world!"',
    },
  })
  execPnpmSync(['install'], { expectSuccess: true })

  const result = execPnpmSync(['update', '--latest'])

  expect(result.status).toBe(0)
  expect(result.stdout.toString()).not.toContain('Hello world!')
})

test('a targeted update in the only workspace member runs the postinstall of the workspace root alone', () => {
  prepareInstalledWorkspace(['a'])

  execPnpmSync(['update', DEP], { cwd: path.resolve('packages/a'), expectSuccess: true })

  expect(projectsThatRanPostinstall(['a'])).toStrictEqual(['root'])
})

test('a targeted update in a larger workspace runs the postinstall of every mutated project', () => {
  prepareInstalledWorkspace(['a', 'b'])

  execPnpmSync(['update', DEP], { cwd: path.resolve('packages/a'), expectSuccess: true })

  expect(projectsThatRanPostinstall(['a', 'b'])).toStrictEqual(['root', 'a'])
})

test('an argumentless update in a workspace member runs the postinstall of that member and the root', () => {
  prepareInstalledWorkspace(['a', 'b'])

  execPnpmSync(['update'], { cwd: path.resolve('packages/a'), expectSuccess: true })

  expect(projectsThatRanPostinstall(['a', 'b'])).toStrictEqual(['root', 'a'])
})

test('an add in a workspace member runs the postinstall of that member and the root', () => {
  prepareInstalledWorkspace(['a', 'b'])

  execPnpmSync(['add', '@pnpm.e2e/foo'], { cwd: path.resolve('packages/a'), expectSuccess: true })

  expect(projectsThatRanPostinstall(['a', 'b'])).toStrictEqual(['root', 'a'])
})

test('a remove in a workspace member runs no project postinstall', () => {
  prepareInstalledWorkspace(['a', 'b'])

  execPnpmSync(['remove', DEP], { cwd: path.resolve('packages/a'), expectSuccess: true })

  expect(projectsThatRanPostinstall(['a', 'b'])).toStrictEqual([])
})

test('a remove that falls back to resolution runs no project postinstall', () => {
  prepareInstalledWorkspace(['a', 'b'])
  // A pnpmfile added after the install changes the recorded
  // pnpmfileChecksum, which keeps the remove off the fast lockfile update,
  // so the removal takes the resolve-then-materialize path.
  fs.writeFileSync('.pnpmfile.cjs', 'module.exports = { hooks: { readPackage: (pkg) => pkg } }')

  execPnpmSync(['remove', DEP], { cwd: path.resolve('packages/a'), expectSuccess: true })

  expect(projectsThatRanPostinstall(['a', 'b'])).toStrictEqual([])
})

test('an argumentless update at the workspace root runs the postinstall of the root alone', () => {
  prepareInstalledWorkspace(['a', 'b'])

  execPnpmSync(['update'], { expectSuccess: true })

  expect(projectsThatRanPostinstall(['a', 'b'])).toStrictEqual(['root'])
})

test('a recursive targeted update runs no project postinstall', () => {
  prepareInstalledWorkspace(['a', 'b'])

  execPnpmSync(['-r', 'update', DEP], { expectSuccess: true })

  expect(projectsThatRanPostinstall(['a', 'b'])).toStrictEqual([])
})

test('a recursive argumentless update runs the postinstall of every project', () => {
  prepareInstalledWorkspace(['a', 'b'])

  execPnpmSync(['-r', 'update'], { expectSuccess: true })

  expect(projectsThatRanPostinstall(['a', 'b'])).toStrictEqual(['root', 'a', 'b'])
})

/** A workspace whose root and `packages/*` members all stamp a file from `postinstall`, installed once with the stamps then cleared. */
function prepareInstalledWorkspace (members: string[]): void {
  preparePackages(members.map((name) => ({
    location: `packages/${name}`,
    package: projectManifest(name),
  })))
  fs.writeFileSync('package.json', JSON.stringify(projectManifest('root')))
  writeYamlFileSync('pnpm-workspace.yaml', { packages: ['packages/*'] })

  execPnpmSync(['install'], { expectSuccess: true })
  clearPostinstallStamps(members)
}

function projectsThatRanPostinstall (members: string[]): string[] {
  return ['root', ...members].filter((project) => {
    const dir = project === 'root' ? '.' : path.join('packages', project)
    return fs.existsSync(path.join(dir, 'ran-postinstall.txt'))
  })
}

function projectManifest (name: string): ProjectManifest {
  return {
    name,
    version: '1.0.0',
    dependencies: { [DEP]: '^100.0.0' },
    scripts: {
      postinstall: 'node -e "require(\'fs\').writeFileSync(\'ran-postinstall.txt\',\'\')"',
    },
  }
}

function clearPostinstallStamps (members: string[]): void {
  for (const dir of ['.', ...members.map((name) => path.join('packages', name))]) {
    fs.rmSync(path.join(dir, 'ran-postinstall.txt'), { force: true })
  }
}
