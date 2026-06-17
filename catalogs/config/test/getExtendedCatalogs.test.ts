import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { getExtendedCatalogs } from '@pnpm/catalogs.config'
import { writeYamlFileSync } from 'write-yaml-file'

function createWorkspace (manifestsByDir: Record<string, unknown>): string {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-extended-catalogs-'))
  for (const [dir, manifest] of Object.entries(manifestsByDir)) {
    const absoluteDir = path.join(root, dir)
    fs.mkdirSync(absoluteDir, { recursive: true })
    writeYamlFileSync(path.join(absoluteDir, 'pnpm-workspace.yaml'), manifest)
  }
  return root
}

test('returns the manifest own catalogs when there is no extends field', async () => {
  const root = createWorkspace({ '.': { catalog: { foo: '^1.0.0' } } })

  await expect(getExtendedCatalogs(root, { catalog: { foo: '^1.0.0' } })).resolves.toEqual({
    default: { foo: '^1.0.0' },
  })
})

test('returns an empty catalog for an undefined manifest', async () => {
  const root = createWorkspace({ '.': {} })

  await expect(getExtendedCatalogs(root, undefined)).resolves.toEqual({ default: undefined })
})

test('merges catalogs from the extended workspace manifest', async () => {
  const root = createWorkspace({
    '.': { catalog: { 'is-positive': '^3.1.0' } },
    'packages/a': { catalog: { 'is-odd': '^3.0.1' } },
  })

  await expect(getExtendedCatalogs(path.join(root, 'packages/a'), {
    catalog: { 'is-odd': '^3.0.1' },
    extends: '../..',
  })).resolves.toEqual({
    default: { 'is-odd': '^3.0.1', 'is-positive': '^3.1.0' },
  })
})

test('accepts an array in the extends field and supports named catalogs', async () => {
  const root = createWorkspace({
    base1: { catalogs: { tools: { eslint: '^9.0.0' } } },
    base2: { catalog: { react: '^18.0.0' } },
  })

  await expect(getExtendedCatalogs(root, {
    catalog: { lodash: '^4.0.0' },
    extends: ['base1', 'base2'],
  })).resolves.toEqual({
    default: { react: '^18.0.0', lodash: '^4.0.0' },
    tools: { eslint: '^9.0.0' },
  })
})

test('the extending manifest catalogs take precedence over the extended ones', async () => {
  const root = createWorkspace({
    '.': { catalog: { foo: '^1.0.0' } },
    'packages/a': {},
  })

  await expect(getExtendedCatalogs(path.join(root, 'packages/a'), { catalog: { foo: '^2.0.0' }, extends: '../..' })).resolves.toEqual({
    default: { foo: '^2.0.0' },
  })
})

test('manifests listed later in extends take precedence over earlier ones', async () => {
  const root = createWorkspace({
    a: { catalog: { foo: '^1.0.0' } },
    b: { catalog: { foo: '^2.0.0' } },
  })

  await expect(getExtendedCatalogs(root, { extends: ['a', 'b'] })).resolves.toEqual({
    default: { foo: '^2.0.0' },
  })
})

test('extends is resolved recursively', async () => {
  const root = createWorkspace({
    a: { catalog: { a: '^1.0.0' }, extends: '../b' },
    b: { catalog: { b: '^1.0.0' } },
  })

  await expect(getExtendedCatalogs(root, { extends: 'a' })).resolves.toEqual({
    default: { a: '^1.0.0', b: '^1.0.0' },
  })
})

test('throws when an extended workspace manifest cannot be found', async () => {
  const root = createWorkspace({ '.': {} })

  await expect(getExtendedCatalogs(root, { extends: 'packages/missing' })).rejects.toMatchObject({
    code: 'ERR_PNPM_WORKSPACE_EXTENDS_NOT_FOUND',
  })
})

test('throws when extends references form a cycle', async () => {
  const root = createWorkspace({
    a: { extends: '../b' },
    b: { extends: '../a' },
  })

  await expect(getExtendedCatalogs(root, { extends: 'a' })).rejects.toMatchObject({
    code: 'ERR_PNPM_WORKSPACE_EXTENDS_CYCLE',
  })
})

test('the <root> token resolves to the nearest ancestor workspace', async () => {
  const root = createWorkspace({
    '.': { catalog: { 'is-positive': '^3.1.0' } },
    'packages/a': { catalog: { 'is-odd': '^3.0.1' } },
  })

  await expect(getExtendedCatalogs(path.join(root, 'packages/a'), {
    catalog: { 'is-odd': '^3.0.1' },
    extends: '<root>',
  })).resolves.toEqual({
    default: { 'is-odd': '^3.0.1', 'is-positive': '^3.1.0' },
  })
})

test('the <root> token works as a path prefix', async () => {
  const root = createWorkspace({
    '.': {},
    'configs/base': { catalog: { react: '^18.0.0' } },
    'packages/a': {},
  })

  await expect(getExtendedCatalogs(path.join(root, 'packages/a'), {
    extends: '<root>/configs/base',
  })).resolves.toEqual({
    default: { react: '^18.0.0' },
  })
})

test('throws when <root> has no ancestor workspace', async () => {
  const root = createWorkspace({ '.': {} })

  await expect(getExtendedCatalogs(root, { extends: '<root>' })).rejects.toMatchObject({
    code: 'ERR_PNPM_WORKSPACE_EXTENDS_ROOT_NOT_FOUND',
  })
})

test('a glob extends every matching workspace manifest and skips directories without one', async () => {
  const root = createWorkspace({
    'packages/a': { catalog: { a: '^1.0.0' } },
    'packages/b': { catalog: { b: '^1.0.0' } },
  })
  fs.mkdirSync(path.join(root, 'packages/c-without-manifest'), { recursive: true })

  await expect(getExtendedCatalogs(root, { extends: 'packages/*' })).resolves.toEqual({
    default: { a: '^1.0.0', b: '^1.0.0' },
  })
})

test('later glob matches win on conflicts', async () => {
  const root = createWorkspace({
    'packages/a': { catalog: { foo: '^1.0.0' } },
    'packages/b': { catalog: { foo: '^2.0.0' } },
  })

  await expect(getExtendedCatalogs(root, { extends: 'packages/*' })).resolves.toEqual({
    default: { foo: '^2.0.0' },
  })
})

test('a glob with no matches contributes nothing', async () => {
  const root = createWorkspace({ '.': { catalog: { foo: '^1.0.0' } } })

  await expect(getExtendedCatalogs(root, {
    catalog: { foo: '^1.0.0' },
    extends: 'packages/*',
  })).resolves.toEqual({ default: { foo: '^1.0.0' } })
})

test('extends accepts a direct path to a pnpm-workspace.yaml file', async () => {
  const root = createWorkspace({
    '.': { catalog: { foo: '^1.0.0' } },
    'packages/a': {},
  })

  await expect(getExtendedCatalogs(path.join(root, 'packages/a'), {
    extends: '../../pnpm-workspace.yaml',
  })).resolves.toEqual({
    default: { foo: '^1.0.0' },
  })
})

test('extends accepts an absolute path to a manifest outside the workspace', async () => {
  const external = createWorkspace({ '.': { catalog: { shared: '^1.0.0' } } })
  const root = createWorkspace({ '.': {} })

  await expect(getExtendedCatalogs(root, {
    extends: path.join(external, 'pnpm-workspace.yaml'),
  })).resolves.toEqual({
    default: { shared: '^1.0.0' },
  })
})

test('throws on a cycle between the root and a package (glob + <root>)', async () => {
  const root = createWorkspace({
    '.': { extends: 'packages/*' },
    'packages/a': { extends: '<root>' },
  })

  await expect(getExtendedCatalogs(root, { extends: 'packages/*' })).rejects.toMatchObject({
    code: 'ERR_PNPM_WORKSPACE_EXTENDS_CYCLE',
  })
})
