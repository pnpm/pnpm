import path from 'path'
import { type DepPath } from '@pnpm/types'
import { type InjectedPackageInfo, findInjectedPackages } from '../src/findInjectedPackages'

test('findInjectedPackages', () => {
  const items = [...findInjectedPackages({
    lockfile: {
      packages: {
        ['foo@file:packages/foo' as DepPath]: {
          resolution: {
            type: 'directory',
            directory: 'packages/foo',
          },
        },
        ['foo@file:packages/foo(peer1@0.1.2)(peer2@2.1.0)' as DepPath]: {
          resolution: {
            type: 'directory',
            directory: 'packages/foo',
          },
        },
        ['foo@1.2.3' as DepPath]: {
          resolution: {
            integrity: '00000000',
          },
        },
        ['bar@file:packages/bar' as DepPath]: {
          resolution: {
            type: 'directory',
            directory: 'packages/bar',
          },
        },
      },
    },
    pkgName: 'foo',
    pkgRootDir: path.resolve('packages/foo'),
    virtualStoreDir: path.resolve('node_modules/.pnpm'),
    virtualStoreDirMaxLength: 120,
    workspaceDir: process.cwd(),
  })]
  expect(items).toStrictEqual([
    {
      depPath: 'foo@file:packages/foo' as DepPath,
      parsedDepPath: {
        name: 'foo',
        nonSemverVersion: 'file:packages/foo',
        patchHash: undefined,
        peersSuffix: undefined,
      },
      resolution: {
        directory: 'packages/foo',
        type: 'directory',
      },
      sourceDir: path.resolve('packages/foo'),
      targetDir: path.resolve('node_modules/.pnpm/foo@file+packages+foo/node_modules/foo'),
    },
    {
      depPath: 'foo@file:packages/foo(peer1@0.1.2)(peer2@2.1.0)' as DepPath,
      parsedDepPath: {
        name: 'foo',
        nonSemverVersion: 'file:packages/foo',
        patchHash: undefined,
        peersSuffix: '(peer1@0.1.2)(peer2@2.1.0)',
      },
      resolution: {
        directory: 'packages/foo',
        type: 'directory',
      },
      sourceDir: path.resolve('packages/foo'),
      targetDir: path.resolve('node_modules/.pnpm/foo@file+packages+foo_peer1@0.1.2_peer2@2.1.0/node_modules/foo'),
    },
  ] as InjectedPackageInfo[])
})
