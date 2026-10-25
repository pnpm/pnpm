import assert from 'node:assert/strict'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { after, describe, test } from 'node:test'

import { type ProjectPin, readProjectPin } from '../lib/projectPin.js'

const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'get-pnpm-project-pin-'))
let projectCount = 0

after(() => {
  fs.rmSync(tmpDir, { recursive: true, force: true })
})

describe('readProjectPin', () => {
  test('reads a range from devEngines.packageManager', () => {
    const dir = project({ 'package.json': manifest({ devEngines: { packageManager: { name: 'pnpm', version: '^12.1.0' } } }) })

    assert.deepEqual(pinOf(dir), {
      spec: '^12.1.0',
      field: 'devEngines.packageManager',
      manifestPath: path.join(dir, 'package.json'),
    })
  })

  test('takes pnpm\'s entry from a list in devEngines.packageManager', () => {
    const dir = project({
      'package.json': manifest({
        devEngines: { packageManager: [{ name: 'yarn', version: '4.0.0' }, { name: 'pnpm', version: '12.1.0' }] },
      }),
    })

    assert.equal(pinOf(dir)?.spec, '12.1.0')
  })

  test('prefers devEngines.packageManager over packageManager', () => {
    const dir = project({
      'package.json': manifest({
        packageManager: 'pnpm@11.20.0',
        devEngines: { packageManager: { name: 'pnpm', version: '12.1.0' } },
      }),
    })

    assert.deepEqual(pick(pinOf(dir)!), { spec: '12.1.0', field: 'devEngines.packageManager' })
  })

  test('reads packageManager without the build Corepack records', () => {
    const dir = project({ 'package.json': manifest({ packageManager: `pnpm@12.1.0+sha512.${'a'.repeat(128)}` }) })

    assert.deepEqual(pick(pinOf(dir)!), { spec: '12.1.0', field: 'packageManager' })
  })

  test('refuses a packageManager that pins no exact version', () => {
    const dir = project({ 'package.json': manifest({ packageManager: 'pnpm@^12.1.0' }) })

    assert.throws(() => pinOf(dir), /pins no exact pnpm version/)
  })

  test('reads no pin from a project that uses another package manager', () => {
    const viaDevEngines = project({ 'package.json': manifest({ devEngines: { packageManager: { name: 'yarn', version: '4.0.0' } } }) })
    const viaPackageManager = project({ 'package.json': manifest({ packageManager: 'yarn@4.0.0' }) })

    assert.equal(pinOf(viaDevEngines), undefined)
    assert.equal(pinOf(viaPackageManager), undefined)
  })

  test('refuses a devEngines.packageManager entry with no usable version', () => {
    const dir = project({ 'package.json': manifest({ devEngines: { packageManager: { name: 'pnpm', version: 'latest-ish' } } }) })

    assert.throws(() => pinOf(dir), /no version or range of it/)
  })

  test('reads no pin from a project that pins nothing', () => {
    const dir = project({ 'package.json': manifest({}) })

    assert.equal(pinOf(dir), undefined)
  })

  test('reads the pin of a package.json5 or package.yaml', () => {
    const json5 = project({ 'package.json5': "{ packageManager: 'pnpm@12.1.0' }" })
    const yaml = project({ 'package.yaml': 'devEngines:\n  packageManager:\n    name: pnpm\n    version: ^12.1.0\n' })

    assert.deepEqual(pinOf(json5), { spec: '12.1.0', field: 'packageManager', manifestPath: path.join(json5, 'package.json5') })
    assert.deepEqual(pinOf(yaml), { spec: '^12.1.0', field: 'devEngines.packageManager', manifestPath: path.join(yaml, 'package.yaml') })
  })

  test('reads a manifest that starts with a byte order mark', () => {
    const dir = project({ 'package.json': `\uFEFF${manifest({ packageManager: 'pnpm@12.1.0' })}` })

    assert.equal(pinOf(dir)?.spec, '12.1.0')
  })

  test('names the manifest that is not valid JSON', () => {
    const dir = project({ 'package.json': '{ "packageManager": ' })

    assert.throws(() => pinOf(dir), (err: Error) => err.message.startsWith(`${path.join(dir, 'package.json')} is not valid JSON:`))
  })

  test('reads the manifest of the nearest project above the directory', () => {
    const dir = project({ 'package.json': manifest({ packageManager: 'pnpm@12.1.0' }), 'src/lib/.keep': '' })

    assert.equal(pinOf(path.join(dir, 'src', 'lib'))?.spec, '12.1.0')
  })

  test('reads the pin of the workspace root from a member', () => {
    const dir = project({
      'pnpm-workspace.yaml': 'packages:\n  - packages/*\n',
      'package.json': manifest({ devEngines: { packageManager: { name: 'pnpm', version: '^12.1.0' } } }),
      'packages/member/package.json': manifest({ packageManager: 'pnpm@11.20.0' }),
      'pnpm-lock.yaml': lockfile('12.3.0'),
    })

    assert.deepEqual(pinOf(path.join(dir, 'packages', 'member')), {
      spec: '^12.1.0',
      field: 'devEngines.packageManager',
      manifestPath: path.join(dir, 'package.json'),
      lockedVersion: '12.3.0',
    })
  })

  test('reads the lockfile of the workspace root even when the workspace shares none', () => {
    const dir = project({
      'pnpm-workspace.yaml': 'packages:\n  - packages/*\nsharedWorkspaceLockfile: false\n',
      'package.json': manifest({ devEngines: { packageManager: { name: 'pnpm', version: '^12.1.0' } } }),
      'pnpm-lock.yaml': lockfile('12.3.0'),
      'packages/member/package.json': manifest({}),
      'packages/member/pnpm-lock.yaml': lockfile('12.2.0'),
    })

    assert.equal(pinOf(path.join(dir, 'packages', 'member'))?.lockedVersion, '12.3.0')
  })

  test('reads the lockfile in lockfileDir, but not a lockfileDir set through include', () => {
    const files = {
      'package.json': manifest({ devEngines: { packageManager: { name: 'pnpm', version: '^12.1.0' } } }),
      'packages/member/package.json': manifest({}),
      'lock/pnpm-lock.yaml': lockfile('12.3.0'),
    }
    const set = project({ ...files, 'pnpm-workspace.yaml': 'packages:\n  - packages/*\nlockfileDir: ./lock\n' })
    const included = project({
      ...files,
      'pnpm-workspace.yaml': 'packages:\n  - packages/*\ninclude:\n  ./settings.yaml: "*"\n',
      'settings.yaml': 'lockfileDir: ./lock\n',
    })

    assert.equal(pinOf(path.join(set, 'packages', 'member'))?.lockedVersion, '12.3.0')
    assert.equal(pinOf(included)?.lockedVersion, undefined)
  })

  test('falls back to the pin of a member, unlocked, when the workspace root pins nothing', () => {
    const dir = project({
      'pnpm-workspace.yaml': 'packages:\n  - packages/*\n',
      'package.json': manifest({}),
      'pnpm-lock.yaml': lockfile('12.3.0'),
      'packages/member/package.json': manifest({ devEngines: { packageManager: { name: 'pnpm', version: '^12.1.0' } } }),
      'packages/member/pnpm-lock.yaml': lockfile('12.3.0'),
    })

    assert.deepEqual(pinOf(path.join(dir, 'packages', 'member')), {
      spec: '^12.1.0',
      field: 'devEngines.packageManager',
      manifestPath: path.join(dir, 'packages', 'member', 'package.json'),
    })
  })

  test('reads the pin and lockfile of a project the workspace does not include', () => {
    const dir = project({
      'pnpm-workspace.yaml': 'packages:\n  - packages/*\n  - "!packages/excluded"\n',
      'package.json': manifest({ packageManager: 'pnpm@11.20.0' }),
      'pnpm-lock.yaml': lockfile('12.2.0'),
      'other/package.json': manifest({ devEngines: { packageManager: { name: 'pnpm', version: '^12.1.0' } } }),
      'other/pnpm-lock.yaml': lockfile('12.3.0'),
      'packages/excluded/package.json': manifest({ packageManager: 'pnpm@12.1.0' }),
    })
    const everything = project({
      'pnpm-workspace.yaml': 'packages:\n  - "**"\n',
      'package.json': manifest({ packageManager: 'pnpm@11.20.0' }),
      'node_modules/dep/package.json': manifest({ packageManager: 'pnpm@12.1.0' }),
    })

    assert.deepEqual(pinOf(path.join(dir, 'other')), {
      spec: '^12.1.0',
      field: 'devEngines.packageManager',
      manifestPath: path.join(dir, 'other', 'package.json'),
      lockedVersion: '12.3.0',
    })
    assert.equal(pinOf(path.join(dir, 'packages', 'excluded'))?.spec, '12.1.0')
    assert.equal(pinOf(path.join(everything, 'node_modules', 'dep'))?.spec, '12.1.0')
  })

  test('includes no project but the root in a workspace without packages', () => {
    const dir = project({
      'pnpm-workspace.yaml': 'nodeLinker: isolated\n',
      'package.json': manifest({ packageManager: 'pnpm@11.20.0' }),
      'packages/member/package.json': manifest({ packageManager: 'pnpm@12.1.0' }),
    })

    assert.equal(pinOf(path.join(dir, 'packages', 'member'))?.spec, '12.1.0')
  })

  test('falls back to the pin of the workspace root, unlocked, for a project it does not include', () => {
    const dir = project({
      'pnpm-workspace.yaml': 'packages:\n  - packages/*\n',
      'package.json': manifest({ devEngines: { packageManager: { name: 'pnpm', version: '^12.1.0' } } }),
      'pnpm-lock.yaml': lockfile('12.3.0'),
      'other/package.json': manifest({}),
      'other/pnpm-lock.yaml': lockfile('12.3.0'),
    })

    assert.deepEqual(pinOf(path.join(dir, 'other')), {
      spec: '^12.1.0',
      field: 'devEngines.packageManager',
      manifestPath: path.join(dir, 'package.json'),
    })
  })

  test('names the pnpm-workspace.yaml that is not valid YAML', () => {
    const dir = project({ 'pnpm-workspace.yaml': 'packages: [\n', 'package.json': manifest({}) })

    assert.throws(() => pinOf(dir), (err: Error) => err.message.startsWith(`${path.join(dir, 'pnpm-workspace.yaml')} is not valid YAML:`))
  })

  test('takes the locked version only while it is in range', () => {
    const pinned = (lockedVersion: string): string | undefined => pinOf(project({
      'package.json': manifest({ devEngines: { packageManager: { name: 'pnpm', version: '^12.1.0' } } }),
      'pnpm-lock.yaml': lockfile(lockedVersion),
    }))?.lockedVersion

    assert.equal(pinned('12.3.0'), '12.3.0')
    assert.equal(pinned('12.4.0-rc.1'), '12.4.0-rc.1')
    assert.equal(pinned('11.20.0'), undefined)
  })

  test('reads a quoted locked version, and a lockfile that is only the env document', () => {
    const dir = project({
      'package.json': manifest({ devEngines: { packageManager: { name: 'pnpm', version: '>=12' } } }),
      'pnpm-lock.yaml': lockfile("'12.3.0'").split('\n---\n')[0]!,
    })

    assert.equal(pinOf(dir)?.lockedVersion, '12.3.0')
  })

  test('ignores a lockfile without an env document, and the lockfile of an exact pin', () => {
    const noEnvDocument = project({
      'package.json': manifest({ devEngines: { packageManager: { name: 'pnpm', version: '^12.1.0' } } }),
      'pnpm-lock.yaml': "lockfileVersion: '9.0'\n\nimporters:\n\n  .: {}\n",
    })
    const exact = project({
      'package.json': manifest({ devEngines: { packageManager: { name: 'pnpm', version: '12.1.0' } } }),
      'pnpm-lock.yaml': lockfile('12.1.0'),
    })

    assert.equal(pinOf(noEnvDocument)?.lockedVersion, undefined)
    assert.equal(pinOf(exact)?.lockedVersion, undefined)
  })

  test('reads no pin where there is no manifest at all', () => {
    assert.equal(pinOf(project({ 'src/.keep': '' })), undefined)
  })

  test('does not look above the root it is given', () => {
    const dir = project({
      'pnpm-workspace.yaml': '',
      'package.json': manifest({ packageManager: 'pnpm@11.20.0' }),
      'checkout/package.json': manifest({ packageManager: 'pnpm@12.1.0' }),
    })

    assert.equal(readProjectPin(path.join(dir, 'checkout'), { root: path.join(dir, 'checkout') })?.spec, '12.1.0')
  })
})

/**
 * The temporary directory can sit below a workspace of the machine's own, so
 * the search stops at the directory these tests make.
 */
function pinOf (dir: string): ProjectPin | undefined {
  return readProjectPin(dir, { root: tmpDir })
}

function project (files: Record<string, string>): string {
  const dir = path.join(tmpDir, `project-${projectCount++}`)
  for (const [file, content] of Object.entries(files)) {
    fs.mkdirSync(path.dirname(path.join(dir, file)), { recursive: true })
    fs.writeFileSync(path.join(dir, file), content)
  }
  return dir
}

function manifest (fields: Record<string, unknown>): string {
  return JSON.stringify({ name: 'project', ...fields }, null, 2)
}

/** A lockfile led by the env document pnpm writes, with `version` locked. */
function lockfile (version: string): string {
  return `---
lockfileVersion: '9.0'

importers:

  .:
    configDependencies: {}
    packageManagerDependencies:
      pnpm:
        specifier: ^12.1.0
        version: ${version}

packages:

  pnpm@${version.replaceAll("'", '')}:
    resolution: {integrity: sha512-AAAA}

---
lockfileVersion: '9.0'

importers:

  .: {}
`
}

function pick ({ spec, field }: { spec: string, field: string }): { spec: string, field: string } {
  return { spec, field }
}
