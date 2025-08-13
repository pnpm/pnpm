import { getNodeArtifactAddress } from '../lib/getNodeArtifactAddress'

test.each([
  [
    '16.0.0',
    'https://nodejs.org/download/release/',
    'win32',
    'ia32',
    {
      basename: 'node-v16.0.0-win-x86',
      dirname: 'https://nodejs.org/download/release/v16.0.0',
      extname: '.zip',
    },
  ],
  [
    '16.0.0',
    'https://nodejs.org/download/release/',
    'linux',
    'arm',
    {
      basename: 'node-v16.0.0-linux-armv7l',
      dirname: 'https://nodejs.org/download/release/v16.0.0',
      extname: '.tar.gz',
    },
  ],
  [
    '16.0.0',
    'https://nodejs.org/download/release/',
    'linux',
    'x64',
    {
      basename: 'node-v16.0.0-linux-x64',
      dirname: 'https://nodejs.org/download/release/v16.0.0',
      extname: '.tar.gz',
    },
  ],
  [
    '15.14.0',
    'https://nodejs.org/download/release/',
    'darwin',
    'arm64',
    {
      basename: 'node-v15.14.0-darwin-x64',
      dirname: 'https://nodejs.org/download/release/v15.14.0',
      extname: '.tar.gz',
    },
  ],
  [
    '16.0.0',
    'https://nodejs.org/download/release/',
    'darwin',
    'arm64',
    {
      basename: 'node-v16.0.0-darwin-arm64',
      dirname: 'https://nodejs.org/download/release/v16.0.0',
      extname: '.tar.gz',
    },
  ],
])('getNodeArtifactAddress', (version, nodeMirrorBaseUrl, platform, arch, tarball) => {
  expect(getNodeArtifactAddress({
    version,
    baseUrl: nodeMirrorBaseUrl,
    platform,
    arch,
  })).toStrictEqual(tarball)
})
