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
            projectPath: '',
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
            projectPath: '/packages/0',
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
            projectPath: '',
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
            projectPath: '',
          },
          foundVersion: '2',
          wantedRange: '^1.0.0',
        },
      ],
    },
  }))).toMatchSnapshot()
})
