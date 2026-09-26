import { stripVTControlCharacters as stripAnsi } from 'node:util'

import { expect, test } from '@jest/globals'
import { renderPeerIssues } from '@pnpm/deps.inspection.peers-issues-renderer'
import type { PeerDependencyIssues } from '@pnpm/types'

test('renderPeerIssues() returns an empty string when there are no issues', () => {
  expect(renderPeerIssues({
    '.': {
      missing: {},
      bad: {},
      conflicts: [],
      intersections: {},
    },
  })).toBe('')
})

test('renderPeerIssues() renders bad peer dependencies', () => {
  expect(stripAnsi(renderPeerIssues({
    '.': {
      missing: {},
      bad: {
        a: [
          {
            parents: [
              { name: 'b', version: '1.0.0' },
            ],
            foundVersion: '2',
            resolvedFrom: [],
            optional: false,
            wantedRange: '3',
          },
        ],
      },
      conflicts: [],
      intersections: {},
    },
  }))).toMatchSnapshot()
})

test('renderPeerIssues() splits bad peer dependencies by foundVersion', () => {
  expect(stripAnsi(renderPeerIssues({
    '.': {
      missing: {},
      bad: {
        a: [
          {
            parents: [{ name: 'b', version: '1.0.0' }],
            foundVersion: '1.0.0',
            resolvedFrom: [],
            optional: false,
            wantedRange: '^2.0.0',
          },
          {
            parents: [{ name: 'c', version: '1.0.0' }],
            foundVersion: '2.0.0',
            resolvedFrom: [],
            optional: false,
            wantedRange: '^3.0.0',
          },
        ],
      },
      conflicts: [],
      intersections: {},
    },
  }))).toMatchSnapshot()
})

test('renderPeerIssues() renders missing peer dependencies that are required', () => {
  expect(stripAnsi(renderPeerIssues({
    '.': {
      missing: {
        a: [
          {
            parents: [
              { name: 'b', version: '1.0.0' },
            ],
            optional: false,
            wantedRange: '^1.0.0',
          },
        ],
      },
      bad: {},
      conflicts: [],
      intersections: { a: '^1.0.0' },
    },
  }))).toMatchSnapshot()
})

test('renderPeerIssues() renders conflicting peer dependencies', () => {
  expect(stripAnsi(renderPeerIssues({
    '.': {
      missing: {
        a: [
          {
            parents: [{ name: 'b', version: '1.0.0' }],
            optional: false,
            wantedRange: '^1.0.0',
          },
          {
            parents: [{ name: 'c', version: '1.0.0' }],
            optional: false,
            wantedRange: '^2.0.0',
          },
        ],
      },
      bad: {},
      conflicts: ['a'],
      intersections: {},
    },
  }))).toMatchSnapshot()
})

test('renderPeerIssues() formats version ranges with spaces or "*" with quotes', () => {
  expect(stripAnsi(renderPeerIssues({
    '.': {
      missing: {
        a: [
          {
            parents: [{ name: 'z', version: '1.0.0' }],
            optional: false,
            wantedRange: '*',
          },
        ],
        b: [
          {
            parents: [{ name: 'z', version: '1.0.0' }],
            optional: false,
            wantedRange: '1 || 2',
          },
        ],
      },
      bad: {},
      conflicts: [],
      intersections: { a: '*', b: '1 || 2' },
    },
  }))).toMatchSnapshot()
})

test('renderPeerIssues() handles missing parents gracefully', () => {
  expect(stripAnsi(renderPeerIssues({
    '.': {
      missing: {
        foo: [
          {
            parents: [],
            optional: false,
            wantedRange: '>=1.0.0 <3.0.0',
          },
        ],
      },
      bad: {},
      conflicts: [],
      intersections: { foo: '^1.0.0' },
    },
  }))).toMatchSnapshot()
})

function missingReactIssues (): PeerDependencyIssues {
  return {
    missing: {
      react: [
        {
          parents: [{ name: '@my-org/package-a', version: '3.1.4' }],
          optional: false,
          wantedRange: '>=18.2.0',
        },
      ],
    },
    bad: {},
    conflicts: [],
    intersections: { react: '>=18.2.0' },
  }
}

// https://github.com/pnpm/pnpm/issues/15351
test('renderPeerIssues() names the project of each issue', () => {
  expect(stripAnsi(renderPeerIssues({
    'apps/web': missingReactIssues(),
    'apps/docs': missingReactIssues(),
  }))).toBe(`apps/docs
  ✕ missing peer react
    Wanted:
      >=18.2.0:
        @my-org/package-a@3.1.4

apps/web
  ✕ missing peer react
    Wanted:
      >=18.2.0:
        @my-org/package-a@3.1.4`)
})

test('renderPeerIssues() skips projects without reportable issues', () => {
  expect(stripAnsi(renderPeerIssues({
    '.': { ...missingReactIssues(), intersections: {} },
    'apps/web': missingReactIssues(),
  }))).toMatch(/^apps\/web\n {2}✕ missing peer react\n/)
})

test('renderPeerIssues() omits the heading when only the root project has issues', () => {
  expect(stripAnsi(renderPeerIssues({
    '.': missingReactIssues(),
  }))).toMatch(/^✕ missing peer react\n {2}Wanted:/)
})

test('renderPeerIssues() names the root project when other projects are listed', () => {
  const rendered = stripAnsi(renderPeerIssues({
    '.': missingReactIssues(),
    'apps/web': { ...missingReactIssues(), intersections: {} },
  }))
  expect(rendered).toMatch(/^\.\n {2}✕ missing peer react\n/)
  expect(rendered).not.toContain('apps/web')
})

test('renderPeerIssues() strips control characters from the project heading', () => {
  const rendered = renderPeerIssues({
    'apps/\u001b[2J\n-web\u202e': missingReactIssues(),
  })
  expect(rendered).not.toContain('\u001b[2J')
  expect(rendered).not.toContain('\u202e')
  expect(stripAnsi(rendered)).toMatch(/^apps\/\[2J-web\n/)
})
