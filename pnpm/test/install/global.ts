import path from 'path'
import PATH_NAME from 'path-name'
import fs from 'fs'
import { prepare } from '@pnpm/prepare'
import { type ProjectManifest } from '@pnpm/types'
import isWindows from 'is-windows'
import { GLOBAL_LAYOUT_VERSION } from '@pnpm/constants'
import {
  addDistTag,
  execPnpm,
  execPnpmSync,
} from '../utils/index.js'

function globalPkgDir (pnpmHome: string): string {
  return path.join(pnpmHome, 'global', GLOBAL_LAYOUT_VERSION)
}

/**
 * Find an installed global package in the flat isolated directory structure.
 * Scans globalDir for hash symlinks, resolves them,
 * and returns the path to the package's node_modules entry.
 */
function findGlobalPkg (globalDir: string, pkgName: string): string | null {
  let entries: fs.Dirent[]
  try {
    entries = fs.readdirSync(globalDir, { withFileTypes: true })
  } catch {
    return null
  }
  for (const entry of entries) {
    if (!entry.isSymbolicLink()) continue
    let installDir: string
    try {
      installDir = fs.realpathSync(path.join(globalDir, entry.name))
    } catch {
      continue
    }
    let pkgJson: { dependencies?: Record<string, string> }
    try {
      pkgJson = JSON.parse(fs.readFileSync(path.join(installDir, 'package.json'), 'utf-8'))
    } catch {
      continue
    }
    if (pkgJson.dependencies?.[pkgName]) {
      return path.join(installDir, 'node_modules', pkgName)
    }
  }
  return null
}

test('global installation', async () => {
  prepare()
  const global = path.resolve('..', 'global')
  const pnpmHome = path.join(global, 'pnpm')
  fs.mkdirSync(global)

  const env = { [PATH_NAME]: pnpmHome, PNPM_HOME: pnpmHome, XDG_DATA_HOME: global }

  await execPnpm(['add', '--global', 'is-positive'], { env })

  // there was an issue when subsequent installations were removing everything installed prior
  // https://github.com/pnpm/pnpm/issues/808
  await execPnpm(['add', '--global', 'is-negative'], { env })

  const isPositivePath = findGlobalPkg(globalPkgDir(pnpmHome), 'is-positive')
  expect(isPositivePath).toBeTruthy()
  const { default: isPositive } = await import(isPositivePath!)
  expect(typeof isPositive).toBe('function')

  const isNegativePath = findGlobalPkg(globalPkgDir(pnpmHome), 'is-negative')
  expect(isNegativePath).toBeTruthy()
  const { default: isNegative } = await import(isNegativePath!)
  expect(typeof isNegative).toBe('function')
})

test('global install warns when project has packageManager configured', async () => {
  prepare({
    name: 'project',
    version: '1.0.0',
    packageManager: 'yarn@4.0.0',
  })

  const global = path.resolve('..', 'global')
  const pnpmHome = path.join(global, 'pnpm')
  fs.mkdirSync(global)

  const env = { [PATH_NAME]: pnpmHome, PNPM_HOME: pnpmHome, XDG_DATA_HOME: global }

  const { status } = execPnpmSync([
    'add',
    '--global',
    'is-positive',
    '--config.package-manager-strict=true',
  ], { env })

  expect(status).toBe(0)
})

test('global installation to custom directory with --global-dir', async () => {
  prepare()
  const global = path.resolve('..', 'global')
  const pnpmHome = path.join(global, 'pnpm')
  const env = { [PATH_NAME]: pnpmHome, PNPM_HOME: pnpmHome }

  await execPnpm(['add', '--global', '--global-dir=../global', 'is-positive'], { env })

  const isPositivePath = findGlobalPkg(path.join(global, 'v11'), 'is-positive')
  expect(isPositivePath).toBeTruthy()
  const { default: isPositive } = await import(isPositivePath!)
  expect(typeof isPositive).toBe('function')
})

test('always install latest when doing global installation without spec', async () => {
  prepare()
  await addDistTag('@pnpm.e2e/peer-c', '2.0.0', 'latest')

  const global = path.resolve('..', 'global')
  const pnpmHome = path.join(global, 'pnpm')
  fs.mkdirSync(global)

  const env = { [PATH_NAME]: pnpmHome, PNPM_HOME: pnpmHome, XDG_DATA_HOME: global }

  await execPnpm(['add', '-g', '@pnpm.e2e/peer-c@1'], { env })
  await execPnpm(['add', '-g', '@pnpm.e2e/peer-c'], { env })

  const peerCPath = findGlobalPkg(globalPkgDir(pnpmHome), '@pnpm.e2e/peer-c')
  expect(peerCPath).toBeTruthy()
  expect((await import(path.join(peerCPath!, 'package.json'))).default.version).toBe('2.0.0')
})

test('run lifecycle events of global packages in correct working directory', async () => {
  if (isWindows()) {
    // Skipping this test on Windows because "$npm_execpath run create-file" will fail on Windows
    return
  }

  prepare()
  const global = path.resolve('..', 'global')
  const pnpmHome = path.join(global, 'pnpm')
  fs.mkdirSync(pnpmHome, { recursive: true })

  const env = {
    [PATH_NAME]: `${pnpmHome}${path.delimiter}${process.env[PATH_NAME]!}`,
    PNPM_HOME: pnpmHome,
    XDG_DATA_HOME: global,
  }

  await execPnpm(['add', '-g', '--allow-build=@pnpm.e2e/postinstall-calls-pnpm', '@pnpm.e2e/postinstall-calls-pnpm@1.0.0'], { env })

  const pkgPath = findGlobalPkg(globalPkgDir(pnpmHome), '@pnpm.e2e/postinstall-calls-pnpm')
  expect(pkgPath).toBeTruthy()
  expect(fs.existsSync(path.join(pkgPath!, 'created-by-postinstall'))).toBeTruthy()
})

// CONTEXT: dangerously-allow-all-builds has been removed from rc files, as a result, this test no longer applies
// TODO: Maybe we should create a yaml config file specifically for `--global`? After all, this test is to serve such use-cases
test.skip('dangerously-allow-all-builds=true in global config', async () => {
  // the directory structure below applies only to Linux
  if (process.platform !== 'linux') return

  const manifest: ProjectManifest = {
    name: 'local',
    version: '0.0.0',
    private: true,
    pnpm: {
      allowBuilds: {}, // don't allow any dependencies to be built
    },
  }

  const project = prepare(manifest)

  const home = path.resolve('..', 'home/username')
  const cfgHome = path.resolve(home, '.config')
  const pnpmCfgDir = path.resolve(cfgHome, 'pnpm')
  const pnpmRcFile = path.join(pnpmCfgDir, 'rc')
  const global = path.resolve('..', 'global')
  const pnpmHome = path.join(global, 'pnpm')
  const globalDir = globalPkgDir(pnpmHome)
  fs.mkdirSync(pnpmCfgDir, { recursive: true })
  fs.writeFileSync(pnpmRcFile, [
    'reporter=append-only',
    'dangerously-allow-all-builds=true',
  ].join('\n'))

  const env = {
    [PATH_NAME]: `${pnpmHome}${path.delimiter}${process.env[PATH_NAME]!}`,
    HOME: home,
    XDG_CONFIG_HOME: cfgHome,
    PNPM_HOME: pnpmHome,
    XDG_DATA_HOME: global,
  }

  // global install should run scripts
  await execPnpm(['add', '-g', '@pnpm.e2e/postinstall-calls-pnpm@1.0.0'], { env })
  expect(fs.readdirSync(path.join(globalDir, 'node_modules/@pnpm.e2e/postinstall-calls-pnpm'))).toContain('created-by-postinstall')

  // local config should override global config
  await execPnpm(['add', '@pnpm.e2e/postinstall-calls-pnpm@1.0.0'], { env })
  expect(fs.readdirSync(path.resolve('node_modules/@pnpm.e2e/postinstall-calls-pnpm'))).not.toContain('created-by-postinstall')

  // global config should be used if local config did not specify
  delete manifest.pnpm!.allowBuilds
  project.writePackageJson(manifest)
  fs.rmSync('node_modules', { recursive: true })
  fs.rmSync('pnpm-lock.yaml')
  await execPnpm(['add', '@pnpm.e2e/postinstall-calls-pnpm@1.0.0'], { env })
  expect(fs.readdirSync(path.resolve('node_modules/@pnpm.e2e/postinstall-calls-pnpm'))).toContain('created-by-postinstall')
})

// CONTEXT: dangerously-allow-all-builds has been removed from rc files, as a result, this test no longer applies
// TODO: Maybe we should create a yaml config file specifically for `--global`? After all, this test is to serve such use-cases
test.skip('dangerously-allow-all-builds=false in global config', async () => {
  // the directory structure below applies only to Linux
  if (process.platform !== 'linux') return

  const manifest: ProjectManifest = {
    name: 'local',
    version: '0.0.0',
    private: true,
    pnpm: {
      allowBuilds: { '@pnpm.e2e/postinstall-calls-pnpm': true },
    },
  }

  const project = prepare(manifest)

  const home = path.resolve('..', 'home/username')
  const cfgHome = path.resolve(home, '.config')
  const pnpmCfgDir = path.resolve(cfgHome, 'pnpm')
  const pnpmRcFile = path.join(pnpmCfgDir, 'rc')
  const global = path.resolve('..', 'global')
  const pnpmHome = path.join(global, 'pnpm')
  const globalDir = globalPkgDir(pnpmHome)
  fs.mkdirSync(pnpmCfgDir, { recursive: true })
  fs.writeFileSync(pnpmRcFile, [
    'reporter=append-only',
    'dangerously-allow-all-builds=false',
  ].join('\n'))

  const env = {
    [PATH_NAME]: `${pnpmHome}${path.delimiter}${process.env[PATH_NAME]!}`,
    HOME: home,
    XDG_CONFIG_HOME: cfgHome,
    PNPM_HOME: pnpmHome,
    XDG_DATA_HOME: global,
  }

  // global install should run scripts
  await execPnpm(['add', '-g', '@pnpm.e2e/postinstall-calls-pnpm@1.0.0'], { env })
  expect(fs.readdirSync(path.join(globalDir, 'node_modules/@pnpm.e2e/postinstall-calls-pnpm'))).not.toContain('created-by-postinstall')

  // local config should override global config
  await execPnpm(['add', '@pnpm.e2e/postinstall-calls-pnpm@1.0.0'], { env })
  expect(fs.readdirSync(path.resolve('node_modules/@pnpm.e2e/postinstall-calls-pnpm'))).toContain('created-by-postinstall')

  // global config should be used if local config did not specify
  delete manifest.pnpm!.allowBuilds
  project.writePackageJson(manifest)
  fs.rmSync('node_modules', { recursive: true })
  fs.rmSync('pnpm-lock.yaml')
  await execPnpm(['add', '@pnpm.e2e/postinstall-calls-pnpm@1.0.0'], { env })
  expect(fs.readdirSync(path.resolve('node_modules/@pnpm.e2e/postinstall-calls-pnpm'))).not.toContain('created-by-postinstall')
})

test('global update to latest', async () => {
  prepare()
  const global = path.resolve('..', 'global')
  const pnpmHome = path.join(global, 'pnpm')
  fs.mkdirSync(global)

  const env = { [PATH_NAME]: pnpmHome, PNPM_HOME: pnpmHome, XDG_DATA_HOME: global }

  await execPnpm(['add', '--global', 'is-positive@1'], { env })
  await execPnpm(['update', '--global', '--latest'], { env })

  const isPositivePath = findGlobalPkg(globalPkgDir(pnpmHome), 'is-positive')
  expect(isPositivePath).toBeTruthy()
  const pkgJson = JSON.parse(fs.readFileSync(path.join(isPositivePath!, 'package.json'), 'utf-8'))
  expect(pkgJson.version).toBe('3.1.0')
})

test('global update should not crash if there are no global packages', async () => {
  prepare()
  const global = path.resolve('..', 'global')
  const pnpmHome = path.join(global, 'pnpm')
  fs.mkdirSync(global)

  const env = { [PATH_NAME]: pnpmHome, PNPM_HOME: pnpmHome, XDG_DATA_HOME: global }

  expect(execPnpmSync(['update', '--global'], { env }).status).toBe(0)
})

test('global add cleans up stale bins when re-adding a package with different bins', async () => {
  prepare()
  const global = path.resolve('..', 'global')
  const pnpmHome = path.join(global, 'pnpm')
  fs.mkdirSync(pnpmHome, { recursive: true })

  const env = { [PATH_NAME]: pnpmHome, PNPM_HOME: pnpmHome, XDG_DATA_HOME: global }

  // Create v1 tarball with bin "old-bin"
  const pkgDir = path.resolve('..', 'my-tool')
  fs.mkdirSync(path.join(pkgDir, 'package'), { recursive: true })
  fs.writeFileSync(path.join(pkgDir, 'package', 'package.json'), JSON.stringify({
    name: 'my-tool',
    version: '1.0.0',
    bin: { 'old-bin': './index.js' },
  }))
  fs.writeFileSync(path.join(pkgDir, 'package', 'index.js'), '#!/usr/bin/env node\nconsole.log("v1")\n')
  const tarballV1 = path.join(pkgDir, 'my-tool-1.0.0.tgz')
  execPnpmSync(['pack', '--pack-destination', pkgDir], { cwd: path.join(pkgDir, 'package') })

  await execPnpm(['add', '-g', tarballV1], { env })
  expect(fs.existsSync(path.join(pnpmHome, 'old-bin'))).toBeTruthy()

  // Create v2 tarball with bin "new-bin"
  fs.writeFileSync(path.join(pkgDir, 'package', 'package.json'), JSON.stringify({
    name: 'my-tool',
    version: '2.0.0',
    bin: { 'new-bin': './index.js' },
  }))
  const tarballV2 = path.join(pkgDir, 'my-tool-2.0.0.tgz')
  execPnpmSync(['pack', '--pack-destination', pkgDir], { cwd: path.join(pkgDir, 'package') })

  // Re-add the same package from new tarball — old bins should be cleaned up
  await execPnpm(['add', '-g', tarballV2], { env })

  // old-bin should be gone, new-bin should exist
  expect(fs.existsSync(path.join(pnpmHome, 'old-bin'))).toBeFalsy()
  expect(fs.existsSync(path.join(pnpmHome, 'new-bin'))).toBeTruthy()
})

test('global add refuses to install when bin name conflicts with another global package', async () => {
  prepare()
  const global = path.resolve('..', 'global')
  const pnpmHome = path.join(global, 'pnpm')
  fs.mkdirSync(pnpmHome, { recursive: true })

  const env = { [PATH_NAME]: pnpmHome, PNPM_HOME: pnpmHome, XDG_DATA_HOME: global }

  // Create two local packages that both expose a bin called "my-bin"
  const pkgA = path.resolve('..', 'pkg-a')
  fs.mkdirSync(pkgA, { recursive: true })
  fs.writeFileSync(path.join(pkgA, 'package.json'), JSON.stringify({
    name: 'pkg-a',
    version: '1.0.0',
    bin: { 'my-bin': './index.js' },
  }))
  fs.writeFileSync(path.join(pkgA, 'index.js'), '#!/usr/bin/env node\nconsole.log("a")\n')

  const pkgB = path.resolve('..', 'pkg-b')
  fs.mkdirSync(pkgB, { recursive: true })
  fs.writeFileSync(path.join(pkgB, 'package.json'), JSON.stringify({
    name: 'pkg-b',
    version: '1.0.0',
    bin: { 'my-bin': './index.js' },
  }))
  fs.writeFileSync(path.join(pkgB, 'index.js'), '#!/usr/bin/env node\nconsole.log("b")\n')

  // Install pkg-a globally — should succeed
  await execPnpm(['add', '-g', pkgA], { env })
  expect(findGlobalPkg(globalPkgDir(pnpmHome), 'pkg-a')).toBeTruthy()

  // Install pkg-b globally — should fail due to bin conflict
  const result = execPnpmSync(['add', '-g', pkgB], { env })
  expect(result.status).not.toBe(0)
  expect(result.stdout.toString()).toContain('ERR_PNPM_GLOBAL_BIN_CONFLICT')

  // pkg-a should still be installed
  expect(findGlobalPkg(globalPkgDir(pnpmHome), 'pkg-a')).toBeTruthy()
})

test('global remove deletes install group and bin shims', async () => {
  prepare()
  const global = path.resolve('..', 'global')
  const pnpmHome = path.join(global, 'pnpm')
  fs.mkdirSync(pnpmHome, { recursive: true })

  const env = { [PATH_NAME]: pnpmHome, PNPM_HOME: pnpmHome, XDG_DATA_HOME: global }

  // Create two packages with bins and install them together as a group
  const pkgA = path.resolve('..', 'tool-a')
  fs.mkdirSync(pkgA, { recursive: true })
  fs.writeFileSync(path.join(pkgA, 'package.json'), JSON.stringify({
    name: 'tool-a',
    version: '1.0.0',
    bin: { 'tool-a-bin': './index.js' },
  }))
  fs.writeFileSync(path.join(pkgA, 'index.js'), '#!/usr/bin/env node\nconsole.log("a")\n')

  const pkgB = path.resolve('..', 'tool-b')
  fs.mkdirSync(pkgB, { recursive: true })
  fs.writeFileSync(path.join(pkgB, 'package.json'), JSON.stringify({
    name: 'tool-b',
    version: '1.0.0',
    bin: { 'tool-b-bin': './index.js' },
  }))
  fs.writeFileSync(path.join(pkgB, 'index.js'), '#!/usr/bin/env node\nconsole.log("b")\n')

  // Install as a group
  await execPnpm(['add', '-g', pkgA, pkgB], { env })
  expect(fs.existsSync(path.join(pnpmHome, 'tool-a-bin'))).toBeTruthy()
  expect(fs.existsSync(path.join(pnpmHome, 'tool-b-bin'))).toBeTruthy()

  // Remove one package — entire group (both bins) should be removed
  await execPnpm(['remove', '-g', 'tool-a'], { env })
  expect(fs.existsSync(path.join(pnpmHome, 'tool-a-bin'))).toBeFalsy()
  expect(fs.existsSync(path.join(pnpmHome, 'tool-b-bin'))).toBeFalsy()
  expect(findGlobalPkg(globalPkgDir(pnpmHome), 'tool-a')).toBeNull()
  expect(findGlobalPkg(globalPkgDir(pnpmHome), 'tool-b')).toBeNull()
})
