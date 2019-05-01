import readImporterManifest, { tryReadImporterManifest } from '@pnpm/read-importer-manifest'
import fs = require('graceful-fs')
import path = require('path')
import test = require('tape')
import tempy = require('tempy')
import { promisify } from 'util'

const writeFile = promisify(fs.writeFile)
const readFile = promisify(fs.readFile)

const fixtures = path.join(__dirname, 'fixtures')

test('readImporterManifest()', async (t) => {
  t.deepEqual(
    (await tryReadImporterManifest(path.join(fixtures, 'package-json'))).manifest,
    { name: 'foo', version: '1.0.0' },
  )

  t.deepEqual(
    (await tryReadImporterManifest(path.join(fixtures, 'package-json5'))).manifest,
    { name: 'foo', version: '1.0.0' },
  )

  t.deepEqual(
    (await tryReadImporterManifest(path.join(fixtures, 'package-yaml'))).manifest,
    { name: 'foo', version: '1.0.0' },
  )

  t.deepEqual(
    (await tryReadImporterManifest(fixtures)).manifest,
    null,
  )

  t.end()
})

test('preserve tab indentation in json file', async (t) => {
  process.chdir(tempy.directory())

  await writeFile('package.json', '{\n\t"name": "foo"\n}\n', 'utf8')

  const { manifest, writeImporterManifest } = await readImporterManifest(process.cwd())

  await writeImporterManifest({ ...manifest, dependencies: { bar: '1.0.0' } })

  const rawManifest = await readFile('package.json', 'utf8')
  t.equal(rawManifest, '{\n\t"name": "foo",\n\t"dependencies": {\n\t\t"bar": "1.0.0"\n\t}\n}\n')
  t.end()
})

test('preserve space indentation in json file', async (t) => {
  process.chdir(tempy.directory())

  await writeFile('package.json', '{\n  "name": "foo"\n}\n', 'utf8')

  const { manifest, writeImporterManifest } = await readImporterManifest(process.cwd())

  await writeImporterManifest({ ...manifest, dependencies: { bar: '1.0.0' } })

  const rawManifest = await readFile('package.json', 'utf8')
  t.equal(rawManifest, '{\n  "name": "foo",\n  "dependencies": {\n    "bar": "1.0.0"\n  }\n}\n')
  t.end()
})

test('preserve tab indentation in json5 file', async (t) => {
  process.chdir(tempy.directory())

  await writeFile('package.json5', "{\n\tname: 'foo',\n}\n", 'utf8')

  const { manifest, writeImporterManifest } = await readImporterManifest(process.cwd())

  await writeImporterManifest({ ...manifest, dependencies: { bar: '1.0.0' } })

  const rawManifest = await readFile('package.json5', 'utf8')
  t.equal(rawManifest, "{\n\tname: 'foo',\n\tdependencies: {\n\t\tbar: '1.0.0',\n\t},\n}\n")
  t.end()
})

test('preserve space indentation in json5 file', async (t) => {
  process.chdir(tempy.directory())

  await writeFile('package.json5', "{\n  name: 'foo'\n}\n", 'utf8')

  const { manifest, writeImporterManifest } = await readImporterManifest(process.cwd())

  await writeImporterManifest({ ...manifest, dependencies: { bar: '1.0.0' } })

  const rawManifest = await readFile('package.json5', 'utf8')
  t.equal(rawManifest, "{\n  name: 'foo',\n  dependencies: {\n    bar: '1.0.0',\n  },\n}\n")
  t.end()
})
