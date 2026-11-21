import crypto from 'node:crypto'
import fs from 'node:fs'
import http from 'node:http'
import path from 'node:path'
import url from 'node:url'
import zlib from 'node:zlib'

import { expect, test } from '@jest/globals'
import { LOCKFILE_VERSION } from '@pnpm/constants'
import { install, mutateModules } from '@pnpm/installing.deps-installer'
import type { LockfileObject } from '@pnpm/lockfile.fs'
import { prepareEmpty } from '@pnpm/prepare'
import type { BaseManifest, ProjectId, ProjectRootDir, ReadPackageHook } from '@pnpm/types'
import normalize from 'normalize-path'

import {
  computeWorkspaceDepth,
  createLocalResolutionPolicy,
} from '../../src/install/verifyLockfileLocalResolutions.js'
import { verifyLockfileResolutions } from '../../src/install/verifyLockfileResolutions.js'
import { commitLocalGitRepo } from '../utils/commitLocalGitRepo.js'
import { testDefaults } from '../utils/index.js'
import { serveGitRepoOverHttp } from '../utils/serveGitRepoOverHttp.js'

function createMinimalTarball (manifest: Record<string, unknown>): { bytes: Buffer, integrity: string } {
  const jsonBytes = Buffer.from(JSON.stringify(manifest), 'utf8')
  const indexBytes = Buffer.from("module.exports = 'remote-pkg'\n", 'utf8')
  const blocks = [
    tarEntry('package/package.json', jsonBytes),
    tarEntry('package/index.js', indexBytes),
    Buffer.alloc(1024, 0),
  ]
  const bytes = zlib.gzipSync(Buffer.concat(blocks))
  const integrity = `sha512-${crypto.createHash('sha512').update(bytes).digest('base64')}`
  return { bytes, integrity }
}

function tarEntry (name: string, content: Buffer): Buffer {
  const header = Buffer.alloc(512, 0)
  header.write(name, 0, Math.min(name.length, 100), 'utf8')
  header.write('0000644\0', 100, 8, 'ascii')
  header.write('0000000\0', 108, 8, 'ascii')
  header.write('0000000\0', 116, 8, 'ascii')
  header.write(`${content.length.toString(8).padStart(11, '0')}\0`, 124, 12, 'ascii')
  header.write(`${Math.floor(Date.now() / 1000).toString(8).padStart(11, '0')}\0`, 136, 12, 'ascii')
  header.fill(0x20, 148, 156)
  header[156] = '0'.charCodeAt(0)
  header.write('ustar\0', 257, 6, 'ascii')
  header.write('00', 263, 2, 'ascii')
  let sum = 0
  for (let idx = 0; idx < 512; idx++) sum += header[idx]
  header.write(`${sum.toString(8).padStart(6, '0')}\0 `, 148, 8, 'ascii')
  const padLen = (512 - (content.length % 512)) % 512
  return Buffer.concat([header, content, Buffer.alloc(padLen, 0)])
}

async function withMockRegistry (
  manifest: Record<string, unknown>,
  run: (registryUrl: string) => Promise<void>
): Promise<void> {
  await withMultiMockRegistry([manifest], run)
}

async function withMultiMockRegistry (
  manifests: Array<Record<string, unknown>>,
  run: (registryUrl: string) => Promise<void>
): Promise<void> {
  const entries = new Map<string, { manifest: Record<string, unknown>, bytes: Buffer, integrity: string }>()
  for (const manifest of manifests) {
    const name = String(manifest.name)
    const { bytes, integrity } = createMinimalTarball(manifest)
    entries.set(name, { manifest, bytes, integrity })
  }
  const server = http.createServer((req, res) => {
    const addr = server.address() as { port: number }
    const urlPath = (req.url ?? '').slice(1)
    if (urlPath.endsWith('-1.0.0.tgz')) {
      const pkgName = urlPath.slice(0, -'-1.0.0.tgz'.length)
      const hit = entries.get(pkgName)
      if (hit != null) {
        res.writeHead(200, { 'content-type': 'application/octet-stream' })
        res.end(hit.bytes)
        return
      }
    }
    const hit = entries.get(urlPath)
    if (hit != null) {
      res.writeHead(200, { 'content-type': 'application/json' })
      res.end(JSON.stringify({
        name: urlPath,
        'dist-tags': { latest: '1.0.0' },
        versions: {
          '1.0.0': {
            ...hit.manifest,
            dist: {
              tarball: `http://127.0.0.1:${addr.port}/${urlPath}-1.0.0.tgz`,
              integrity: hit.integrity,
            },
          },
        },
      }))
      return
    }
    res.writeHead(404)
    res.end('not found')
  })
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve))
  try {
    const addr = server.address() as { port: number }
    await run(`http://127.0.0.1:${addr.port}/`)
  } finally {
    server.closeAllConnections()
    await new Promise<void>((resolve) => server.close(() => resolve()))
  }
}

test('verifyLockfileResolutions rejects non-file depPaths backed by directory or file: tarball resolutions', async () => {
  for (const [depPath, resolution] of [
    ['remote-pkg@1.0.0', { type: 'directory', directory: '../../../../../etc' }],
    ['remote-pkg@1.0.0', { integrity: 'sha512-deadbeef', tarball: 'file:../../../../../etc/a.tgz' }],
    ['remote-pkg@https://example.com/a.tgz', { type: 'directory', directory: '../../../../../etc' }],
    ['remote-pkg@https://example.com/a.tgz', { integrity: 'sha512-deadbeef', tarball: 'file:../../../../../etc/a.tgz' }],
  ] as const) {
    const lockfile: LockfileObject = {
      lockfileVersion: LOCKFILE_VERSION,
      importers: {},
      packages: { [depPath]: { resolution } } as unknown as LockfileObject['packages'],
    }
    // eslint-disable-next-line no-await-in-loop -- keeps failures tied to the exact case
    await expect(verifyLockfileResolutions(lockfile, [])).rejects.toMatchObject({
      code: 'ERR_PNPM_RESOLUTION_SHAPE_MISMATCH',
    })
  }
})

test('verifyLockfileResolutions rejects unauthorized local children on non-local snapshots and forged lockfile overrides', async () => {
  const forgedLockfile: LockfileObject = {
    lockfileVersion: LOCKFILE_VERSION,
    overrides: { 'host-etc': 'file:../../../../../etc' },
    importers: {
      ['.' as ProjectId]: {
        specifiers: { 'remote-pkg': '1.0.0' },
        dependencies: { 'remote-pkg': '1.0.0' },
      },
    },
    packages: {
      'remote-pkg@1.0.0': {
        resolution: { integrity: 'sha512-deadbeef' },
        dependencies: { 'host-etc': 'file:../../../../../etc' },
      },
      'host-etc@file:../../../../../etc': {
        resolution: { type: 'directory', directory: '../../../../../etc' },
      },
    } as unknown as LockfileObject['packages'],
  }

  await expect(verifyLockfileResolutions(forgedLockfile, [], {
    localPolicy: createLocalResolutionPolicy({}),
  })).rejects.toMatchObject({
    code: 'ERR_PNPM_LOCAL_SUBDEP_NOT_ALLOWED',
  })

  const linkEscapeLockfile: LockfileObject = {
    lockfileVersion: LOCKFILE_VERSION,
    importers: {
      ['.' as ProjectId]: {
        specifiers: { 'remote-pkg': '1.0.0' },
        dependencies: { 'remote-pkg': '1.0.0' },
      },
    },
    packages: {
      'remote-pkg@1.0.0': {
        resolution: { integrity: 'sha512-deadbeef' },
        dependencies: { 'host-link': 'link:<root>/../../etc' },
      },
    } as unknown as LockfileObject['packages'],
  }
  await expect(verifyLockfileResolutions(linkEscapeLockfile, [], {
    localPolicy: createLocalResolutionPolicy({}),
  })).rejects.toMatchObject({
    code: 'ERR_PNPM_LOCAL_SUBDEP_NOT_ALLOWED',
  })
})

test('verifyLockfileResolutions allows package-root links, importer local deps, and active local overrides', async () => {
  const validLockfile: LockfileObject = {
    lockfileVersion: LOCKFILE_VERSION,
    importers: {
      ['.' as ProjectId]: {
        specifiers: { 'remote-pkg': '1.0.0', 'vendor-lib': 'file:../vendor/lib' },
        dependencies: { 'remote-pkg': '1.0.0', 'vendor-lib': 'file:../vendor/lib(react@18.0.0)' },
      },
      ['packages/shared' as ProjectId]: { specifiers: {} },
    },
    packages: {
      'remote-pkg@1.0.0': {
        resolution: { integrity: 'sha512-deadbeef' },
        dependencies: {
          internal: 'link:<root>/child',
          shared: 'link:packages/shared',
        },
      },
      'vendor-lib@file:../vendor/lib(react@18.0.0)': {
        resolution: { type: 'directory', directory: '../vendor/lib' },
        dependencies: { 'vendor-sub': 'file:../vendor/sub' },
      },
      'vendor-sub@file:../vendor/sub': {
        resolution: { type: 'directory', directory: '../vendor/sub' },
      },
    } as unknown as LockfileObject['packages'],
  }

  await expect(verifyLockfileResolutions(validLockfile, [], {
    localPolicy: createLocalResolutionPolicy({}),
  })).resolves.toBeUndefined()

  const overriddenLockfile: LockfileObject = {
    lockfileVersion: LOCKFILE_VERSION,
    importers: {
      ['.' as ProjectId]: {
        specifiers: { 'remote-pkg': '1.0.0' },
        dependencies: { 'remote-pkg': '1.0.0' },
      },
    },
    packages: {
      'remote-pkg@1.0.0': {
        resolution: { integrity: 'sha512-deadbeef' },
        dependencies: { helper: 'file:local-helper' },
      },
      'helper@file:local-helper': {
        resolution: { type: 'directory', directory: 'local-helper' },
      },
    } as unknown as LockfileObject['packages'],
  }

  await expect(verifyLockfileResolutions(overriddenLockfile, [], {
    localPolicy: createLocalResolutionPolicy({ overrides: { 'remote-pkg>helper': 'file:./local-helper' } }),
  })).resolves.toBeUndefined()

  await expect(verifyLockfileResolutions(overriddenLockfile, [], {
    localPolicy: createLocalResolutionPolicy({ overrides: { 'other-parent>helper': 'file:./local-helper' } }),
  })).rejects.toMatchObject({
    code: 'ERR_PNPM_LOCAL_SUBDEP_NOT_ALLOWED',
  })

  const secretAbsDir = path.resolve('/tmp/secret')
  const secretFileUrl = url.pathToFileURL(secretAbsDir).href

  for (const [packages, expectedCode] of [
    [
      {
        'remote-pkg@1.0.0': {
          resolution: { integrity: 'sha512-deadbeef' },
          dependencies: { root: 'link:/' },
        },
      },
      'ERR_PNPM_LOCAL_SUBDEP_NOT_ALLOWED',
    ],
    [
      {
        'remote-pkg@1.0.0': {
          resolution: { integrity: 'sha512-deadbeef' },
          dependencies: { secret: secretFileUrl },
        },
        [`secret@${secretFileUrl}`]: {
          resolution: { type: 'directory', directory: normalize(secretAbsDir) },
        },
      },
      'ERR_PNPM_LOCAL_SUBDEP_NOT_ALLOWED',
    ],
    [
      {
        'vendor-lib@file:../vendor/lib': {
          resolution: { type: 'directory', directory: '/etc' },
        },
      },
      'ERR_PNPM_RESOLUTION_SHAPE_MISMATCH',
    ],
  ] as const) {
    const crafted: LockfileObject = {
      lockfileVersion: LOCKFILE_VERSION,
      pnpmfileChecksum: 'sha256-hook',
      importers: {
        ['.' as ProjectId]: {
          specifiers: { 'vendor-lib': 'file:../vendor/lib' },
          dependencies: { 'vendor-lib': 'file:../vendor/lib' },
        },
      },
      packages: packages as unknown as LockfileObject['packages'],
    }
    // eslint-disable-next-line no-await-in-loop -- keeps failures tied to the exact case
    await expect(verifyLockfileResolutions(crafted, [], {
      localPolicy: createLocalResolutionPolicy({
        hasPnpmfileHook: true,
        overrides: { 'ws-pkg': 'workspace:*', 'trusted>helper': 'file:./local-helper' },
      }),
    })).rejects.toMatchObject({ code: expectedCode })
  }
})

test('install rejects non-local packages declaring file:, link:, workspace-path, or hoisted file: peer subdependencies', async () => {
  for (const [field, alias, spec] of [
    ['dependencies', 'host-gh', 'file:~/.config/gh'],
    ['dependencies', 'host-etc', 'file:/etc'],
    ['dependencies', 'host-parent', 'file:../secret'],
    ['dependencies', 'host-link', 'link:~/.config/gh'],
    ['dependencies', 'host-ws', 'workspace:../secret'],
    ['dependencies', 'host-git-file', 'git+file:///tmp/repo'],
    ['optionalDependencies', 'host-opt', 'file:../secret'],
    ['peerDependencies', 'host-peer', 'file:../secret'],
  ] as const) {
    prepareEmpty()
    const secretDir = path.resolve('..', 'secret')
    fs.mkdirSync(secretDir, { recursive: true })
    fs.writeFileSync(path.join(secretDir, 'secret.txt'), 'secret\n')

    // eslint-disable-next-line no-await-in-loop -- isolated temp dirs per case
    await withMockRegistry(
      { name: 'remote-pkg', version: '1.0.0', [field]: { [alias]: spec } },
      async (registryUrl) => {
        await expect(install(
          { name: 'app', version: '1.0.0', dependencies: { 'remote-pkg': '1.0.0' } },
          testDefaults({
            autoInstallPeers: true,
            blockExoticSubdeps: true,
            overrides: { 'other-parent>host-parent': 'file:../secret' },
            registriesByScope: { default: registryUrl },
          })
        )).rejects.toMatchObject({
          code: 'ERR_PNPM_LOCAL_SUBDEP_NOT_ALLOWED',
        })
      }
    )
  }

  prepareEmpty()
  const secretDir = path.resolve('..', 'secret')
  fs.mkdirSync(secretDir, { recursive: true })
  fs.writeFileSync(path.join(secretDir, 'secret.txt'), 'secret\n')
  const gitRepoDir = path.resolve('..', 'git-remote')
  fs.mkdirSync(gitRepoDir, { recursive: true })
  fs.writeFileSync(path.join(gitRepoDir, 'package.json'), JSON.stringify({
    name: 'git-remote',
    version: '1.0.0',
    dependencies: { 'host-parent': 'file:../secret' },
  }))
  const localGitSpec = await commitLocalGitRepo(gitRepoDir)
  await expect(install(
    { name: 'app', version: '1.0.0', dependencies: { 'git-remote': localGitSpec } },
    testDefaults({ blockExoticSubdeps: false })
  )).rejects.toMatchObject({
    code: 'ERR_PNPM_LOCAL_SUBDEP_NOT_ALLOWED',
  })
  const served = await serveGitRepoOverHttp(gitRepoDir, localGitSpec.slice(localGitSpec.lastIndexOf('#') + 1))
  try {
    await expect(install(
      { name: 'app', version: '1.0.0', dependencies: { 'git-remote': served.gitSpec } },
      testDefaults({ blockExoticSubdeps: false })
    )).rejects.toMatchObject({
      code: 'ERR_PNPM_LOCAL_SUBDEP_NOT_ALLOWED',
    })
  } finally {
    await served.close()
  }
})

test('install preserves workspace overrides, packageExtensions, readPackage hooks, and deep workspace links', async () => {
  prepareEmpty()
  const helperDir = path.resolve('local-helper')
  fs.mkdirSync(helperDir, { recursive: true })
  fs.writeFileSync(path.join(helperDir, 'package.json'), JSON.stringify({ name: 'helper', version: '1.0.0' }))

  const wsPkgDir = path.resolve('packages/ws-pkg')
  fs.mkdirSync(wsPkgDir, { recursive: true })
  const wsManifest = { name: 'ws-pkg', version: '1.0.0' }
  fs.writeFileSync(path.join(wsPkgDir, 'package.json'), JSON.stringify(wsManifest))

  const workspacePackages = new Map([
    ['ws-pkg', new Map([['1.0.0', { rootDir: wsPkgDir as ProjectRootDir, manifest: wsManifest }]])],
  ])

  await withMockRegistry(
    { name: 'remote-pkg', version: '1.0.0', dependencies: { helper: '^1.0.0', 'ws-pkg': '^1.0.0' } },
    async (registryUrl) => {
      await install(
        { name: 'app', version: '1.0.0', dependencies: { 'remote-pkg': '1.0.0' } },
        testDefaults({
          linkWorkspacePackagesDepth: Infinity,
          overrides: { 'remote-pkg>helper': 'file:./local-helper' },
          registriesByScope: { default: registryUrl },
          workspacePackages,
        })
      )
    }
  )
})

test('verifyLockfileResolutions authorizes override closure in both package-map orders and enforces ranged, mixed-variation, and live-root rules', async () => {
  const helperEntry = {
    resolution: { type: 'directory', directory: 'local-helper' },
    dependencies: { nested: 'file:../outside-nested', ext: 'link:../outside-link' },
  }
  const nestedEntry = { resolution: { type: 'directory', directory: '../outside-nested' } }
  const remoteEntry = { resolution: { integrity: 'sha512-deadbeef' }, dependencies: { helper: 'file:local-helper' } }

  for (const packages of [
    { 'helper@file:local-helper': helperEntry, 'nested@file:../outside-nested': nestedEntry, 'remote-pkg@1.0.0': remoteEntry },
    { 'remote-pkg@1.0.0': remoteEntry, 'helper@file:local-helper': helperEntry, 'nested@file:../outside-nested': nestedEntry },
  ]) {
    const lockfile: LockfileObject = {
      lockfileVersion: LOCKFILE_VERSION,
      importers: { ['.' as ProjectId]: { specifiers: { 'remote-pkg': '1.0.0' }, dependencies: { 'remote-pkg': '1.0.0' } } },
      packages: packages as unknown as LockfileObject['packages'],
    }
    // eslint-disable-next-line no-await-in-loop -- keeps failures tied to the exact package-map order
    await expect(verifyLockfileResolutions(lockfile, [], {
      localPolicy: createLocalResolutionPolicy({ overrides: { 'remote-pkg>helper': 'file:./local-helper' } }),
    })).resolves.toBeUndefined()
    // eslint-disable-next-line no-await-in-loop -- keeps failures tied to the exact package-map order
    await expect(verifyLockfileResolutions(lockfile, [], {
      localPolicy: createLocalResolutionPolicy({ overrides: { 'other-parent>helper': 'file:./local-helper' } }),
    })).rejects.toMatchObject({ code: 'ERR_PNPM_LOCAL_SUBDEP_NOT_ALLOWED' })
  }

  expect(computeWorkspaceDepth('/work/..build', '/work')).toBe(1)
  expect(computeWorkspaceDepth('/external', '/work')).toBeNull()

  const widgetLockfile: LockfileObject = {
    lockfileVersion: LOCKFILE_VERSION,
    importers: { ['.' as ProjectId]: { specifiers: {}, dependencies: {} } },
    packages: {
      'remote@2.0.0': {
        version: '1.0.0',
        resolution: { integrity: 'sha512-deadbeef' },
        dependencies: { widget: 'file:helper' },
      },
      'widget@file:helper': { resolution: { type: 'directory', directory: 'helper' } },
    } as unknown as LockfileObject['packages'],
  }
  await expect(verifyLockfileResolutions(widgetLockfile, [], {
    localPolicy: createLocalResolutionPolicy({ overrides: { 'remote@^1>widget': 'file:./helper' } }),
  })).rejects.toMatchObject({ code: 'ERR_PNPM_LOCAL_SUBDEP_NOT_ALLOWED' })
  await expect(verifyLockfileResolutions(widgetLockfile, [], {
    localPolicy: createLocalResolutionPolicy({ lockfileDir: 'C:/work/app', overrides: { 'widget@^1': 'file:C:/work/app/helper' } }),
  })).rejects.toMatchObject({ code: 'ERR_PNPM_LOCAL_SUBDEP_NOT_ALLOWED' })
  await expect(verifyLockfileResolutions(widgetLockfile, [], {
    localPolicy: createLocalResolutionPolicy({ lockfileDir: 'C:/work/app', overrides: { widget: 'file:C:/work/app/helper' } }),
  })).resolves.toBeUndefined()

  const mixedVarLockfile: LockfileObject = {
    lockfileVersion: LOCKFILE_VERSION,
    importers: { ['.' as ProjectId]: { specifiers: { mixed: 'file:./local' }, dependencies: { mixed: 'file:./local' } } },
    packages: {
      'mixed@file:./local': {
        resolution: {
          type: 'variations',
          variants: [
            { targets: [{ os: 'linux' }], resolution: { type: 'directory', directory: './local' } },
            { targets: [{ os: 'darwin' }], resolution: { tarball: 'https://evil.example/mixed.tgz' } },
          ],
        },
        dependencies: { leak: 'link:../secret' },
      },
    } as unknown as LockfileObject['packages'],
  }
  await expect(verifyLockfileResolutions(mixedVarLockfile, [], {
    localPolicy: createLocalResolutionPolicy({}),
  })).rejects.toMatchObject({ code: 'ERR_PNPM_RESOLUTION_SHAPE_MISMATCH' })

  const forgedImporterLockfile: LockfileObject = {
    lockfileVersion: LOCKFILE_VERSION,
    importers: {
      ['.' as ProjectId]: { specifiers: {}, dependencies: {} },
      ['packages/forged' as ProjectId]: { specifiers: {}, dependencies: {} },
    },
    packages: {
      'remote@1.0.0': { resolution: { integrity: 'sha512-deadbeef' }, dependencies: { forged: 'link:packages/forged' } },
    } as unknown as LockfileObject['packages'],
  }
  await expect(verifyLockfileResolutions(forgedImporterLockfile, [], {
    localPolicy: createLocalResolutionPolicy({ liveWorkspaceDirs: new Set(['.']) }),
  })).rejects.toMatchObject({ code: 'ERR_PNPM_LOCAL_SUBDEP_NOT_ALLOWED' })
})

test('readPackage hook rewriting a remote dependency outside the workspace succeeds on warm and frozen installs while identity hook rejects forgery', async () => {
  prepareEmpty()
  const sharedDir = path.resolve('..', 'shared-outside')
  fs.mkdirSync(sharedDir, { recursive: true })
  fs.writeFileSync(path.join(sharedDir, 'package.json'), JSON.stringify({ name: 'shared', version: '1.0.0' }))

  const rewriteHook: ReadPackageHook = <Pkg extends BaseManifest>(pkg: Pkg): Pkg => {
    if (pkg.name === 'remote-pkg') {
      pkg.dependencies = { ...pkg.dependencies, shared: 'file:../shared-outside' }
    }
    return pkg
  }
  const identityHook: ReadPackageHook = <Pkg extends BaseManifest>(pkg: Pkg): Pkg => pkg

  await withMockRegistry(
    { name: 'remote-pkg', version: '1.0.0', dependencies: { shared: '^1.0.0' } },
    async (registryUrl) => {
      const manifest = { name: 'app', version: '1.0.0', dependencies: { 'remote-pkg': '1.0.0' } }
      await install(manifest, testDefaults({
        hooks: { readPackage: [rewriteHook], untrackedPnpmfileReadPackageHook: true },
        registriesByScope: { default: registryUrl },
      }))
      await install(manifest, testDefaults({
        frozenLockfile: true,
        hooks: { readPackage: [rewriteHook], untrackedPnpmfileReadPackageHook: true },
        registriesByScope: { default: registryUrl },
      }))
      await expect(install(manifest, testDefaults({
        frozenLockfile: true,
        hooks: { readPackage: [identityHook], untrackedPnpmfileReadPackageHook: true },
        registriesByScope: { default: registryUrl },
      }))).rejects.toMatchObject({ code: 'ERR_PNPM_LOCAL_SUBDEP_NOT_ALLOWED' })
    }
  )
})

test('hoisted local peer authorizations are scoped to the declaring parent and importer', async () => {
  prepareEmpty()
  const peerDir = path.resolve('..', 'shared-peer')
  fs.mkdirSync(peerDir, { recursive: true })
  fs.writeFileSync(path.join(peerDir, 'package.json'), JSON.stringify({ name: 'peer-x', version: '1.0.0' }))
  const peerSpec = `file:${normalize(peerDir)}`

  await withMultiMockRegistry(
    [
      { name: 'trusted-parent', version: '1.0.0' },
      { name: 'evil-parent', version: '1.0.0', peerDependencies: { 'peer-x': peerSpec } },
    ],
    async (registryUrl) => {
      await expect(install(
        { name: 'app', version: '1.0.0', dependencies: { 'trusted-parent': '1.0.0', 'evil-parent': '1.0.0' } },
        testDefaults({
          autoInstallPeers: true,
          packageExtensions: { 'trusted-parent': { peerDependencies: { 'peer-x': peerSpec } } },
          registriesByScope: { default: registryUrl },
        })
      )).rejects.toMatchObject({ code: 'ERR_PNPM_LOCAL_SUBDEP_NOT_ALLOWED' })
    }
  )

  prepareEmpty()
  const wsPeerDir = path.resolve('..', 'ws-shared-peer')
  fs.mkdirSync(wsPeerDir, { recursive: true })
  fs.writeFileSync(path.join(wsPeerDir, 'package.json'), JSON.stringify({ name: 'peer-x', version: '1.0.0' }))
  const wsPeerSpec = `file:${normalize(wsPeerDir)}`
  const localLibDir = path.resolve('local-lib')
  fs.mkdirSync(localLibDir, { recursive: true })
  fs.writeFileSync(path.join(localLibDir, 'package.json'), JSON.stringify({
    name: 'local-lib',
    version: '1.0.0',
    peerDependencies: { 'peer-x': wsPeerSpec },
  }))
  const impADir = path.resolve('packages/imp-a') as ProjectRootDir
  const impBDir = path.resolve('packages/imp-b') as ProjectRootDir
  fs.mkdirSync(impADir, { recursive: true })
  fs.mkdirSync(impBDir, { recursive: true })

  await withMultiMockRegistry(
    [{ name: 'evil-b', version: '1.0.0', peerDependencies: { 'peer-x': wsPeerSpec } }],
    async (registryUrl) => {
      await expect(mutateModules(
        [
          {
            mutation: 'install',
            rootDir: impADir,
          },
          {
            mutation: 'install',
            rootDir: impBDir,
          },
        ],
        testDefaults({
          allProjects: [
            {
              buildIndex: 0,
              manifest: { name: 'imp-a', version: '1.0.0', dependencies: { 'local-lib': 'file:../../local-lib' } },
              rootDir: impADir,
            },
            {
              buildIndex: 1,
              manifest: { name: 'imp-b', version: '1.0.0', dependencies: { 'evil-b': '1.0.0' } },
              rootDir: impBDir,
            },
          ],
          autoInstallPeers: true,
          registriesByScope: { default: registryUrl },
        })
      )).rejects.toMatchObject({ code: 'ERR_PNPM_LOCAL_SUBDEP_NOT_ALLOWED' })
    }
  )
})

test('a hook-authorized local peer of a remote Git parent reinstalls with a lockfile', async () => {
  prepareEmpty()
  for (const [dir, name] of [['helper-outside', 'helper'], ['other-outside', 'other']]) {
    fs.mkdirSync(path.resolve('..', dir), { recursive: true })
    fs.writeFileSync(path.resolve('..', dir, 'package.json'), JSON.stringify({ name, version: '1.0.0' }))
  }
  const gitRepoDir = path.resolve('..', 'git-remote')
  fs.mkdirSync(gitRepoDir, { recursive: true })
  fs.writeFileSync(path.join(gitRepoDir, 'package.json'), JSON.stringify({
    name: 'git-remote',
    version: '1.0.0',
    peerDependencies: { helper: '^1.0.0' },
  }))
  const localGitSpec = await commitLocalGitRepo(gitRepoDir)
  const served = await serveGitRepoOverHttp(gitRepoDir, localGitSpec.slice(localGitSpec.lastIndexOf('#') + 1))
  const readPackage: ReadPackageHook = (pkg) => {
    if (pkg.name === 'git-remote' && pkg.peerDependencies != null) {
      pkg.peerDependencies.helper = 'link:../helper-outside'
    }
    return pkg
  }
  try {
    const dependencies = { 'git-remote': served.gitSpec }
    await install(
      { name: 'app', version: '1.0.0', dependencies },
      testDefaults({ autoInstallPeers: true, hooks: { readPackage: [readPackage] } })
    )
    await install(
      { name: 'app', version: '1.0.0', dependencies: { ...dependencies, other: 'link:../other-outside' } },
      testDefaults({ autoInstallPeers: true, hooks: { readPackage: [readPackage] } })
    )
  } finally {
    await served.close()
  }
})
