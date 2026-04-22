/// <reference path="../../../../__typings__/index.d.ts"/>
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
