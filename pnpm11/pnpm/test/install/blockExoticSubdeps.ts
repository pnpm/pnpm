import { execFileSync } from 'node:child_process'
import crypto from 'node:crypto'
import fs from 'node:fs'
import http from 'node:http'
import type { AddressInfo } from 'node:net'
import path from 'node:path'

import { describe, expect, test } from '@jest/globals'
import { prepareEmpty } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/testing.registry-mock'
import { writeJsonFileSync } from 'write-json-file'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpmSync, spawnPnpm, waitForPnpmExit } from '../utils/index.js'

interface TestRegistryRoute {
  contentType: string
  body: Buffer
}

interface TestRegistryServer {
  baseUrl: string
  getExoticHits: () => number
  resetExoticHits: () => void
  close: () => Promise<void>
}

function packDirectoryToTarball (pkgDir: string): Buffer {
  execPnpmSync(['pack'], { cwd: pkgDir, expectSuccess: true })
  const tgzFile = fs.readdirSync(pkgDir).find((entry) => entry.endsWith('.tgz'))
  if (!tgzFile) {
    throw new Error(`Expected pnpm pack to produce a .tgz in ${pkgDir}`)
  }
  return fs.readFileSync(path.join(pkgDir, tgzFile))
}

function createTarballFixture (
  scratchRoot: string,
  slug: string,
  manifest: Record<string, unknown>,
  indexJs: string = 'module.exports = "ok"\n'
): Buffer {
  const pkgDir = path.join(scratchRoot, slug)
  fs.mkdirSync(pkgDir, { recursive: true })
  writeJsonFileSync(path.join(pkgDir, 'package.json'), manifest)
  fs.writeFileSync(path.join(pkgDir, 'index.js'), indexJs)
  return packDirectoryToTarball(pkgDir)
}

function sha512Integrity (bytes: Buffer): string {
  return `sha512-${crypto.createHash('sha512').update(bytes).digest('base64')}`
}

async function startTestServer (
  scratchRoot: string,
  options: {
    exoticTgz?: Buffer
    packages?: Array<{ name: string, version: string, dependencies?: Record<string, string> }>
  }
): Promise<TestRegistryServer> {
  const routes = new Map<string, TestRegistryRoute>()
  let exoticHits = 0

  const server = http.createServer((req, res) => {
    const reqPath = (req.url ?? '/').split('?')[0]
    if (reqPath === '/exotic.tgz') {
      exoticHits += 1
    }
    const route = routes.get(reqPath)
    if (!route) {
      res.writeHead(404, { 'Content-Type': 'text/plain', Connection: 'close' })
      res.end('not found')
      return
    }
    res.writeHead(200, {
      'Content-Type': route.contentType,
      'Content-Length': String(route.body.length),
      Connection: 'close',
    })
    res.end(route.body)
  })

  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve))
  const address = server.address()
  if (!address || typeof address === 'string') {
    server.close()
    throw new Error('Expected numeric server port')
  }
  const baseUrl = `http://127.0.0.1:${address.port}`

  if (options.exoticTgz) {
    routes.set('/exotic.tgz', {
      contentType: 'application/octet-stream',
      body: options.exoticTgz,
    })
  }

  for (const pkg of options.packages ?? []) {
    const tgz = createTarballFixture(scratchRoot, `${pkg.name}-${pkg.version}`, {
      name: pkg.name,
      version: pkg.version,
      main: 'index.js',
      dependencies: pkg.dependencies ?? {},
    })
    const tgzPath = `/${pkg.name}/-/${pkg.name}-${pkg.version}.tgz`
    const packument = JSON.stringify({
      name: pkg.name,
      'dist-tags': { latest: pkg.version },
      versions: {
        [pkg.version]: {
          name: pkg.name,
          version: pkg.version,
          dependencies: pkg.dependencies ?? {},
          dist: {
            tarball: `${baseUrl}${tgzPath}`,
            integrity: sha512Integrity(tgz),
          },
        },
      },
    })
    routes.set(`/${pkg.name}`, {
      contentType: 'application/json',
      body: Buffer.from(packument),
    })
    routes.set(tgzPath, {
      contentType: 'application/octet-stream',
      body: tgz,
    })
  }

  return {
    baseUrl,
    getExoticHits: () => exoticHits,
    resetExoticHits: () => {
      exoticHits = 0
    },
    close: async () => {
      server.closeAllConnections()
      await new Promise<void>((resolve) => server.close(() => resolve()))
    },
  }
}

/**
 * Serves a bare clone of `repoDir` over loopback HTTP using git's dumb
 * protocol, so a registry package can reference it as a remote git
 * dependency rather than a host-filesystem `git+file:` one.
 */
async function serveGitRepoOverHttp (
  scratchRoot: string,
  repoDir: string
): Promise<{ gitSpec: string, close: () => Promise<void> }> {
  const bareName = `${path.basename(repoDir)}.git`
  const bareDir = path.join(scratchRoot, 'served', bareName)
  execFileSync('git', ['clone', '--bare', repoDir, bareDir])
  execFileSync('git', ['update-server-info'], { cwd: bareDir })
  const server = http.createServer((req, res) => {
    const urlPath = decodeURIComponent((req.url ?? '/').split('?')[0])
    const filePath = path.join(scratchRoot, 'served', urlPath)
    if (!filePath.startsWith(bareDir) || !fs.existsSync(filePath) || fs.statSync(filePath).isDirectory()) {
      res.writeHead(404).end()
      return
    }
    res.writeHead(200, { 'content-type': 'application/octet-stream' })
    fs.createReadStream(filePath).pipe(res)
  })
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve))
  const { port } = server.address() as AddressInfo
  return {
    gitSpec: `git+http://127.0.0.1:${port}/${bareName}#main`,
    close: async () => new Promise<void>((resolve) => server.close(() => resolve())),
  }
}

function initLocalGitPackage (scratchRoot: string, repoName: string): string {
  const normalized = initLocalGitRepo(scratchRoot, repoName).replace(/\\/g, '/')
  return normalized.startsWith('/') ? `git+file://${normalized}#main` : `git+file:///${normalized}#main`
}

function initLocalGitRepo (scratchRoot: string, repoName: string): string {
  const repoDir = path.join(scratchRoot, repoName)
  fs.mkdirSync(repoDir, { recursive: true })
  writeJsonFileSync(path.join(repoDir, 'package.json'), {
    name: repoName,
    version: '1.0.0',
    main: 'index.js',
  })
  fs.writeFileSync(path.join(repoDir, 'index.js'), `module.exports = ${JSON.stringify(repoName)}\n`)
  execFileSync('git', ['init', '--initial-branch=main'], { cwd: repoDir })
  execFileSync('git', ['config', 'user.email', 'test@example.com'], { cwd: repoDir })
  execFileSync('git', ['config', 'user.name', 'Test User'], { cwd: repoDir })
  execFileSync('git', ['config', 'commit.gpgsign', 'false'], { cwd: repoDir })
  execFileSync('git', ['add', '.'], { cwd: repoDir })
  execFileSync('git', ['commit', '-m', 'init'], { cwd: repoDir })
  return repoDir
}

async function runPnpm (
  args: string[],
  registryUrl: string,
  opts?: { expectSuccess?: boolean }
): Promise<{ status: number | null, output: string }> {
  const proc = spawnPnpm(args, {
    env: {
      pnpm_config_silent: 'false',
      pnpm_config_registry: registryUrl,
    },
  })
  const result = await waitForPnpmExit(proc)
  const output = `${result.stdout.toString()}\n${result.stderr.toString()}`
  if (opts?.expectSuccess && result.status !== 0) {
    throw new Error(`Expected pnpm ${args.join(' ')} to succeed (exit ${String(result.status)}):\n${output}`)
  }
  return { status: result.status, output }
}

describe('lockfile blockExoticSubdeps verification', () => {
  test('frozen, warm, and repeat installs reject a URL tarball subdependency without fetching it', async () => {
    prepareEmpty()
    const scratchRoot = path.resolve('../exotic-fixtures')
    const exoticTgz = createTarballFixture(
      scratchRoot,
      'exotic-pkg',
      { name: 'exotic', version: '0.0.0', main: 'index.js' },
      'module.exports = "EXOTIC_PAYLOAD"\n'
    )
    const exoticServer = await startTestServer(scratchRoot, { exoticTgz })
    const registryServer = await startTestServer(scratchRoot, {
      packages: [
        {
          name: 'reg-parent',
          version: '1.0.0',
          dependencies: { exotic: `${exoticServer.baseUrl}/exotic.tgz` },
        },
      ],
    })
    const registryUrl = `${registryServer.baseUrl}/`

    try {
      writeJsonFileSync('package.json', {
        name: 'repro-app',
        version: '1.0.0',
        dependencies: { 'reg-parent': '1.0.0' },
      })
      writeYamlFileSync('pnpm-workspace.yaml', {
        blockExoticSubdeps: false,
      })

      await runPnpm(['install'], registryUrl, { expectSuccess: true })
      expect(fs.existsSync('node_modules/reg-parent')).toBe(true)

      writeYamlFileSync('pnpm-workspace.yaml', {
        blockExoticSubdeps: true,
      })

      const hotRepeat = await runPnpm(['install'], registryUrl)
      expect(hotRepeat.status).toBe(1)
      expect(hotRepeat.output).toContain('ERR_PNPM_EXOTIC_SUBDEP')
      expect(hotRepeat.output).not.toContain('Already up to date')

      fs.rmSync('node_modules', { recursive: true, force: true })
      exoticServer.resetExoticHits()
      const coldStore = path.resolve('../cold-store')
      const coldCache = path.resolve('../cold-cache')
      const coldArgs = [`--store-dir=${coldStore}`, `--config.cache-dir=${coldCache}`]

      const frozen = await runPnpm(['install', '--frozen-lockfile', ...coldArgs], registryUrl)
      expect(frozen.status).toBe(1)
      expect(frozen.output).toContain('ERR_PNPM_EXOTIC_SUBDEP')
      expect(exoticServer.getExoticHits()).toBe(0)
      expect(fs.existsSync('node_modules/reg-parent')).toBe(false)

      const warm = await runPnpm(['install', ...coldArgs], registryUrl)
      expect(warm.status).toBe(1)
      expect(warm.output).toContain('ERR_PNPM_EXOTIC_SUBDEP')
      expect(exoticServer.getExoticHits()).toBe(0)

      await runPnpm(
        ['install', '--frozen-lockfile', '--trust-lockfile', ...coldArgs],
        registryUrl,
        { expectSuccess: true }
      )
      const afterTrust = await runPnpm(['install', ...coldArgs], registryUrl)
      expect(afterTrust.status).toBe(1)
      expect(afterTrust.output).toContain('ERR_PNPM_EXOTIC_SUBDEP')
    } finally {
      await registryServer.close()
      await exoticServer.close()
    }
  })

  test('frozen and warm installs reject a git subdependency', async () => {
    prepareEmpty()
    const scratchRoot = path.resolve('../git-exotic-fixtures')
    const servedGit = await serveGitRepoOverHttp(scratchRoot, initLocalGitRepo(scratchRoot, 'exotic-git'))
    const gitSpec = servedGit.gitSpec
    const registryServer = await startTestServer(scratchRoot, {
      packages: [
        {
          name: 'git-parent',
          version: '1.0.0',
          dependencies: { 'exotic-git': gitSpec },
        },
      ],
    })
    const registryUrl = `${registryServer.baseUrl}/`

    try {
      writeJsonFileSync('package.json', {
        name: 'git-subdep-app',
        version: '1.0.0',
        dependencies: { 'git-parent': '1.0.0' },
      })
      writeYamlFileSync('pnpm-workspace.yaml', {
        blockExoticSubdeps: false,
      })

      await runPnpm(['install', '--lockfile-only'], registryUrl, { expectSuccess: true })

      writeYamlFileSync('pnpm-workspace.yaml', {
        blockExoticSubdeps: true,
      })

      const frozenGit = await runPnpm(['install', '--frozen-lockfile'], registryUrl)
      expect(frozenGit.status).toBe(1)
      expect(frozenGit.output).toContain('ERR_PNPM_EXOTIC_SUBDEP')

      const warmGit = await runPnpm(['install'], registryUrl)
      expect(warmGit.status).toBe(1)
      expect(warmGit.output).toContain('ERR_PNPM_EXOTIC_SUBDEP')
    } finally {
      await registryServer.close()
      await servedGit.close()
    }
  })

  test('direct importer exotic deps, workspace package exotic deps, and overrides pass frozen-lockfile', async () => {
    prepareEmpty()
    const scratchRoot = path.resolve('../allowed-exotic-fixtures')
    const exoticTgz = createTarballFixture(
      scratchRoot,
      'ws-exotic-pkg',
      { name: 'exotic', version: '0.0.0', main: 'index.js' }
    )
    const exoticServer = await startTestServer(scratchRoot, { exoticTgz })
    const directGitSpec = initLocalGitPackage(scratchRoot, 'direct-git')
    const mockRegistryUrl = `http://localhost:${REGISTRY_MOCK_PORT}/npm/`

    try {
      fs.mkdirSync('packages/pkg-a', { recursive: true })
      writeJsonFileSync('packages/pkg-a/package.json', {
        name: 'pkg-a',
        version: '1.0.0',
        dependencies: {
          exotic: `${exoticServer.baseUrl}/exotic.tgz`,
        },
      })
      writeJsonFileSync('package.json', {
        name: 'root-app',
        version: '1.0.0',
        dependencies: {
          'direct-git': directGitSpec,
          'pkg-a': 'workspace:*',
          '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
        },
      })
      writeYamlFileSync('pnpm-workspace.yaml', {
        packages: ['packages/*'],
        blockExoticSubdeps: true,
        injectWorkspacePackages: true,
        overrides: {
          '@pnpm.e2e/pkg-with-1-dep>@pnpm.e2e/dep-of-pkg-with-1-dep': '100.1.0',
        },
      })

      await runPnpm(['install'], mockRegistryUrl, { expectSuccess: true })

      fs.rmSync('node_modules', { recursive: true, force: true })
      fs.rmSync('packages/pkg-a/node_modules', { recursive: true, force: true })

      await runPnpm(['install', '--frozen-lockfile'], mockRegistryUrl, { expectSuccess: true })
      expect(fs.existsSync('node_modules/direct-git')).toBe(true)
      expect(fs.existsSync('node_modules/pkg-a')).toBe(true)

      await runPnpm(['--filter', 'root-app', 'deploy', './deployed'], mockRegistryUrl, { expectSuccess: true })
      expect(fs.existsSync('deployed/node_modules/pkg-a')).toBe(true)

      await runPnpm(['--filter', 'root-app', 'deploy', '--legacy', './deployed-legacy'], mockRegistryUrl, { expectSuccess: true })
      expect(fs.existsSync('deployed-legacy/node_modules/pkg-a')).toBe(true)
    } finally {
      await exoticServer.close()
    }
  })

  test('rejects forged runtime binary URLs and phantom importer peer providers without fetching', async () => {
    prepareEmpty()
    const scratchRoot = path.resolve('../forged-exotic-fixtures')
    const exoticTgz = createTarballFixture(
      scratchRoot,
      'forged-exotic-pkg',
      { name: 'exotic', version: '0.0.0', main: 'index.js' }
    )
    const regParentTgz = createTarballFixture(
      scratchRoot,
      'forged-reg-parent-pkg',
      { name: 'reg-parent', version: '1.0.0', main: 'index.js' }
    )
    const server = await startTestServer(scratchRoot, {
      exoticTgz,
      packages: [{ name: 'reg-parent', version: '1.0.0' }],
    })
    const registryUrl = `${server.baseUrl}/`
    const exoticUrl = `${server.baseUrl}/exotic.tgz`
    const exoticIntegrity = sha512Integrity(exoticTgz)
    const regParentIntegrity = sha512Integrity(regParentTgz)

    try {
      writeJsonFileSync('package.json', {
        name: 'app',
        version: '1.0.0',
        dependencies: {
          'reg-parent': '1.0.0',
        },
      })
      writeYamlFileSync('pnpm-workspace.yaml', {
        blockExoticSubdeps: true,
      })

      fs.writeFileSync(
        'pnpm-lock.yaml',
        `lockfileVersion: '9.0'

importers:
  .:
    dependencies:
      reg-parent:
        specifier: 1.0.0
        version: 1.0.0

packages:
  reg-parent@1.0.0:
    resolution: {integrity: ${regParentIntegrity}}
  node@runtime:22.0.0:
    resolution: {type: binary, archive: tarball, url: ${exoticUrl}, integrity: ${exoticIntegrity}, bin: index.js}
    version: 22.0.0

snapshots:
  reg-parent@1.0.0:
    dependencies:
      node: runtime:22.0.0
  node@runtime:22.0.0: {}
`
      )
      server.resetExoticHits()

      const frozenRuntime = await runPnpm(['install', '--frozen-lockfile'], registryUrl)
      expect(frozenRuntime.status).toBe(1)
      expect(frozenRuntime.output).toContain('ERR_PNPM_EXOTIC_SUBDEP')

      const warmRuntime = await runPnpm(['install'], registryUrl)
      expect(warmRuntime.status).toBe(1)
      expect(warmRuntime.output).toContain('ERR_PNPM_EXOTIC_SUBDEP')
      expect(server.getExoticHits()).toBe(0)

      fs.writeFileSync(
        'pnpm-lock.yaml',
        `lockfileVersion: '9.0'

importers:
  .:
    dependencies:
      reg-parent:
        specifier: 1.0.0
        version: 1.0.0(exotic@${exoticUrl})
  ghost-pkg:
    dependencies:
      exotic:
        specifier: ${exoticUrl}
        version: ${exoticUrl}

packages:
  reg-parent@1.0.0:
    resolution: {integrity: ${regParentIntegrity}}
    peerDependencies:
      exotic: '*'
  exotic@${exoticUrl}:
    resolution: {tarball: ${exoticUrl}, integrity: ${exoticIntegrity}}
    version: 0.0.0

snapshots:
  reg-parent@1.0.0(exotic@${exoticUrl}):
    dependencies:
      exotic: ${exoticUrl}
  exotic@${exoticUrl}: {}
`
      )

      const frozenPhantom = await runPnpm(['install', '--frozen-lockfile'], registryUrl)
      expect(frozenPhantom.status).toBe(1)
      expect(frozenPhantom.output).toContain('ERR_PNPM_EXOTIC_SUBDEP')

      const ignoreManifestPhantom = await runPnpm(
        ['install', '--frozen-lockfile', '--config.ignore-package-manifest=true'],
        registryUrl
      )
      expect(ignoreManifestPhantom.status).toBe(1)
      expect(ignoreManifestPhantom.output).toContain('ERR_PNPM_EXOTIC_SUBDEP')

      const warmPhantom = await runPnpm(['install'], registryUrl)
      expect(warmPhantom.status).toBe(1)
      expect(warmPhantom.output).toContain('ERR_PNPM_EXOTIC_SUBDEP')
      expect(server.getExoticHits()).toBe(0)
    } finally {
      await server.close()
    }
  })
})
