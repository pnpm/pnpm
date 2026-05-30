import { stripVTControlCharacters as stripAnsi } from 'node:util'

import { expect, test } from '@jest/globals'
import { renderPeerIssues } from '@pnpm/deps.inspection.peers-issues-renderer'

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
