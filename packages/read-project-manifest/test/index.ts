/// <reference path="../../../typings/index.d.ts"/>
import readProjectManifest, { tryReadProjectManifest } from '@pnpm/read-project-manifest'
import { promises as fs } from 'fs'
import path = require('path')
import tempy = require('tempy')

const fixtures = path.join(__dirname, '../fixtures')

test('readProjectManifest()', async () => {
  expect(
    (await tryReadProjectManifest(path.join(fixtures, 'package-json'))).manifest
  ).toStrictEqual(
    { name: 'foo', version: '1.0.0' }
  )

  expect(
    (await tryReadProjectManifest(path.join(fixtures, 'package-json5'))).manifest
  ).toStrictEqual(
    { name: 'foo', version: '1.0.0' }
  )

  expect(
    (await tryReadProjectManifest(path.join(fixtures, 'package-yaml'))).manifest
  ).toStrictEqual(
    { name: 'foo', version: '1.0.0' }
  )

  expect(
    (await tryReadProjectManifest(fixtures)).manifest
  ).toStrictEqual(null)
})

test('preserve tab indentation in json file', async () => {
  process.chdir(tempy.directory())

  await fs.writeFile('package.json', '{\n\t"name": "foo"\n}\n', 'utf8')

  const { manifest, writeProjectManifest } = await readProjectManifest(process.cwd())

  await writeProjectManifest({ ...manifest, dependencies: { bar: '1.0.0' } })

  const rawManifest = await fs.readFile('package.json', 'utf8')
  expect(rawManifest).toBe('{\n\t"name": "foo",\n\t"dependencies": {\n\t\t"bar": "1.0.0"\n\t}\n}\n')
})

test('preserve space indentation in json file', async () => {
  process.chdir(tempy.directory())

  await fs.writeFile('package.json', '{\n  "name": "foo"\n}\n', 'utf8')

  const { manifest, writeProjectManifest } = await readProjectManifest(process.cwd())

  await writeProjectManifest({ ...manifest, dependencies: { bar: '1.0.0' } })

  const rawManifest = await fs.readFile('package.json', 'utf8')
  expect(rawManifest).toBe('{\n  "name": "foo",\n  "dependencies": {\n    "bar": "1.0.0"\n  }\n}\n')
})

test('preserve tab indentation in json5 file', async () => {
  process.chdir(tempy.directory())

  await fs.writeFile('package.json5', "{\n\tname: 'foo',\n}\n", 'utf8')

  const { manifest, writeProjectManifest } = await readProjectManifest(process.cwd())

  await writeProjectManifest({ ...manifest, dependencies: { bar: '1.0.0' } })

  const rawManifest = await fs.readFile('package.json5', 'utf8')
  expect(rawManifest).toBe("{\n\tname: 'foo',\n\tdependencies: {\n\t\tbar: '1.0.0',\n\t},\n}\n")
})

test('preserve space indentation in json5 file', async () => {
  process.chdir(tempy.directory())

  await fs.writeFile('package.json5', "{\n  name: 'foo'\n}\n", 'utf8')

  const { manifest, writeProjectManifest } = await readProjectManifest(process.cwd())

  await writeProjectManifest({ ...manifest, dependencies: { bar: '1.0.0' } })

  const rawManifest = await fs.readFile('package.json5', 'utf8')
  expect(rawManifest).toBe("{\n  name: 'foo',\n  dependencies: {\n    bar: '1.0.0',\n  },\n}\n")
})

test('do not save manifest if it had no changes', async () => {
  process.chdir(tempy.directory())

  await fs.writeFile(
    'package.json5',
    JSON.stringify({
      dependencies: { foo: '*', bar: '*' },
      devDependencies: {},
    }),
    'utf8'
  )

  const { writeProjectManifest } = await readProjectManifest(process.cwd())

  const stat1 = await fs.stat('package.json5')

  await writeProjectManifest({
    dependencies: { bar: '*', foo: '*' },
    peerDependencies: {},
  })

  const stat2 = await fs.stat('package.json5')

  expect(stat1.ino).toBe(stat2.ino)
})

test('fail on invalid JSON', async () => {
  let err!: Error
  try {
    await readProjectManifest(path.join(fixtures, 'invalid-package-json'))
  } catch (_err) {
    err = _err
  }

  expect(err).toBeTruthy()
  expect(err['code']).toBe('ERR_PNPM_JSON_PARSE')
  // eslint-disable-next-line
  expect(err.message).toMatch(/^Unexpected string in JSON at position 20 while parsing \'{  "name": "foo"  "version": "1.0.0"}\' in /)
})

test('fail on invalid JSON5', async () => {
  let err!: Error
  try {
    await readProjectManifest(path.join(fixtures, 'invalid-package-json5'))
  } catch (_err) {
    err = _err
  }

  expect(err).toBeTruthy()
  expect(err['code']).toBe('ERR_PNPM_JSON5_PARSE')
  expect(err.message).toMatch(/^JSON5: invalid character 'v' at 3:3 in/)
})

test('fail on invalid YAML', async () => {
  let err!: Error
  try {
    await readProjectManifest(path.join(fixtures, 'invalid-package-yaml'))
  } catch (_err) {
    err = _err
  }

  expect(err).toBeTruthy()
  expect(err['code']).toBe('ERR_PNPM_YAML_PARSE')
  expect(err.message).toMatch(/^missed comma between flow collection entries at line 3, column 3:/)
})

test('preserve trailing new line at the end of package.json', async () => {
  process.chdir(tempy.directory())

  await fs.writeFile('package.json', '{}', 'utf8')

  const { manifest, writeProjectManifest } = await readProjectManifest(process.cwd())

  await writeProjectManifest({ ...manifest, dependencies: { bar: '1.0.0' } })

  const rawManifest = await fs.readFile('package.json', 'utf8')
  expect(rawManifest).toBe('{"dependencies":{"bar":"1.0.0"}}')
})

test('preserve trailing new line at the end of package.json5', async () => {
  process.chdir(tempy.directory())

  await fs.writeFile('package.json5', '{}', 'utf8')

  const { manifest, writeProjectManifest } = await readProjectManifest(process.cwd())

  await writeProjectManifest({ ...manifest, dependencies: { bar: '1.0.0' } })

  const rawManifest = await fs.readFile('package.json5', 'utf8')
  expect(rawManifest).toBe("{dependencies:{bar:'1.0.0'}}")
})
