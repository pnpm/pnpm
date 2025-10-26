import { filterPeerDependencyIssues } from '../src/install/reportPeerDependencyIssues.js'

test('filterPeerDependencyIssues() ignore missing', () => {
  expect(filterPeerDependencyIssues({
    '.': {
      missing: {
        aaa: [
          {
            parents: [
              {
                name: 'xxx',
                version: '1.0.0',
              }],

            optional: false,
            wantedRange: '>=1.0.0 <3.0.0',
          }],
        '@foo/bar': [
          {
            parents: [
              {
                name: 'xxx',
                version: '1.0.0',
              }],

            optional: false,
            wantedRange: '>=1.0.0 <3.0.0',
          }],

      },
      bad: {},
      conflicts: [],
      intersections: {
        aaa: '^1.0.0',
        '@foo/bar': '^1.0.0',
      },
    },
  }, {
    ignoreMissing: ['aaa', '@foo/*'],
  })).toStrictEqual({
    '.': {
      bad: {},
      conflicts: [],
      intersections: {
        '@foo/bar': '^1.0.0',
        aaa: '^1.0.0',
      },
      missing: {},
    },
  })
})

test('filterPeerDependencyIssues() allow any version', () => {
  expect(filterPeerDependencyIssues({
    '.': {
      missing: {},
      bad: {
        bbb: [
          {
            parents: [
              {
                name: 'xxx',
                version: '1.0.0',
              }],

            foundVersion: '2.0.0',
            resolvedFrom: [],
            optional: false,
            wantedRange: '^1.0.0',
          }],

        '@foo/bar': [
          {
            parents: [
              {
                name: 'xxx',
                version: '1.0.0',
              }],

            foundVersion: '2.0.0',
            resolvedFrom: [],
            optional: false,
            wantedRange: '^1.0.0',
          }],

      },
      conflicts: [],
      intersections: {},
    },
  }, {
    allowAny: ['bbb', '@foo/*'],
  })).toStrictEqual({
    '.': {
      bad: {},
      conflicts: [],
      intersections: {},
      missing: {},
    },
  })
})

test('filterPeerDependencyIssues() allowed versions', () => {
  expect(filterPeerDependencyIssues({
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
    allowedVersions: {
      bbb: '2',
      'xxx>@foo/bar': '2',
      'ccc@3>@foo/bar': '3',
      'ccc@>=2.3.5 <3>@foo/bar': '4',
    },
  })).toStrictEqual({
    '.': {
      bad: {
        '@foo/bar': [
          {
            foundVersion: '2.0.0',
            optional: false,
            parents: [
              {
                name: 'aaa',
                version: '1.0.0',
              },
            ],
            resolvedFrom: [],
            wantedRange: '^1.0.0',
          },
        ],
      },
      conflicts: [],
      intersections: {},
      missing: {},
    },
  })
})

test('filterPeerDependencyIssues() ignores missing optional dependency issues', () => {
  expect(filterPeerDependencyIssues({
    '.': {
      missing: {
        aaa: [
          {
            parents: [
              {
                name: 'xxx',
                version: '1.0.0',
              }],

            optional: true,
            wantedRange: '>=1.0.0 <3.0.0',
          },
        ],
      },
      bad: {},
      conflicts: [],
      intersections: {},
    },
  }, {
    allowAny: [],
  })).toStrictEqual({
    '.': {
      bad: {},
      conflicts: [],
      intersections: {},
      missing: {},
    },
  })
})
