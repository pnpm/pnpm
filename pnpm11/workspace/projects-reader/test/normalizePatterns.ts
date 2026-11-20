import path from 'node:path'

import { expect, test } from '@jest/globals'
import { findPackages } from '@pnpm/workspace.projects-reader'

const root = path.join(import.meta.dirname, 'findPackages-fixtures/many-pkgs-2')

test.each([
  './components/component-1',
  '././components/component-1/',
  './components/./component-1',
  'components/./component-1',
  'components/component-1/.',
  'components//component-1',
  './/components/component-1',
  'components/missing/../component-1',
])('normalizes literal pattern %s', async (pattern) => {
  const projects = await findPackages(root, { patterns: [pattern], includeRoot: true })
  expect(projects.map(({ manifest }) => manifest.name).sort()).toStrictEqual(['component-1', 'many-pkgs-2'])
})

test.each([
  './components/*',
  '././components/*',
  './components/**',
  './components/{component-1,component-2}',
  'components//./*',
  'components/missing/../*',
])('normalizes glob pattern %s', async (pattern) => {
  const projects = await findPackages(root, { patterns: [pattern], includeRoot: true })
  expect(projects.map(({ manifest }) => manifest.name).sort()).toStrictEqual(['component-1', 'component-2', 'many-pkgs-2'])
})

test.each([
  '!./components/component-1',
  '!././components/component-1/',
  '!components/./component-1',
  '!./components//component-1',
  '!components/missing/../component-1',
].flatMap((pattern) => ['components/*', './components/**'].map((include) => ({ include, pattern }))))('normalizes $pattern with $include', async ({ include, pattern }) => {
  const projects = await findPackages(root, { patterns: [include, pattern], includeRoot: true })
  expect(projects.map(({ manifest }) => manifest.name).sort()).toStrictEqual(['component-2', 'many-pkgs-2'])
})

test.each([
  './../many-pkgs-2/components/*',
  '././../many-pkgs-2/components/*',
  '../many-pkgs-2/components/missing/../*',
])('normalizes parent-relative pattern %s', async (pattern) => {
  const projects = await findPackages(root, {
    patterns: [pattern, '!./../many-pkgs-2/components/./component-1'],
    includeRoot: true,
  })
  expect(projects.map(({ manifest }) => manifest.name).sort()).toStrictEqual(['component-2', 'many-pkgs-2'])
})

test('deduplicates normalized patterns and preserves the workspace root', async () => {
  const projects = await findPackages(root, {
    patterns: ['././', 'components/../', 'components/component-1', './components/./component-1'],
    includeRoot: true,
  })
  expect(projects.map(({ manifest }) => manifest.name).sort()).toStrictEqual(['component-1', 'many-pkgs-2'])
})

test.each([
  '!.',
  '!./',
  '!components/..',
])('drops negation %s that normalizes to the workspace root', async (pattern) => {
  const projects = await findPackages(root, { patterns: ['components/*', pattern], includeRoot: true })
  expect(projects.map(({ manifest }) => manifest.name).sort()).toStrictEqual(['component-1', 'component-2', 'many-pkgs-2'])
})
