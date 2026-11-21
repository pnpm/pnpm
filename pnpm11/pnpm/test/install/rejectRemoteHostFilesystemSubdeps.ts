import { execFileSync } from 'node:child_process'
import crypto from 'node:crypto'
import fs from 'node:fs'
import http from 'node:http'
import path from 'node:path'
import { pathToFileURL } from 'node:url'
import zlib from 'node:zlib'

import { expect, test } from '@jest/globals'
import { prepareEmpty } from '@pnpm/prepare'

import { execPnpm, spawnPnpm, waitForPnpmExit } from '../utils/index.js'

function createTarball (manifest: Record<string, unknown>): { bytes: Buffer, integrity: string } {
  const jsonBytes = Buffer.from(JSON.stringify(manifest), 'utf8')
  const indexBytes = Buffer.from("module.exports = 'remote-pkg'\n", 'utf8')
  const blocks = [
    tarBlock('package/package.json', jsonBytes),
    tarBlock('package/index.js', indexBytes),
    Buffer.alloc(1024, 0),
  ]
  const bytes = zlib.gzipSync(Buffer.concat(blocks))
  const integrity = `sha512-${crypto.createHash('sha512').update(bytes).digest('base64')}`
  return { bytes, integrity }
}

function tarBlock (name: string, content: Buffer): Buffer {
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

async function withTestServer (
  manifest: Record<string, unknown>,
  fn: (url: string) => void | Promise<void>
): Promise<void> {
  const { bytes, integrity } = createTarball(manifest)
  const server = http.createServer((req, res) => {
    const addr = server.address() as { port: number }
    if (req.url === '/remote-pkg') {
      res.writeHead(200, { 'content-type': 'application/json' })
      res.end(JSON.stringify({
        name: 'remote-pkg',
        'dist-tags': { latest: '1.0.0' },
        versions: {
          '1.0.0': {
            ...manifest,
            dist: {
              tarball: `http://127.0.0.1:${addr.port}/remote-pkg-1.0.0.tgz`,
              integrity,
            },
          },
        },
      }))
      return
    }
    if (req.url === '/remote-pkg-1.0.0.tgz') {
      res.writeHead(200, { 'content-type': 'application/octet-stream' })
      res.end(bytes)
      return
    }
    res.writeHead(404)
    res.end('not found')
  })
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve))
  try {
    const addr = server.address() as { port: number }
    await fn(`http://127.0.0.1:${addr.port}/`)
  } finally {
    server.closeAllConnections()
    await new Promise<void>((resolve) => server.close(() => resolve()))
  }
}

function setupHostSecrets (): { fakeHome: string } {
  const fakeHome = path.resolve('..', 'fake-home')
  fs.mkdirSync(path.join(fakeHome, '.config', 'gh'), { recursive: true })
  fs.writeFileSync(path.join(fakeHome, '.config', 'gh', 'hosts.yml'), 'oauth_token: gho_FAKE_TEST_SECRET\n')
  const siblingSecret = path.resolve('..', 'secret')
  fs.mkdirSync(siblingSecret, { recursive: true })
  fs.writeFileSync(path.join(siblingSecret, 'credentials.txt'), 'top_secret_value\n')
  return { fakeHome }
}

function assertNoLeakedFiles (dir: string): void {
  if (!fs.existsSync(dir)) return
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    expect(entry.name).not.toBe('hosts.yml')
    expect(entry.name).not.toBe('credentials.txt')
    const full = path.join(dir, entry.name)
    if (entry.isSymbolicLink()) {
      const target = fs.readlinkSync(full)
      expect(target).not.toContain('.config/gh')
      expect(target).not.toContain('secret')
    } else if (entry.isDirectory()) {
      assertNoLeakedFiles(full)
    }
  }
}

test('CLI install rejects registry and remote tarball packages declaring host filesystem subdependencies', async () => {
  for (const [alias, badSpec] of [
    ['host-gh', 'file:~/.config/gh'],
    ['host-etc', 'file:/etc'],
    ['host-parent', 'file:../secret'],
    ['host-link', 'link:~/.config/gh'],
  ] as const) {
    prepareEmpty()
    const { fakeHome } = setupHostSecrets()
    // eslint-disable-next-line no-await-in-loop -- isolated temp dirs per case
    await withTestServer(
      { name: 'remote-pkg', version: '1.0.0', dependencies: { [alias]: badSpec } },
      async (registryUrl) => {
        fs.writeFileSync('.npmrc', `registry=${registryUrl}\n`)
        fs.writeFileSync('pnpm-workspace.yaml', 'blockExoticSubdeps: true\nstrictDepBuilds: true\n')
        fs.writeFileSync('package.json', JSON.stringify({
          name: 'app',
          version: '1.0.0',
          dependencies: { 'remote-pkg': '1.0.0' },
        }))

        const { status, stdout, stderr } = await waitForPnpmExit(
          spawnPnpm(['install', `--registry=${registryUrl}`], {
            env: { HOME: fakeHome, USERPROFILE: fakeHome },
          })
        )
        expect(status).not.toBe(0)
        expect(stdout.toString() + stderr.toString()).toContain('ERR_PNPM_LOCAL_SUBDEP_NOT_ALLOWED')
        assertNoLeakedFiles(path.resolve('node_modules'))

        fs.writeFileSync('pnpm-workspace.yaml', 'blockExoticSubdeps: false\nstrictDepBuilds: true\n')
        fs.writeFileSync('package.json', JSON.stringify({
          name: 'app',
          version: '1.0.0',
          dependencies: { 'remote-pkg': `${registryUrl}remote-pkg-1.0.0.tgz` },
        }))
        const tarballRes = await waitForPnpmExit(
          spawnPnpm(['install', `--registry=${registryUrl}`], {
            env: { HOME: fakeHome, USERPROFILE: fakeHome },
          })
        )
        expect(tarballRes.status).not.toBe(0)
        expect(tarballRes.stdout.toString() + tarballRes.stderr.toString()).toContain('ERR_PNPM_LOCAL_SUBDEP_NOT_ALLOWED')
        assertNoLeakedFiles(path.resolve('node_modules'))
      }
    )
  }

  prepareEmpty()
  const { fakeHome } = setupHostSecrets()
  const gitRepoDir = path.resolve('..', 'git-remote')
  fs.mkdirSync(gitRepoDir, { recursive: true })
  fs.writeFileSync(path.join(gitRepoDir, 'package.json'), JSON.stringify({
    name: 'git-remote',
    version: '1.0.0',
    dependencies: { 'host-parent': 'file:../secret' },
  }))
  execFileSync('git', ['init', '-q', '-b', 'main'], { cwd: gitRepoDir })
  execFileSync('git', ['config', 'user.email', 'test@example.invalid'], { cwd: gitRepoDir })
  execFileSync('git', ['config', 'user.name', 'Test'], { cwd: gitRepoDir })
  execFileSync('git', ['add', '-A'], { cwd: gitRepoDir })
  execFileSync('git', ['-c', 'commit.gpgsign=false', 'commit', '-q', '-m', 'init'], { cwd: gitRepoDir })
  const commit = execFileSync('git', ['rev-parse', 'HEAD'], { cwd: gitRepoDir, encoding: 'utf8' }).trim()
  fs.writeFileSync('pnpm-workspace.yaml', 'blockExoticSubdeps: false\nstrictDepBuilds: true\n')
  fs.writeFileSync('package.json', JSON.stringify({
    name: 'app',
    version: '1.0.0',
    dependencies: { 'git-remote': `git+${pathToFileURL(gitRepoDir).href}#${commit}` },
  }))
  const gitRes = await waitForPnpmExit(
    spawnPnpm(['install'], {
      env: { HOME: fakeHome, USERPROFILE: fakeHome },
    })
  )
  expect(gitRes.status).not.toBe(0)
  expect(gitRes.stdout.toString() + gitRes.stderr.toString()).toContain('ERR_PNPM_LOCAL_SUBDEP_NOT_ALLOWED')
  assertNoLeakedFiles(path.resolve('node_modules'))
})

test('CLI rejects crafted lockfile with non-importer directory or local child on --frozen-lockfile and warm install', async () => {
  for (const [lockfileYaml, expectedCode] of [
    [
      "lockfileVersion: '9.0'\n\nimporters:\n  .:\n    dependencies:\n      remote-pkg:\n        specifier: 1.0.0\n        version: 1.0.0\n\npackages:\n  remote-pkg@1.0.0:\n    resolution: {type: directory, directory: ../secret}\n\nsnapshots:\n  remote-pkg@1.0.0: {}\n",
      'ERR_PNPM_RESOLUTION_SHAPE_MISMATCH',
    ],
    [
      "lockfileVersion: '9.0'\n\nimporters:\n  .:\n    dependencies:\n      remote-pkg:\n        specifier: 1.0.0\n        version: 1.0.0\n\npackages:\n  remote-pkg@1.0.0:\n    resolution: {integrity: sha512-deadbeef}\n  host-secret@file:../secret:\n    resolution: {type: directory, directory: ../secret}\n\nsnapshots:\n  remote-pkg@1.0.0:\n    dependencies:\n      host-secret: file:../secret\n  host-secret@file:../secret: {}\n",
      'ERR_PNPM_LOCAL_SUBDEP_NOT_ALLOWED',
    ],
  ] as const) {
    prepareEmpty()
    const { fakeHome } = setupHostSecrets()
    // eslint-disable-next-line no-await-in-loop -- isolated temp dirs per case
    await withTestServer({ name: 'remote-pkg', version: '1.0.0' }, async (registryUrl) => {
      fs.writeFileSync('.npmrc', `registry=${registryUrl}\n`)
      fs.writeFileSync('package.json', JSON.stringify({
        name: 'app',
        version: '1.0.0',
        dependencies: { 'remote-pkg': '1.0.0' },
      }))
      fs.writeFileSync('pnpm-lock.yaml', lockfileYaml)

      for (const args of [['install', '--frozen-lockfile', `--registry=${registryUrl}`], ['install', `--registry=${registryUrl}`]]) {
        // eslint-disable-next-line no-await-in-loop -- sequential CLI runs in same temp dir
        const { status, stdout, stderr } = await waitForPnpmExit(
          spawnPnpm(args, {
            env: { HOME: fakeHome, USERPROFILE: fakeHome },
          })
        )
        expect(status).not.toBe(0)
        expect(stdout.toString() + stderr.toString()).toContain(expectedCode)
        assertNoLeakedFiles(path.resolve('node_modules'))
      }
    })
  }
})

test('CLI preserves workspace overrides and linkWorkspacePackages: deep on install and --frozen-lockfile', async () => {
  prepareEmpty()
  fs.mkdirSync('local-helper', { recursive: true })
  fs.writeFileSync('local-helper/package.json', JSON.stringify({ name: 'helper', version: '1.0.0' }))
  fs.mkdirSync('packages/ws-pkg', { recursive: true })
  fs.writeFileSync('packages/ws-pkg/package.json', JSON.stringify({ name: 'ws-pkg', version: '1.0.0' }))

  await withTestServer(
    { name: 'remote-pkg', version: '1.0.0', dependencies: { helper: '^1.0.0', 'ws-pkg': '^1.0.0' } },
    async (registryUrl) => {
      fs.writeFileSync('.npmrc', `registry=${registryUrl}\n`)
      fs.writeFileSync(
        'pnpm-workspace.yaml',
        'packages:\n  - packages/*\nlinkWorkspacePackages: deep\noverrides:\n  remote-pkg>helper: file:./local-helper\n'
      )
      fs.writeFileSync('package.json', JSON.stringify({
        name: 'app',
        version: '1.0.0',
        dependencies: { 'remote-pkg': '1.0.0' },
      }))

      await execPnpm(['install', `--registry=${registryUrl}`])
      fs.rmSync('node_modules', { recursive: true, force: true })
      await execPnpm(['install', '--frozen-lockfile', `--registry=${registryUrl}`])
    }
  )
})

test('CLI preserves range-qualified overrides, spec-conditional readPackage hooks, fast add with overrides, and absent workspace projects', async () => {
  prepareEmpty()
  const outsideShared = path.resolve('..', 'shared-outside-hook')
  fs.mkdirSync(outsideShared, { recursive: true })
  fs.writeFileSync(path.join(outsideShared, 'package.json'), JSON.stringify({ name: 'shared', version: '1.0.0' }))
  fs.mkdirSync('local-helper', { recursive: true })
  fs.writeFileSync('local-helper/package.json', JSON.stringify({ name: 'helper', version: '1.0.0' }))

  await withTestServer(
    { name: 'remote-pkg', version: '1.0.0', dependencies: { helper: '^1.0.0', shared: '^1.0.0' } },
    async (registryUrl) => {
      // 1. Range-qualified override without a pnpmfile hook + incremental `pnpm add` against that lockfile.
      fs.writeFileSync('.npmrc', `registry=${registryUrl}\n`)
      fs.writeFileSync(
        'pnpm-workspace.yaml',
        'overrides:\n  \'remote-pkg>helper@^1.0.0\': file:./local-helper\n  \'remote-pkg>shared@^1.0.0\': file:./local-helper\n'
      )
      fs.writeFileSync('package.json', JSON.stringify({
        name: 'app',
        version: '1.0.0',
        dependencies: { 'remote-pkg': '1.0.0' },
      }))
      await execPnpm(['install', `--registry=${registryUrl}`])
      fs.rmSync('node_modules', { recursive: true, force: true })
      await execPnpm(['install', '--frozen-lockfile', `--registry=${registryUrl}`])
      fs.mkdirSync('extra-local', { recursive: true })
      fs.writeFileSync('extra-local/package.json', JSON.stringify({ name: 'extra-local', version: '1.0.0' }))
      await execPnpm(['add', 'file:./extra-local', `--registry=${registryUrl}`])

      // 2. Spec-conditional `.pnpmfile.cjs` `readPackage` hook replayed on `--frozen-lockfile`.
      fs.rmSync('pnpm-lock.yaml', { force: true })
      fs.rmSync('node_modules', { recursive: true, force: true })
      fs.writeFileSync('pnpm-workspace.yaml', 'overrides:\n  \'remote-pkg>helper@^1.0.0\': file:./local-helper\n')
      fs.writeFileSync(
        '.pnpmfile.cjs',
        `module.exports = { hooks: { readPackage (pkg) {
  if (pkg.name === 'remote-pkg' && pkg.dependencies && pkg.dependencies.shared === '^1.0.0') {
    pkg.dependencies.shared = 'link:../shared-outside-hook'
  }
  return pkg
} } }\n`
      )
      fs.writeFileSync('package.json', JSON.stringify({
        name: 'app',
        version: '1.0.0',
        dependencies: { 'remote-pkg': '1.0.0' },
      }))
      await execPnpm(['install', `--registry=${registryUrl}`])
      fs.rmSync('node_modules', { recursive: true, force: true })
      await execPnpm(['install', '--frozen-lockfile', `--registry=${registryUrl}`])

      // 3. Absent workspace project in Docker-like layout does not block `--frozen-lockfile`.
      fs.rmSync('.pnpmfile.cjs', { force: true })
      fs.rmSync('pnpm-lock.yaml', { force: true })
      fs.rmSync('node_modules', { recursive: true, force: true })
      fs.writeFileSync('pnpm-workspace.yaml', 'packages:\n  - packages/*\noverrides:\n  \'remote-pkg>helper@^1.0.0\': file:./local-helper\n  \'remote-pkg>shared@^1.0.0\': file:./local-helper\n')
      fs.writeFileSync('package.json', JSON.stringify({ name: 'root', private: true }))
      fs.mkdirSync('packages/app', { recursive: true })
      fs.writeFileSync('packages/app/package.json', JSON.stringify({
        name: 'app',
        version: '1.0.0',
        dependencies: { 'remote-pkg': '1.0.0' },
      }))
      fs.mkdirSync('packages/unused', { recursive: true })
      fs.writeFileSync('packages/unused/package.json', JSON.stringify({
        name: 'unused',
        version: '1.0.0',
        dependencies: { shared: 'file:../../../shared-outside-hook' },
      }))
      await execPnpm(['install', `--registry=${registryUrl}`])
      fs.rmSync('packages/unused', { recursive: true, force: true })
      fs.rmSync('node_modules', { recursive: true, force: true })
      await execPnpm(['install', '--frozen-lockfile', `--registry=${registryUrl}`])
    }
  )
})
