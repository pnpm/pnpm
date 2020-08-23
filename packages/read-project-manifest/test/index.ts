/// <reference path="../../../typings/index.d.ts"/>
import { promisify } from 'util'
import readProjectManifest, { tryReadProjectManifest } from '@pnpm/read-project-manifest'
import path = require('path')
import fs = require('graceful-fs')
import test = require('tape')
import tempy = require('tempy')

const writeFile = promisify(fs.writeFile)
const readFile = promisify(fs.readFile)
const stat = promisify(fs.stat)

const fixtures = path.join(__dirname, '../fixtures')

test('readProjectManifest()', async (t) => {
  t.deepEqual(
    (await tryReadProjectManifest(path.join(fixtures, 'package-json'))).manifest,
    { name: 'foo', version: '1.0.0' }
  )

  t.deepEqual(
    (await tryReadProjectManifest(path.join(fixtures, 'package-json5'))).manifest,
    { name: 'foo', version: '1.0.0' }
  )

  t.deepEqual(
    (await tryReadProjectManifest(path.join(fixtures, 'package-yaml'))).manifest,
    { name: 'foo', version: '1.0.0' }
  )

  t.deepEqual(
    (await tryReadProjectManifest(fixtures)).manifest,
    null
  )

  t.end()
})

test('preserve tab indentation in json file', async (t) => {
  process.chdir(tempy.directory())

  await writeFile('package.json', '{\n\t"name": "foo"\n}\n', 'utf8')

  const { manifest, writeProjectManifest } = await readProjectManifest(process.cwd())

  await writeProjectManifest({ ...manifest, dependencies: { bar: '1.0.0' } })

  const rawManifest = await readFile('package.json', 'utf8')
  t.equal(rawManifest, '{\n\t"name": "foo",\n\t"dependencies": {\n\t\t"bar": "1.0.0"\n\t}\n}\n')
  t.end()
})

test('preserve space indentation in json file', async (t) => {
  process.chdir(tempy.directory())

  await writeFile('package.json', '{\n  "name": "foo"\n}\n', 'utf8')

  const { manifest, writeProjectManifest } = await readProjectManifest(process.cwd())

  await writeProjectManifest({ ...manifest, dependencies: { bar: '1.0.0' } })

  const rawManifest = await readFile('package.json', 'utf8')
  t.equal(rawManifest, '{\n  "name": "foo",\n  "dependencies": {\n    "bar": "1.0.0"\n  }\n}\n')
  t.end()
})

test('preserve tab indentation in json5 file', async (t) => {
  process.chdir(tempy.directory())

  await writeFile('package.json5', "{\n\tname: 'foo',\n}\n", 'utf8')

  const { manifest, writeProjectManifest } = await readProjectManifest(process.cwd())

  await writeProjectManifest({ ...manifest, dependencies: { bar: '1.0.0' } })

  const rawManifest = await readFile('package.json5', 'utf8')
  t.equal(rawManifest, "{\n\tname: 'foo',\n\tdependencies: {\n\t\tbar: '1.0.0',\n\t},\n}\n")
  t.end()
})

test('preserve space indentation in json5 file', async (t) => {
  process.chdir(tempy.directory())

  await writeFile('package.json5', "{\n  name: 'foo'\n}\n", 'utf8')

  const { manifest, writeProjectManifest } = await readProjectManifest(process.cwd())

  await writeProjectManifest({ ...manifest, dependencies: { bar: '1.0.0' } })

  const rawManifest = await readFile('package.json5', 'utf8')
  t.equal(rawManifest, "{\n  name: 'foo',\n  dependencies: {\n    bar: '1.0.0',\n  },\n}\n")
  t.end()
})

test('do not save manifest if it had no changes', async (t) => {
  process.chdir(tempy.directory())

  await writeFile(
    'package.json5',
    JSON.stringify({
      dependencies: { foo: '*', bar: '*' },
      devDependencies: {},
    }),
    'utf8'
  )

  const { writeProjectManifest } = await readProjectManifest(process.cwd())

  const stat1 = await stat('package.json5')

  await writeProjectManifest({
    dependencies: { bar: '*', foo: '*' },
    peerDependencies: {},
  })

  const stat2 = await stat('package.json5')

  t.deepEqual(stat1.ino, stat2.ino, 'manifest was not resaved')

  t.end()
})

test('fail on invalid JSON', async (t) => {
  let err!: Error
  try {
    await readProjectManifest(path.join(fixtures, 'invalid-package-json'))
  } catch (_err) {
    err = _err
  }

  t.ok(err)
  t.equal(err['code'], 'ERR_PNPM_JSON_PARSE')
  t.ok(err.message.startsWith('Unexpected string in JSON at position 20 while parsing \'{  "name": "foo"  "version": "1.0.0"}\' in '))

  t.end()
})

test('fail on invalid JSON5', async (t) => {
  let err!: Error
  try {
    await readProjectManifest(path.join(fixtures, 'invalid-package-json5'))
  } catch (_err) {
    err = _err
  }

  t.ok(err)
  t.equal(err['code'], 'ERR_PNPM_JSON5_PARSE')
  t.ok(err.message.startsWith("JSON5: invalid character 'v' at 3:3 in"))

  t.end()
})

test('fail on invalid YAML', async (t) => {
  let err!: Error
  try {
    await readProjectManifest(path.join(fixtures, 'invalid-package-yaml'))
  } catch (_err) {
    err = _err
  }

  t.ok(err)
  t.equal(err['code'], 'ERR_PNPM_YAML_PARSE')
  t.ok(err.message.startsWith('missed comma between flow collection entries at line 3, column 3:'))

  t.end()
})

test('preserve trailing new line at the end of package.json', async (t) => {
  process.chdir(tempy.directory())

  await writeFile('package.json', '{}', 'utf8')

  const { manifest, writeProjectManifest } = await readProjectManifest(process.cwd())

  await writeProjectManifest({ ...manifest, dependencies: { bar: '1.0.0' } })

  const rawManifest = await readFile('package.json', 'utf8')
  t.equal(rawManifest, '{"dependencies":{"bar":"1.0.0"}}')
  t.end()
})

test('preserve trailing new line at the end of package.json5', async (t) => {
  process.chdir(tempy.directory())

  await writeFile('package.json5', '{}', 'utf8')

  const { manifest, writeProjectManifest } = await readProjectManifest(process.cwd())

  await writeProjectManifest({ ...manifest, dependencies: { bar: '1.0.0' } })

  const rawManifest = await readFile('package.json5', 'utf8')
  t.equal(rawManifest, "{dependencies:{bar:'1.0.0'}}")
  t.end()
})
