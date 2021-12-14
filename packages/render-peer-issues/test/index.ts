import renderPeerIssues from '@pnpm/render-peer-issues'
import stripAnsi from 'strip-ansi'

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
