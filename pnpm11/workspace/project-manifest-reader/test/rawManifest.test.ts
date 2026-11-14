import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import {
  readRawProjectManifest,
  readRawProjectManifestOnly,
} from '@pnpm/workspace.project-manifest-reader'
import { temporaryDirectory } from 'tempy'

test('readRawProjectManifest() leaves runtime engines unfolded on read and write', async () => {
  const dir = temporaryDirectory()
  const manifestPath = path.join(dir, 'package.json')
  fs.writeFileSync(manifestPath, JSON.stringify({
    name: 'fixture',
    devEngines: {
      runtime: {
        name: 'node',
        version: '22.14.0',
        onFail: 'download',
      },
    },
    engines: {
      runtime: {
        name: 'node',
        version: '22.14.0',
        onFail: 'download',
      },
    },
  }, null, 2) + '\n')

  const { manifest, writeProjectManifest } = await readRawProjectManifest(dir)
  expect(manifest.devDependencies).toBeUndefined()
  expect(manifest.dependencies).toBeUndefined()

  const devEngines = manifest.devEngines as { runtime: { version: string } }
  devEngines.runtime.version = '24.0.0'
  await writeProjectManifest(manifest)

  const saved = JSON.parse(fs.readFileSync(manifestPath, 'utf8'))
  expect(saved.devEngines.runtime.version).toBe('24.0.0')
  expect(saved.devDependencies).toBeUndefined()
  expect(saved.dependencies).toBeUndefined()
})

test('readRawProjectManifest() preserves empty dependency objects, key order, and formatting', async () => {
  const dir = temporaryDirectory()
  const manifestPath = path.join(dir, 'package.json')
  fs.writeFileSync(
    manifestPath,
    '{\r\n\t"name": "fixture",\r\n\t"dependencies": {\r\n\t\t"zebra": "1.0.0",\r\n\t\t"alpha": "2.0.0"\r\n\t}\r\n}'
  )

  const { manifest, writeProjectManifest } = await readRawProjectManifest(dir)
  manifest.devDependencies = {}
  await writeProjectManifest(manifest)

  expect(fs.readFileSync(manifestPath, 'utf8')).toBe(
    '{\r\n\t"name": "fixture",\r\n\t"dependencies": {\r\n\t\t"zebra": "1.0.0",\r\n\t\t"alpha": "2.0.0"\r\n\t},\r\n\t"devDependencies": {}\r\n}'
  )
})

test('readRawProjectManifest() skips no-op writes and writes reordered keys', async () => {
  const dir = temporaryDirectory()
  const manifestPath = path.join(dir, 'package.json')
  const withBom = '\uFEFF{\n  "scripts": {\n    "a": "1",\n    "b": "2"\n  }\n}\n'
  fs.writeFileSync(manifestPath, withBom)

  const { manifest, writeProjectManifest } = await readRawProjectManifest(dir)
  await writeProjectManifest(manifest)
  expect(fs.readFileSync(manifestPath, 'utf8')).toBe(withBom)

  manifest.scripts = { b: '2', a: '1' }
  await writeProjectManifest(manifest)
  expect(fs.readFileSync(manifestPath, 'utf8')).toBe(
    '{\n  "scripts": {\n    "b": "2",\n    "a": "1"\n  }\n}\n'
  )
})

test('readRawProjectManifestOnly() treats empty package.yaml as empty object and rejects non-object roots', async () => {
  const yamlDir = temporaryDirectory()
  fs.writeFileSync(path.join(yamlDir, 'package.yaml'), '')
  expect(await readRawProjectManifestOnly(yamlDir)).toStrictEqual({})

  const invalidJsonDir = temporaryDirectory()
  fs.writeFileSync(path.join(invalidJsonDir, 'package.json'), '["not-an-object"]')
  await expect(readRawProjectManifestOnly(invalidJsonDir)).rejects.toMatchObject({
    code: 'ERR_PNPM_INVALID_MANIFEST',
  })
})
