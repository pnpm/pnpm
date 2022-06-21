import { getNodeTarball } from '../lib/getNodeTarball'

test.each([
  [
    '16.0.0',
    'https://nodejs.org/download/release/',
    'win32',
    'ia32',
    {
      pkgName: 'node-v16.0.0-win-x86',
      tarball: 'https://nodejs.org/download/release/v16.0.0/node-v16.0.0-win-x86.zip',
    },
  ],
  [
    '16.0.0',
    'https://nodejs.org/download/release/',
    'linux',
    'arm',
    {
      pkgName: 'node-v16.0.0-linux-armv7l',
      tarball: 'https://nodejs.org/download/release/v16.0.0/node-v16.0.0-linux-armv7l.tar.gz',
    },
  ],
  [
    '16.0.0',
    'https://nodejs.org/download/release/',
    'linux',
    'x64',
    {
      pkgName: 'node-v16.0.0-linux-x64',
      tarball: 'https://nodejs.org/download/release/v16.0.0/node-v16.0.0-linux-x64.tar.gz',
    },
  ],
])('getNodeTarball', (version, nodeMirrorBaseUrl, platform, arch, tarball) => {
  expect(getNodeTarball(version, nodeMirrorBaseUrl, platform, arch)).toStrictEqual(tarball)
})
