import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { checkPeerDependencies } from '@pnpm/deps.inspection.peers-checker'
import { fixtures } from '@pnpm/test-fixtures'

const f = fixtures(import.meta.dirname)

test('detects unmet peer dependencies', async () => {
  const fixture = f.find('with-unmet-peers')
  const issues = await checkPeerDependencies([fixture], {
    lockfileDir: fixture,
    checkWantedLockfileOnly: true,
  })

  const projectIssues = issues['.']
  expect(projectIssues).toBeDefined()
  expect(projectIssues.bad).toHaveProperty('react')
  expect(projectIssues.bad.react).toHaveLength(1)
  expect(projectIssues.bad.react[0]).toMatchObject({
    wantedRange: '^18.0.0',
    foundVersion: '17.0.0',
    parents: [
      { name: 'react-dom', version: '18.0.0' },
    ],
  })
})

test('detects missing peer dependencies', async () => {
  const fixture = f.find('with-missing-peer')
  const issues = await checkPeerDependencies([fixture], {
    lockfileDir: fixture,
    checkWantedLockfileOnly: true,
  })

  const projectIssues = issues['.']
  expect(projectIssues).toBeDefined()
  expect(projectIssues.missing).toHaveProperty('ajv')
  expect(projectIssues.missing.ajv).toHaveLength(1)
  expect(projectIssues.missing.ajv[0]).toMatchObject({
    wantedRange: '^6.9.1',
    parents: [
      { name: 'ajv-keywords', version: '3.4.1' },
    ],
  })
  expect(projectIssues.intersections).toHaveProperty('ajv')
})

test('reports no issues for satisfied peer dependencies', async () => {
  const fixture = f.find('with-peer')
  const issues = await checkPeerDependencies([fixture], {
    lockfileDir: fixture,
    checkWantedLockfileOnly: true,
  })

  const projectIssues = issues['.']
  expect(projectIssues).toBeDefined()
  expect(Object.keys(projectIssues.bad)).toHaveLength(0)
  expect(Object.keys(projectIssues.missing)).toHaveLength(0)
})

test('reports no issues for satisfied loose peer dependency ranges', async () => {
  const fixture = f.find('with-loose-peer-range')
  const issues = await checkPeerDependencies([fixture], {
    lockfileDir: fixture,
    checkWantedLockfileOnly: true,
  })

  const projectIssues = issues['.']
  expect(projectIssues).toBeDefined()
  expect(Object.keys(projectIssues.bad)).toHaveLength(0)
  expect(Object.keys(projectIssues.missing)).toHaveLength(0)
})

test('respects peerDependencyRules.allowAny', async () => {
  const fixture = f.find('with-unmet-peers')
  const issues = await checkPeerDependencies([fixture], {
    lockfileDir: fixture,
    checkWantedLockfileOnly: true,
    peerDependencyRules: {
      allowAny: ['react'],
    },
  })

  const projectIssues = issues['.']
  expect(Object.keys(projectIssues.bad)).toHaveLength(0)
})

test('respects peerDependencyRules.ignoreMissing', async () => {
  const fixture = f.find('with-missing-peer')
  const issues = await checkPeerDependencies([fixture], {
    lockfileDir: fixture,
    checkWantedLockfileOnly: true,
    peerDependencyRules: {
      ignoreMissing: ['ajv'],
    },
  })

  const projectIssues = issues['.']
  expect(Object.keys(projectIssues.missing)).toHaveLength(0)
})

test('detects conflicting missing peer dependencies', async () => {
  const fixture = f.find('with-conflicting-peers')
  const issues = await checkPeerDependencies([fixture], {
    lockfileDir: fixture,
    checkWantedLockfileOnly: true,
  })

  const projectIssues = issues['.']
  expect(projectIssues.missing).toHaveProperty('react')
  expect(projectIssues.missing.react).toHaveLength(2)
  expect(projectIssues.conflicts).toStrictEqual(['react'])
  expect(projectIssues.intersections).not.toHaveProperty('react')
})

test('peerDependencyRules.ignoreMissing also removes the conflicts of ignored peers', async () => {
  const fixture = f.find('with-conflicting-peers')
  const issues = await checkPeerDependencies([fixture], {
    lockfileDir: fixture,
    checkWantedLockfileOnly: true,
    peerDependencyRules: {
      ignoreMissing: ['react'],
    },
  })

  const projectIssues = issues['.']
  expect(Object.keys(projectIssues.missing)).toHaveLength(0)
  expect(projectIssues.conflicts).toStrictEqual([])
})

test('returns no issues when there are no peer dependency problems', async () => {
  const fixture = f.find('empty')
  const issues = await checkPeerDependencies([fixture], {
    lockfileDir: fixture,
    checkWantedLockfileOnly: true,
  })

  const projectIssues = issues['.']
  expect(projectIssues).toBeDefined()
  expect(Object.keys(projectIssues.bad)).toHaveLength(0)
  expect(Object.keys(projectIssues.missing)).toHaveLength(0)
})

test('detects unmet peer dependencies from linked workspace package', async () => {
  const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-linked-peers-test-'))
  const libDir = path.join(tempDir, 'packages/lib')
  const appDir = path.join(tempDir, 'packages/app')
  fs.mkdirSync(libDir, { recursive: true })
  fs.mkdirSync(appDir, { recursive: true })

  fs.writeFileSync(path.join(tempDir, 'package.json'), JSON.stringify({ name: 'root', version: '1.0.0' }))
  fs.writeFileSync(path.join(tempDir, 'pnpm-workspace.yaml'), 'packages:\n  - packages/*\n')
  fs.writeFileSync(path.join(libDir, 'package.json'), JSON.stringify({
    name: 'lib',
    version: '1.0.0',
    peerDependencies: { foo: '^2.0.0' },
  }))
  fs.writeFileSync(path.join(appDir, 'package.json'), JSON.stringify({
    name: 'app',
    version: '1.0.0',
    dependencies: { lib: 'workspace:*', foo: '1.0.0' },
  }))
  fs.writeFileSync(path.join(tempDir, 'pnpm-lock.yaml'), `lockfileVersion: '9.0'
settings:
  autoInstallPeers: true
  excludeLinksFromLockfile: false
importers:
  packages/lib: {}
  packages/app:
    dependencies:
      lib:
        specifier: workspace:*
        version: link:../lib
      foo:
        specifier: 1.0.0
        version: 1.0.0
packages:
  foo@1.0.0:
    resolution: {integrity: dummy}
`)

  const issues = await checkPeerDependencies([appDir], {
    lockfileDir: tempDir,
    checkWantedLockfileOnly: true,
  })

  const projectIssues = issues['packages/app']
  expect(projectIssues).toBeDefined()
  expect(projectIssues.bad).toHaveProperty('foo')
  expect(projectIssues.bad.foo[0]).toMatchObject({
    wantedRange: '^2.0.0',
    foundVersion: '1.0.0',
    parents: [{ name: 'lib', version: '1.0.0' }],
  })

  fs.rmSync(tempDir, { recursive: true, force: true })
})

test('detects missing peer dependencies from linked workspace package', async () => {
  const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-linked-missing-peers-test-'))
  const libDir = path.join(tempDir, 'packages/lib')
  const appDir = path.join(tempDir, 'packages/app')
  fs.mkdirSync(libDir, { recursive: true })
  fs.mkdirSync(appDir, { recursive: true })

  fs.writeFileSync(path.join(tempDir, 'package.json'), JSON.stringify({ name: 'root', version: '1.0.0' }))
  fs.writeFileSync(path.join(tempDir, 'pnpm-workspace.yaml'), 'packages:\n  - packages/*\n')
  fs.writeFileSync(path.join(libDir, 'package.json'), JSON.stringify({
    name: 'lib',
    version: '1.0.0',
    peerDependencies: { foo: '^2.0.0' },
  }))
  fs.writeFileSync(path.join(appDir, 'package.json'), JSON.stringify({
    name: 'app',
    version: '1.0.0',
    dependencies: { lib: 'workspace:*' },
  }))
  fs.writeFileSync(path.join(tempDir, 'pnpm-lock.yaml'), `lockfileVersion: '9.0'
settings:
  autoInstallPeers: true
  excludeLinksFromLockfile: false
importers:
  packages/lib: {}
  packages/app:
    dependencies:
      lib:
        specifier: workspace:*
        version: link:../lib
`)

  const issues = await checkPeerDependencies([appDir], {
    lockfileDir: tempDir,
    checkWantedLockfileOnly: true,
  })

  const projectIssues = issues['packages/app']
  expect(projectIssues).toBeDefined()
  expect(projectIssues.missing).toHaveProperty('foo')
  expect(projectIssues.missing.foo[0]).toMatchObject({
    wantedRange: '^2.0.0',
    parents: [{ name: 'lib', version: '1.0.0' }],
  })

  fs.rmSync(tempDir, { recursive: true, force: true })
})

test('reports no issues when linked workspace package peer dependencies are satisfied by dependent', async () => {
  const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-linked-satisfied-peers-test-'))
  const libDir = path.join(tempDir, 'packages/lib')
  const appDir = path.join(tempDir, 'packages/app')
  fs.mkdirSync(libDir, { recursive: true })
  fs.mkdirSync(appDir, { recursive: true })

  fs.writeFileSync(path.join(tempDir, 'package.json'), JSON.stringify({ name: 'root', version: '1.0.0' }))
  fs.writeFileSync(path.join(tempDir, 'pnpm-workspace.yaml'), 'packages:\n  - packages/*\n')
  fs.writeFileSync(path.join(libDir, 'package.json'), JSON.stringify({
    name: 'lib',
    version: '1.0.0',
    peerDependencies: { foo: '^2.0.0' },
  }))
  fs.writeFileSync(path.join(appDir, 'package.json'), JSON.stringify({
    name: 'app',
    version: '1.0.0',
    dependencies: { lib: 'workspace:*', foo: '2.1.0' },
  }))
  fs.writeFileSync(path.join(tempDir, 'pnpm-lock.yaml'), `lockfileVersion: '9.0'
settings:
  autoInstallPeers: true
  excludeLinksFromLockfile: false
importers:
  packages/lib: {}
  packages/app:
    dependencies:
      lib:
        specifier: workspace:*
        version: link:../lib
      foo:
        specifier: 2.1.0
        version: 2.1.0
packages:
  foo@2.1.0:
    resolution: {integrity: dummy}
`)

  const issues = await checkPeerDependencies([appDir], {
    lockfileDir: tempDir,
    checkWantedLockfileOnly: true,
  })

  const projectIssues = issues['packages/app']
  expect(projectIssues).toBeDefined()
  expect(Object.keys(projectIssues.bad)).toHaveLength(0)
  expect(Object.keys(projectIssues.missing)).toHaveLength(0)

  fs.rmSync(tempDir, { recursive: true, force: true })
})

test('reports no issues when linked workspace package peer dependencies are satisfied by workspace root', async () => {
  const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-linked-root-satisfied-peers-test-'))
  const libDir = path.join(tempDir, 'packages/lib')
  const appDir = path.join(tempDir, 'packages/app')
  fs.mkdirSync(libDir, { recursive: true })
  fs.mkdirSync(appDir, { recursive: true })

  fs.writeFileSync(path.join(tempDir, 'package.json'), JSON.stringify({
    name: 'root',
    version: '1.0.0',
    dependencies: { foo: '2.1.0' },
  }))
  fs.writeFileSync(path.join(tempDir, 'pnpm-workspace.yaml'), 'packages:\n  - packages/*\n')
  fs.writeFileSync(path.join(libDir, 'package.json'), JSON.stringify({
    name: 'lib',
    version: '1.0.0',
    peerDependencies: { foo: '^2.0.0' },
  }))
  fs.writeFileSync(path.join(appDir, 'package.json'), JSON.stringify({
    name: 'app',
    version: '1.0.0',
    dependencies: { lib: 'workspace:*' },
  }))
  fs.writeFileSync(path.join(tempDir, 'pnpm-lock.yaml'), `lockfileVersion: '9.0'
settings:
  autoInstallPeers: true
  excludeLinksFromLockfile: false
importers:
  .:
    dependencies:
      foo:
        specifier: 2.1.0
        version: 2.1.0
  packages/lib: {}
  packages/app:
    dependencies:
      lib:
        specifier: workspace:*
        version: link:../lib
packages:
  foo@2.1.0:
    resolution: {integrity: dummy}
`)

  const issues = await checkPeerDependencies([appDir], {
    lockfileDir: tempDir,
    checkWantedLockfileOnly: true,
  })

  const projectIssues = issues['packages/app']
  expect(projectIssues).toBeDefined()
  expect(Object.keys(projectIssues.bad)).toHaveLength(0)
  expect(Object.keys(projectIssues.missing)).toHaveLength(0)

  fs.rmSync(tempDir, { recursive: true, force: true })
})

