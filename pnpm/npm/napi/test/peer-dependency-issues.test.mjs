import assert from 'node:assert/strict'
import fs from 'node:fs'
import { createRequire } from 'node:module'
import os from 'node:os'
import path from 'node:path'
import { describe, it } from 'node:test'

const { getPeerDependencyIssues } = createRequire(import.meta.url)('../index.js')

describe('getPeerDependencyIssues', () => {
  it('rejects malformed options at the JavaScript boundary', async () => {
    const project = { rootDir: process.cwd(), manifest: {} }
    const options = { dir: process.cwd(), projects: [project] }

    await assertCallRejects(
      { dir: process.cwd(), projects: [{ rootDir: process.cwd() }] },
      /Missing field `manifest`/,
    )
    await assertCallRejects(
      { ...options, registries: { default: 42 } },
      /into rust type `String`/,
    )
    for (const field of ['peersSuffixMaxLength', 'virtualStoreDirMaxLength']) {
      await assertCallRejects(
        { ...options, [field]: 2 ** 32 },
        new RegExp(`${field}.*must be an integer from 0 through ${2 ** 32 - 1}`),
      )
    }
  })

  it('returns missing peer issues from a valid query', async (context) => {
    const rootDir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-napi-peer-issues-'))
    context.after(() => fs.rmSync(rootDir, { force: true, recursive: true }))

    const appDir = path.join(rootDir, 'app')
    const pluginDir = path.join(rootDir, 'plugin')
    fs.mkdirSync(appDir)
    fs.mkdirSync(pluginDir)
    fs.writeFileSync(path.join(pluginDir, 'package.json'), JSON.stringify({
      name: 'plugin',
      version: '1.0.0',
      peerDependencies: { host: '^1.0.0' },
    }))

    const result = await getPeerDependencyIssues({
      dir: appDir,
      projects: [
        {
          rootDir: appDir,
          manifest: {
            name: 'app',
            dependencies: { plugin: 'file:../plugin' },
          },
        },
        {
          rootDir: pluginDir,
          manifest: {
            name: 'plugin',
            version: '1.0.0',
            peerDependencies: { host: '^1.0.0' },
          },
        },
      ],
      storeDir: path.join(rootDir, 'store'),
      cacheDir: path.join(rootDir, 'cache'),
    })

    assert.deepEqual(result['.'], {
      missing: {
        host: [{
          parents: [{ name: 'plugin', version: '' }],
          optional: false,
          wantedRange: '^1.0.0',
        }],
      },
      bad: {},
      conflicts: [],
      intersections: { host: '^1.0.0' },
    })
  })
})

async function assertCallRejects (options, pattern) {
  await assert.rejects(
    Promise.resolve().then(() => getPeerDependencyIssues(options)),
    pattern,
  )
}
