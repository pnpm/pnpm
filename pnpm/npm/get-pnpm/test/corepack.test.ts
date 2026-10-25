import assert from 'node:assert/strict'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { afterEach, describe, test } from 'node:test'

import { corepackDownloadOptions } from '../lib/corepack.js'

const KEY = { keyid: 'SHA256:test', key: 'MFkw', expires: null }

const tmpDirs: string[] = []

afterEach(() => {
  for (const dir of tmpDirs.splice(0)) {
    fs.rmSync(dir, { recursive: true, force: true })
  }
})

describe('corepackDownloadOptions', () => {
  test('downloads from the npm registry with its own keys by default', () => {
    const { cwd, homeDir } = layout()

    assert.deepEqual(corepackDownloadOptions({ cwd, homeDir, env: {} }), {
      registry: 'https://registry.npmjs.org/',
    })
  })

  test("takes Corepack's registry and credentials over pnpm's config", () => {
    const { cwd, homeDir } = layout({ user: ['registry=https://user.example.test/', '//user.example.test/:_authToken=user-token'] })

    const options = corepackDownloadOptions({
      cwd,
      homeDir,
      env: { COREPACK_NPM_REGISTRY: 'https://corepack.example.test/', COREPACK_NPM_TOKEN: 'corepack-token' },
    })

    assert.deepEqual(options, {
      registry: 'https://corepack.example.test/',
      headers: { authorization: 'Bearer corepack-token' },
    })
  })

  test("sends Corepack's username and password to Corepack's registry", () => {
    const { cwd, homeDir } = layout()

    const options = corepackDownloadOptions({
      cwd,
      homeDir,
      env: { COREPACK_NPM_REGISTRY: 'https://corepack.example.test/', COREPACK_NPM_USERNAME: 'user', COREPACK_NPM_PASSWORD: 'p@ss' },
    })

    assert.deepEqual(options.headers, { authorization: `Basic ${Buffer.from('user:p@ss').toString('base64')}` })
  })

  test("sends Corepack's credentials to the npm registry only when pnpm's config holds none", () => {
    const env = { COREPACK_NPM_TOKEN: 'corepack-token' }

    const unconfigured = layout()
    assert.deepEqual(corepackDownloadOptions({ ...unconfigured, env }).headers, { authorization: 'Bearer corepack-token' })

    const withToken = layout({ user: ['//registry.npmjs.org/:_authToken=user-token'] })
    assert.deepEqual(corepackDownloadOptions({ ...withToken, env }).headers, { authorization: 'Bearer user-token' })

    const elsewhere = layout({ user: ['registry=https://user.example.test/'] })
    assert.deepEqual(corepackDownloadOptions({ ...elsewhere, env }), { registry: 'https://user.example.test/' })
  })

  test('trusts the npm keys of COREPACK_INTEGRITY_KEYS', () => {
    const { cwd, homeDir } = layout()

    const options = corepackDownloadOptions({ cwd, homeDir, env: { COREPACK_INTEGRITY_KEYS: JSON.stringify({ npm: [KEY] }) } })

    assert.deepEqual(options.keys, [KEY])
  })

  test('skips the signature check when COREPACK_INTEGRITY_KEYS is 0 or empty, and reads no project .npmrc then', () => {
    const { cwd, homeDir } = layout({ user: ['registry=https://user.example.test/'], project: ['registry=https://project.example.test/'] })
    const checked = corepackDownloadOptions({ cwd, homeDir, env: {} })
    assert.equal(checked.registry, 'https://project.example.test/')

    for (const value of ['0', '']) {
      assert.deepEqual(corepackDownloadOptions({ cwd, homeDir, env: { COREPACK_INTEGRITY_KEYS: value } }), {
        registry: 'https://user.example.test/',
        verifySignature: false,
      }, value)
    }
  })

  test('refuses a COREPACK_INTEGRITY_KEYS without an npm key set', () => {
    const { cwd, homeDir } = layout()

    for (const value of ['{}', 'null', '{"npm":{}}']) {
      assert.throws(() => corepackDownloadOptions({ cwd, homeDir, env: { COREPACK_INTEGRITY_KEYS: value } }), /holds no "npm" key set/, value)
    }
    assert.throws(() => corepackDownloadOptions({ cwd, homeDir, env: { COREPACK_INTEGRITY_KEYS: '{' } }), /not readable as JSON/)
  })

  test('refuses to download when Corepack has no network', () => {
    const { cwd, homeDir } = layout()

    assert.throws(() => corepackDownloadOptions({ cwd, homeDir, env: { COREPACK_ENABLE_NETWORK: '0' } }), /Network access is disabled/)
  })
})

/** A home directory, and a workspace whose package is the current directory. */
function layout (npmrcs: { user?: string[], project?: string[] } = {}): { homeDir: string, cwd: string } {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'get-pnpm-corepack-'))
  tmpDirs.push(root)
  const homeDir = path.join(root, 'home')
  const project = path.join(root, 'project')
  const cwd = path.join(project, 'packages', 'a')
  fs.mkdirSync(homeDir)
  fs.mkdirSync(cwd, { recursive: true })
  fs.writeFileSync(path.join(project, 'pnpm-workspace.yaml'), '')
  if (npmrcs.user != null) fs.writeFileSync(path.join(homeDir, '.npmrc'), `${npmrcs.user.join('\n')}\n`)
  if (npmrcs.project != null) fs.writeFileSync(path.join(project, '.npmrc'), `${npmrcs.project.join('\n')}\n`)
  return { homeDir, cwd }
}
