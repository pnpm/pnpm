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

test('renderPeerIssues() ignore missing', () => {
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
            ],
            optional: false,
            wantedRange: '>=1.0.0 <3.0.0',
          },
        ],
        '@foo/bar': [
          {
            parents: [
              {
                name: 'xxx',
                version: '1.0.0',
              },
            ],
            optional: false,
            wantedRange: '>=1.0.0 <3.0.0',
          },
        ],
      },
      bad: {},
      conflicts: [],
      intersections: {
        aaa: '^1.0.0',
        '@foo/bar': '^1.0.0',
      },
    },
  }, {
    rules: {
      ignoreMissing: ['aaa', '@foo/*'],
    },
    width: 500,
  }))).toBe('')
})

test('renderPeerIssues() allow any version', () => {
  expect(stripAnsi(renderPeerIssues({
    '.': {
      missing: {},
      bad: {
        bbb: [
          {
            parents: [
              {
                name: 'xxx',
                version: '1.0.0',
              },
            ],
            foundVersion: '2.0.0',
            resolvedFrom: [],
            optional: false,
            wantedRange: '^1.0.0',
          },
        ],
        '@foo/bar': [
          {
            parents: [
              {
                name: 'xxx',
                version: '1.0.0',
              },
            ],
            foundVersion: '2.0.0',
            resolvedFrom: [],
            optional: false,
            wantedRange: '^1.0.0',
          },
        ],
      },
      conflicts: [],
      intersections: {},
    },
  }, {
    rules: {
      allowAny: ['bbb', '@foo/*'],
    },
    width: 500,
  }))).toBe('')
})

test('renderPeerIssues() allowed versions', () => {
  expect(stripAnsi(renderPeerIssues({
    '.': {
      missing: {},
      bad: {
        bbb: [
          {
            parents: [
              {
                name: 'xxx',
                version: '1.0.0',
              },
            ],
            foundVersion: '2.0.0',
            resolvedFrom: [],
            optional: false,
            wantedRange: '^1.0.0',
          },
        ],
        '@foo/bar': [
          {
            parents: [
              {
                name: 'aaa',
                version: '1.0.0',
              },
            ],
            foundVersion: '2.0.0',
            resolvedFrom: [],
            optional: false,
            wantedRange: '^1.0.0',
          },
          {
            parents: [
              {
                name: 'yyy',
                version: '1.0.0',
              },
              {
                name: 'xxx',
                version: '1.0.0',
              },
            ],
            foundVersion: '2.0.0',
            resolvedFrom: [],
            optional: false,
            wantedRange: '^1.0.0',
          },
          {
            parents: [
              {
                name: 'ccc',
                version: '3.0.0',
              },
            ],
            foundVersion: '3.0.0',
            resolvedFrom: [],
            optional: false,
            wantedRange: '^1.0.0',
          },
          {
            parents: [
              {
                name: 'ccc',
                version: '2.3.6',
              },
            ],
            foundVersion: '4.0.0',
            resolvedFrom: [],
            optional: false,
            wantedRange: '^1.0.0',
          },
        ],
      },
      conflicts: [],
      intersections: {},
    },
  }, {
    rules: {
      allowedVersions: {
        bbb: '2',
        'xxx>@foo/bar': '2',
        'ccc@3>@foo/bar': '3',
        'ccc@>=2.3.5 <3>@foo/bar': '4',
      },
    },
    width: 500,
  }))).toMatchSnapshot()
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
    rules: {
      ignoreMissing: [],
    },
    width: 500,
  })).trim()).toBe(`.
└─┬ <unknown> <unknown>
  └── ✕ missing peer foo@">=1.0.0 <3.0.0"
Peer dependencies that should be installed:
  foo@^1.0.0`)
})
