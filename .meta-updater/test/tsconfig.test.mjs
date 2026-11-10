import assert from 'node:assert/strict'
import { mkdtemp, mkdir, readFile, rm, writeFile } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { test } from 'node:test'

import updater from '../lib/index.js'

for (const format of ['v9', 'v13', 'none']) {
  test(`tsconfig references follow manifest changes with ${format} lockfiles`, async t => {
    const workspaceDir = await createWorkspace(t, format)
    const manifest = {
      name: 'consumer',
      dependencies: { library: 'workspace:*' },
    }
    await writeJson(workspaceDir, 'packages/consumer/package.json', manifest)
    let update = (await updater(workspaceDir)).files['tsconfig.json']
    const options = { dir: path.join(workspaceDir, 'packages/consumer'), manifest }
    const config = await update({}, options)
    assert.deepEqual(config.references, [{ path: '../library' }])

    delete manifest.dependencies.library
    await writeJson(workspaceDir, 'packages/consumer/package.json', manifest)
    update = (await updater(workspaceDir)).files['tsconfig.json']
    assert.deepEqual((await update(config, options)).references, [])
  })
}

test('workspace aliases, workspace paths, and link dependencies produce sorted references', async t => {
  const workspaceDir = await createWorkspace(t, 'v13')
  const manifest = {
    name: 'consumer',
    dependencies: {
      alias: 'workspace:library@*',
      relative: 'workspace:../another',
      external: '^1.0.0',
    },
    devDependencies: { linked: 'link:../linked', consumer: 'workspace:*' },
    optionalDependencies: { optional: 'workspace:*' },
    peerDependencies: { peer: 'workspace:*' },
  }
  for (const name of ['another', 'linked', 'external', 'optional', 'peer']) {
    await writeJson(workspaceDir, `packages/${name}/package.json`, { name, version: '1.0.0' })
    await writeJson(workspaceDir, `packages/${name}/tsconfig.json`, {})
  }
  await writeJson(workspaceDir, 'packages/consumer/package.json', manifest)
  const update = (await updater(workspaceDir)).files['tsconfig.json']
  const config = await update({}, { dir: path.join(workspaceDir, 'packages/consumer'), manifest })
  assert.deepEqual(config.references, [
    { path: '../another' },
    { path: '../library' },
    { path: '../linked' },
    { path: '../peer' },
  ])
})

test('references exclude packages without tsconfigs, outside links, and known cycles', async t => {
  const workspaceDir = await createWorkspace(t, 'v13')
  await writeJson(workspaceDir, 'packages/no-types/package.json', { name: 'no-types' })
  await writeJson(workspaceDir, 'packages/library/package.json', { name: '@pnpm/store.controller' })
  const outsideDir = await mkdtemp(path.join(os.tmpdir(), 'meta-updater-outside-'))
  t.after(() => rm(outsideDir, { recursive: true, force: true }))
  await writeJson(outsideDir, 'tsconfig.json', {})
  const manifest = {
    name: '@pnpm/fetching.git-fetcher',
    dependencies: {
      '@pnpm/store.controller': 'workspace:*',
      'no-types': 'workspace:*',
      outside: `link:${outsideDir}`,
    },
  }
  await writeJson(workspaceDir, 'packages/consumer/package.json', manifest)
  const update = (await updater(workspaceDir)).files['tsconfig.json']
  const config = await update({}, { dir: path.join(workspaceDir, 'packages/consumer'), manifest })
  assert.deepEqual(config.references, [])
})

test('test and lint tsconfigs are synchronized with the source references', async t => {
  const workspaceDir = await createWorkspace(t, 'v13')
  const dir = path.join(workspaceDir, 'packages/consumer')
  await mkdir(path.join(dir, 'test'), { recursive: true })
  const manifest = { name: 'consumer', devDependencies: { library: 'workspace:*' } }
  await writeJson(workspaceDir, 'packages/consumer/package.json', manifest)
  const update = (await updater(workspaceDir)).files['tsconfig.json']
  await update({}, { dir, manifest })
  assert.deepEqual((await readJson(dir, 'test/tsconfig.json')).references, [{ path: '..' }])
  await update({ compilerOptions: { composite: false } }, { dir, manifest })
  assert.deepEqual((await readJson(dir, 'test/tsconfig.json')).references, [{ path: '../../library' }])
  assert.equal((await readJson(dir, 'tsconfig.lint.json')).extends, './tsconfig.json')
})

test('packages with independent tsconfigs are left unchanged', async t => {
  const workspaceDir = await createWorkspace(t, 'v13')
  const update = (await updater(workspaceDir)).files['tsconfig.json']
  for (const name of ['@pnpm/tsconfig', '@pnpm-private/typecheck', 'get-pnpm', 'pacquet', '@pnpm/napi']) {
    const config = { references: [{ path: 'preserved' }] }
    assert.equal(await update(config, {
      dir: path.join(workspaceDir, 'packages/consumer'),
      manifest: { name },
    }), config)
  }
})

async function createWorkspace (t, format) {
  const workspaceDir = await mkdtemp(path.join(os.tmpdir(), 'meta-updater-'))
  t.after(() => rm(workspaceDir, { recursive: true, force: true }))
  await writeJson(workspaceDir, 'package.json', { name: 'fixture', private: true })
  for (const file of ['pnpm11/pnpm/package.json', 'pnpm/npm/pnpm/package.json', 'pnpr/npm/pnpr/package.json']) {
    await writeJson(workspaceDir, file, { version: '1.0.0' })
  }
  await writeJson(workspaceDir, '.config/npm-signing-keys.json', [])
  await writeJson(workspaceDir, 'packages/library/package.json', { name: 'library', version: '1.0.0' })
  await writeJson(workspaceDir, 'packages/library/tsconfig.json', {})
  await writeFile(path.join(workspaceDir, 'pnpm-workspace.yaml'), 'packages:\n  - packages/*\n')
  if (format !== 'none') {
    const legacy = 'lockfileVersion: "9.0"\nimporters:\n  packages/consumer:\n    dependencies:\n      library:\n        specifier: workspace:*\n        version: link:../library\n'
    const current = '---\n{"lockfileVersion":"13.0","projects":{"packages/consumer":{"dependencies":{"library":"link:../library"}}}}\n'
    await writeFile(path.join(workspaceDir, 'pnpm-lock.yaml'), format === 'v13' ? `---\n${legacy}${current}` : legacy)
  }
  return workspaceDir
}

async function writeJson (workspaceDir, file, value) {
  const resolvedPath = path.join(workspaceDir, file)
  await mkdir(path.dirname(resolvedPath), { recursive: true })
  await writeFile(resolvedPath, JSON.stringify(value))
}

async function readJson (dir, file) {
  return JSON.parse(await readFile(path.join(dir, file), 'utf8'))
}
