import { renderPeerIssues } from '@pnpm/render-peer-issues'
import { stripVTControlCharacters as stripAnsi } from 'util'

test('renderPeerIssues()', () => {
  expect(stripAnsi(renderPeerIssues({
    'packages/0': {
      conflicts: ['eee'],
      intersections: { ddd: '^1.0.0' },
      bad: {},
      missing: {
        ddd: [
          {
            parents: [
              {
                name: 'zzz',
                version: '1.0.0',
              },
            ],
            optional: false,
            wantedRange: '^1.0.0',
          },
        ],
        eee: [
          {
            parents: [
              {
                name: 'zzz',
                version: '1.0.0',
              },
            ],
            optional: false,
            wantedRange: '^1.0.0',
          },
          {
            parents: [
              {
                name: 'www',
                version: '1.0.0',
              },
            ],
            optional: false,
            wantedRange: '^2.0.0',
          },
        ],
      },
    },
    '.': {
      missing: {
        aaa: [
          {
            parents: [
              {
                name: 'xxx',
                version: '1.0.0',
              },
              {
                name: 'yyy',
                version: '1.0.0',
              },
            ],
            optional: false,
            wantedRange: '>=1.0.0 <3.0.0',
          },
        ],
      },
      bad: {
        bbb: [
          {
            parents: [
              {
                name: 'xxx',
                version: '1.0.0',
              },
            ],
            foundVersion: '2',
            resolvedFrom: [],
            optional: false,
            wantedRange: '^1.0.0',
          },
        ],
        ccc: [
          {
            parents: [
              {
                name: 'xxx',
                version: '1.0.0',
              },
              {
                name: 'yyy',
                version: '1.0.0',
              },
            ],
            foundVersion: '2',
            resolvedFrom: [
              {
                name: 'xxx',
                version: '1.0.0',
              },
            ],
            optional: false,
            wantedRange: '^1.0.0',
          },
        ],
      },
      conflicts: [],
      intersections: { aaa: '^1.0.0' },
    },
  }, { width: 500 }))).toMatchSnapshot()
})

test('renderPeerIssues() optional peer dependencies are printed only if they are in conflict with non-optional peers', () => {
  expect(stripAnsi(renderPeerIssues({
    '.': {
      missing: {
        aaa: [
          {
            parents: [
              {
                name: 'xxx',
                version: '1.0.0',
              },
              {
                name: 'yyy',
                version: '1.0.0',
              },
            ],
            optional: true,
            wantedRange: '^1.0.0',
          },
          {
            parents: [
              {
                name: 'xxx',
                version: '1.0.0',
              },
              {
                name: 'yyy',
                version: '1.0.0',
              },
            ],
            optional: false,
            wantedRange: '^2.0.0',
          },
        ],
        bbb: [
          {
            parents: [
              {
                name: 'xxx',
                version: '1.0.0',
              },
            ],
            optional: true,
            wantedRange: '^1.0.0',
          },
        ],
      },
      bad: {},
      conflicts: ['aaa'],
      intersections: {},
    },
    empty: {
      missing: {},
      bad: {},
      conflicts: [],
      intersections: {},
    },
  }, { width: 500 }))).toMatchSnapshot()
})

test('renderPeerIssues() format correctly the version ranges with spaces and "*"', () => {
  expect(stripAnsi(renderPeerIssues({
    '.': {
      conflicts: [],
      intersections: { a: '*', b: '1 || 2' },
      bad: {},
      missing: {
        a: [
          {
            parents: [
              {
                name: 'z',
                version: '1.0.0',
              },
            ],
            optional: false,
            wantedRange: '*',
          },
        ],
        b: [
          {
            parents: [
              {
                name: 'z',
                version: '1.0.0',
              },
            ],
            optional: false,
            wantedRange: '1 || 2',
          },
        ],
      },
    },
  }, { width: 500 }))).toMatchSnapshot()
})

test('renderPeerIssues() do not fail if the parents array is empty', () => {
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
      intersections: {
        foo: '^1.0.0',
      },
    },
  }, {
    width: 500,
  })).trim()).toBe(`.
└─┬ <unknown> <unknown>
  └── ✕ missing peer foo@">=1.0.0 <3.0.0"
Peer dependencies that should be installed:
  foo@^1.0.0`)
})
