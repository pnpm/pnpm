import { spawnSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import fs from 'node:fs'
import http from 'node:http'
import os from 'node:os'
import path from 'node:path'

import { afterAll, beforeAll, expect, test } from '@jest/globals'
import { installPnpm } from 'get-pnpm'

const VERSION = '99.0.0'
const TAMPERED_PREFIX = '/tampered/'

// The fake pnpm executable is a shell script, so this suite is POSIX-only.
const testOnPosix = process.platform === 'win32' ? test.skip : test

let tmpDir!: string
let setupLogPath!: string
let server!: http.Server
let registry!: string

beforeAll(async () => {
  tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-install-test-'))
  setupLogPath = path.join(tmpDir, 'setup.log')
  const tarballs = {
    wrapper: packTarball('wrapper', {
      'dist/worker.js': 'export {}\n',
    }),
    platform: packTarball('platform', {
      pnpm: `#!/bin/sh\necho "$@" > "${setupLogPath}"\nls "$(dirname "$0")/dist" >> "${setupLogPath}"\n`,
    }),
  }
  server = http.createServer((req, res) => {
    const url = decodeURIComponent(req.url!)
    // Anything under /tampered/ is served with a corrupted tarball, so a
    // checksum mismatch can be provoked without a second server.
    const tampered = url.startsWith(TAMPERED_PREFIX)
    const requestPath = tampered ? url.slice(TAMPERED_PREFIX.length - 1) : url
    if (requestPath === '/@pnpm/exe') {
      res.end(JSON.stringify({ 'dist-tags': { latest: VERSION }, versions: { [VERSION]: {} } }))
      return
    }
    if (requestPath.endsWith(`/${VERSION}`)) {
      const kind = requestPath === `/@pnpm/exe/${VERSION}` ? 'wrapper' : 'platform'
      res.end(JSON.stringify({
        dist: {
          tarball: `${registry}${tampered ? TAMPERED_PREFIX.slice(1) : ''}${kind}.tgz`,
          integrity: `sha512-${createHash('sha512').update(fs.readFileSync(tarballs[kind])).digest('base64')}`,
        },
      }))
      return
    }
    if (requestPath === '/wrapper.tgz' || requestPath === '/platform.tgz') {
      const tarball = fs.readFileSync(tarballs[requestPath === '/wrapper.tgz' ? 'wrapper' : 'platform'])
      res.end(tampered ? Buffer.concat([tarball, Buffer.from('tampered')]) : tarball)
      return
    }
    res.statusCode = 404
    res.end('not found')
  })
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve))
  registry = `http://127.0.0.1:${(server.address() as { port: number }).port}/`
})

afterAll(async () => {
  await new Promise<void>((resolve) => server.close(() => {
    resolve()
  }))
  fs.rmSync(tmpDir, { recursive: true, force: true })
})

testOnPosix('runs setup on an executable placed next to the dist directory it ships with', async () => {
  expect(await installPnpm({ versionSpec: 'latest', registry })).toBe(0)
  expect(fs.readFileSync(setupLogPath, 'utf8')).toBe('setup --force\nworker.js\n')
})

testOnPosix('leaves no temporary directory behind', async () => {
  const tmpDirsBefore = pnpmInstallTmpDirs()
  await installPnpm({ versionSpec: 'latest', registry })
  expect(pnpmInstallTmpDirs()).toStrictEqual(tmpDirsBefore)
})

testOnPosix('refuses a tarball that does not match the checksum the registry published', async () => {
  await expect(installPnpm({ versionSpec: 'latest', registry: `${registry}${TAMPERED_PREFIX.slice(1)}` }))
    .rejects.toThrow(/Checksum mismatch/)
})

function pnpmInstallTmpDirs (): string[] {
  return fs.readdirSync(os.tmpdir()).filter((entry) => entry.startsWith('pnpm-install-') && !entry.startsWith('pnpm-install-test-'))
}

function packTarball (name: string, files: Record<string, string>): string {
  const contentDir = path.join(tmpDir, name, 'package')
  for (const [filePath, content] of Object.entries(files)) {
    const dest = path.join(contentDir, filePath)
    fs.mkdirSync(path.dirname(dest), { recursive: true })
    fs.writeFileSync(dest, content, { mode: 0o755 })
  }
  const tarball = path.join(tmpDir, `${name}.tgz`)
  const { status, stderr } = spawnSync('tar', ['-czf', tarball, '-C', path.join(tmpDir, name), 'package'], { encoding: 'utf8' })
  if (status !== 0) throw new Error(`could not create the ${name} fixture tarball: ${stderr}`)
  return tarball
}
