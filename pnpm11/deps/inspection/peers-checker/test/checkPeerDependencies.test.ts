/// <reference path="../../../../__typings__/index.d.ts"/>
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { checkPeerDependencies } from '@pnpm/deps.inspection.peers-checker'
import { fixtures } from '@pnpm/test-fixtures'

const testFixtures = fixtures(import.meta.dirname)

test('detects unmet peer dependencies', async () => {
  const fixture = testFixtures.find('with-unmet-peers')
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
  const fixture = testFixtures.find('with-missing-peer')
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
  const fixture = testFixtures.find('with-peer')
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
  const fixture = testFixtures.find('with-loose-peer-range')
  const issues = await checkPeerDependencies([fixture], {
    lockfileDir: fixture,
    checkWantedLockfileOnly: true,
  })

  const projectIssues = issues['.']
  expect(projectIssues).toBeDefined()
  expect(Object.keys(projectIssues.bad)).toHaveLength(0)
  expect(Object.keys(projectIssues.missing)).toHaveLength(0)
})

test('resolves the catalog peer range of an injected workspace package through the catalogs', async () => {
  const fixture = testFixtures.find('with-injected-catalog-peer')
  const issues = await checkPeerDependencies([path.join(fixture, 'packages/app')], {
    lockfileDir: fixture,
    catalogs: { default: { 'is-odd': '3.0.1' } },
    checkWantedLockfileOnly: true,
  })

  const projectIssues = issues['packages/app']
  expect(projectIssues).toBeDefined()
  expect(Object.keys(projectIssues.bad)).toHaveLength(0)
  expect(Object.keys(projectIssues.missing)).toHaveLength(0)
})

test('reports a catalog peer range of an injected workspace package that the installed version does not satisfy', async () => {
  const fixture = testFixtures.find('with-injected-catalog-peer')
  const issues = await checkPeerDependencies([path.join(fixture, 'packages/app')], {
    lockfileDir: fixture,
    catalogs: { default: { 'is-odd': '^4.0.0' } },
    checkWantedLockfileOnly: true,
  })

  const projectIssues = issues['packages/app']
  expect(projectIssues.bad['is-odd']).toStrictEqual([expect.objectContaining({
    wantedRange: '^4.0.0',
    foundVersion: '3.0.1',
  })])
})

test('reports the catalog peer range of an injected workspace package that has no catalog entry as written', async () => {
  const fixture = testFixtures.find('with-injected-catalog-peer')
  const issues = await checkPeerDependencies([path.join(fixture, 'packages/app')], {
    lockfileDir: fixture,
    catalogs: {},
    checkWantedLockfileOnly: true,
  })

  const projectIssues = issues['packages/app']
  expect(projectIssues.bad['is-odd']).toStrictEqual([expect.objectContaining({
    wantedRange: 'catalog:',
    foundVersion: '3.0.1',
  })])
})

test('respects peerDependencyRules.allowAny', async () => {
  const fixture = testFixtures.find('with-unmet-peers')
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
  const fixture = testFixtures.find('with-missing-peer')
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
  const fixture = testFixtures.find('with-conflicting-peers')
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
  const fixture = testFixtures.find('with-conflicting-peers')
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
  const fixture = testFixtures.find('empty')
  const issues = await checkPeerDependencies([fixture], {
    lockfileDir: fixture,
    checkWantedLockfileOnly: true,
  })

  const projectIssues = issues['.']
  expect(projectIssues).toBeDefined()
  expect(Object.keys(projectIssues.bad)).toHaveLength(0)
  expect(Object.keys(projectIssues.missing)).toHaveLength(0)
})
