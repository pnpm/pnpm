import renderPeerIssues from '@pnpm/render-peer-issues'
import stripAnsi from 'strip-ansi'

test('renderPeerIssues()', () => {
  expect(stripAnsi(renderPeerIssues({
    missing: {
      aaa: [
        {
          location: {
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
            projectId: '.',
          },
          optional: false,
          wantedRange: '>=1.0.0 <3.0.0',
        },
      ],
      ddd: [
        {
          location: {
            parents: [
              {
                name: 'zzz',
                version: '1.0.0',
              },
            ],
            projectId: 'packages/0',
          },
          optional: false,
          wantedRange: '^1.0.0',
        },
      ],
    },
    bad: {
      bbb: [
        {
          location: {
            parents: [
              {
                name: 'xxx',
                version: '1.0.0',
              },
            ],
            projectId: '.',
          },
          foundVersion: '2',
          optional: false,
          wantedRange: '^1.0.0',
        },
      ],
      ccc: [
        {
          location: {
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
            projectId: '.',
          },
          foundVersion: '2',
          optional: false,
          wantedRange: '^1.0.0',
        },
      ],
    },
    missingMergedByProjects: {
      '.': {
        conflicts: [],
        intersections: { aaa: '^1.0.0' },
      },
      'packages/0': {
        conflicts: [],
        intersections: { ddd: '^1.0.0' },
      },
    },
  }))).toMatchSnapshot()
})

test('renderPeerIssues() optional peer dependencies are printed only if they are in conflict with non-optional peers', () => {
  expect(stripAnsi(renderPeerIssues({
    missing: {
      aaa: [
        {
          location: {
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
            projectId: '.',
          },
          optional: true,
          wantedRange: '^1.0.0',
        },
        {
          location: {
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
            projectId: '.',
          },
          optional: false,
          wantedRange: '^2.0.0',
        },
      ],
      bbb: [
        {
          location: {
            parents: [
              {
                name: 'xxx',
                version: '1.0.0',
              },
            ],
            projectId: '.',
          },
          optional: true,
          wantedRange: '^1.0.0',
        },
      ],
    },
    bad: {},
    missingMergedByProjects: {
      '.': {
        conflicts: ['aaa'],
        intersections: {},
      },
    },
  }))).toMatchSnapshot()
})
