/// <reference path="../../../__typings__/index.d.ts"/>
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { isWorkspaceProjectDir, normalizePatterns } from '@pnpm/workspace.package-patterns'

const workspaceDir = path.resolve('/workspace')

test('a directory pattern selects the manifests inside it', () => {
  expect(normalizePatterns(['.', 'packages/*', 'libs/'])).toStrictEqual([
    './package.{json,yaml,json5}',
    'packages/*/package.{json,yaml,json5}',
    'libs/package.{json,yaml,json5}',
  ])
})

test('the workspace root is a project even when no pattern selects it', () => {
  expect(isWorkspaceProjectDir({ workspaceDir, dir: workspaceDir, patterns: ['packages/*'] })).toBe(true)
})

test.each([
  [['packages/**', '!examples/**'], 'packages/package-1', true],
  [['packages/**', '!examples/**'], 'examples/example-1', false],
  [['packages/**', '!examples/**'], 'docs', false],
  [['packages/*'], 'packages/package-1/nested', false],
  [['packages/**'], 'packages/package-1/nested', true],
  [['./packages/./package-1'], 'packages/package-1', true],
  [['packages/missing/../package-1'], 'packages/package-1', true],
  // A pattern that names an absolute path selects nothing, and negating one
  // excludes nothing, because the paths are relative to the workspace root.
  [['**', '!/libs/**'], 'libs/lib-1', true],
  [['/libs/**'], 'libs/lib-1', false],
  [['**/.dev/**', '!**/.dev/**'], 'packages/.dev/tool', false],
  [['**', '!**/.dev/**'], 'packages/tool', true],
  [['**'], 'node_modules/is-positive', false],
  [[], 'packages/package-1', false],
])('%s selects %s: %s', (patterns, dir, expected) => {
  expect(isWorkspaceProjectDir({ workspaceDir, dir: path.join(workspaceDir, dir), patterns })).toBe(expected)
})

test('without patterns every directory below the workspace root is a project', () => {
  expect(isWorkspaceProjectDir({ workspaceDir, dir: path.join(workspaceDir, 'docs') })).toBe(true)
})
