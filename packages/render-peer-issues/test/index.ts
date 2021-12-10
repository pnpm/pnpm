import renderPeerIssues from '@pnpm/render-peer-issues'

test('renderPeerIssues()', () => {
  expect(renderPeerIssues({
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
          rootDir: '',
          peerRange: '^1.0.0',
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
          rootDir: '',
          peerRange: '^1.0.0',
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
          foundPeerVersion: '2',
          rootDir: '',
          peerRange: '^1.0.0',
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
          foundPeerVersion: '2',
          rootDir: '',
          peerRange: '^1.0.0',
        },
      ],
    },
  })).toMatchSnapshot()
})
