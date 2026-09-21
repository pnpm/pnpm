/// <reference path="../../../__typings__/index.d.ts"/>
import fs from 'node:fs'
import path from 'node:path'
import { promisify } from 'node:util'

import { expect, test } from '@jest/globals'
import { writeProjectManifest } from '@pnpm/workspace.project-manifest-writer'
import { temporaryDirectory } from 'tempy'
import yaml from 'yaml'

const readFile = promisify(fs.readFile)

test('writeProjectManifest()', async () => {
  const dir = temporaryDirectory()

  await writeProjectManifest(path.join(dir, 'package.json'), { name: 'foo', version: '1.0.0' })
  expect(await readFile(path.join(dir, 'package.json'), 'utf8')).toBe('{\n\t"name": "foo",\n\t"version": "1.0.0"\n}\n')

  await writeProjectManifest(path.join(dir, 'package.json5'), { name: 'foo', version: '1.0.0' })
  expect(await readFile(path.join(dir, 'package.json5'), 'utf8')).toBe("{\n\tname: 'foo',\n\tversion: '1.0.0',\n}\n")

  await writeProjectManifest(path.join(dir, 'package.yaml'), { name: 'foo', version: '1.0.0' })
  expect(await readFile(path.join(dir, 'package.yaml'), 'utf8')).toBe('name: foo\nversion: 1.0.0\n')
})

test('preserves YAML comments and key order across updates, additions, and removals', async () => {
  const file = path.join(temporaryDirectory(), 'package.yaml')
  await fs.promises.writeFile(file, `# project
name: example

dependencies:
  # runtime dependencies
  zebra: '1.0.0' # pinned
  alpha: 1.0.0
  removed: 1.0.0

scripts:
  test: echo test # test command
`)
  await writeProjectManifest(file, {
    dependencies: { alpha: '1.0.0', beta: '2.0.0', zebra: '2.0.0' },
    name: 'example',
    scripts: { test: 'echo test' },
  })
  const expected = `# project
name: example

dependencies:
  # runtime dependencies
  zebra: '2.0.0' # pinned
  alpha: 1.0.0
  beta: 2.0.0

scripts:
  test: echo test # test command
`
  expect(await readFile(file, 'utf8')).toBe(expected)
  await writeProjectManifest(file, {
    name: 'example',
    dependencies: { alpha: '1.0.0', beta: '2.0.0', zebra: '3.0.0' },
    scripts: { test: 'echo test' },
  })
  expect(await readFile(file, 'utf8')).toBe(expected.replace("'2.0.0'", "'3.0.0'"))
})

test('preserves YAML metadata with null values, empty collections, and aliases', async () => {
  const file = path.join(temporaryDirectory(), 'package.yaml')
  const original = `name: example
metadata:
  empty: {}
  missing: null
  list: [null, {}, []]
  labels:
    1: first # numeric key
    true: enabled
    null: empty
dependencies:
  zebra: &version 1.0.0 # shared version
  alpha: *version
`
  await fs.promises.writeFile(file, original)
  const manifest = yaml.parse(original)
  manifest.version = '2.0.0'
  await writeProjectManifest(file, manifest)
  const result = await readFile(file, 'utf8')
  expect(yaml.parse(result)).toStrictEqual(manifest)
  expect(result).toContain('zebra: &version 1.0.0 # shared version')
  expect(result).toContain('alpha: *version')
  expect(result).toContain('1: first # numeric key')
})

test('does not overwrite invalid YAML', async () => {
  const file = path.join(temporaryDirectory(), 'package.yaml')
  const original = 'dependencies: [\n'
  await fs.promises.writeFile(file, original)
  await expect(writeProjectManifest(file, { name: 'example' })).rejects.toMatchObject({
    code: 'ERR_PNPM_YAML_PARSE',
    message: expect.stringContaining(file),
  })
  expect(await readFile(file, 'utf8')).toBe(original)
})

test('creates a YAML manifest in a missing directory', async () => {
  const file = path.join(temporaryDirectory(), 'nested', 'package.yaml')
  await writeProjectManifest(file, { name: 'example' })
  expect(await readFile(file, 'utf8')).toBe('name: example\n')
})

test('preserves CRLF line endings when crlf option is true', async () => {
  const dir = temporaryDirectory()

  await writeProjectManifest(path.join(dir, 'package.json'), { name: 'foo', version: '1.0.0' }, { crlf: true })
  expect(await readFile(path.join(dir, 'package.json'), 'utf8')).toBe('{\r\n\t"name": "foo",\r\n\t"version": "1.0.0"\r\n}\r\n')

  await writeProjectManifest(path.join(dir, 'package.json5'), { name: 'foo', version: '1.0.0' }, { crlf: true })
  expect(await readFile(path.join(dir, 'package.json5'), 'utf8')).toBe("{\r\n\tname: 'foo',\r\n\tversion: '1.0.0',\r\n}\r\n")

  const yamlPath = path.join(dir, 'package.yaml')
  await fs.promises.writeFile(yamlPath, 'name: foo\r\nversion: 1.0.0\r\n')
  await writeProjectManifest(yamlPath, { name: 'foo', version: '2.0.0' })
  expect(await readFile(yamlPath, 'utf8')).toBe('name: foo\r\nversion: 2.0.0\r\n')
})
