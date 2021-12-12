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
          wantedRange: '^1.0.0',
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
          wantedRange: '^1.0.0',
        },
      ],
    },
    missingMergedByProjects: {
      '.': {
        conflicts: [],
        intersections: [],
      },
      'packages/0': {
        conflicts: [],
        intersections: [],
      },
    },
  }))).toMatchSnapshot()
})
