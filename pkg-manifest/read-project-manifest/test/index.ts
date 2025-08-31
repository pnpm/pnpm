/// <reference path="../../../__typings__/index.d.ts"/>
import fs from 'fs'
import path from 'path'
import { readProjectManifest, tryReadProjectManifest } from '@pnpm/read-project-manifest'
import { fixtures } from '@pnpm/test-fixtures'
import { temporaryDirectory } from 'tempy'
import { type ProjectManifest } from '@pnpm/types'

const f = fixtures(import.meta.dirname)

test('readProjectManifest()', async () => {
  expect(
    (await tryReadProjectManifest(f.find('package-json'))).manifest
  ).toStrictEqual(
    { name: 'foo', version: '1.0.0' }
  )

  expect(
    (await tryReadProjectManifest(f.find('package-json5'))).manifest
  ).toStrictEqual(
    { name: 'foo', version: '1.0.0' }
  )

  expect(
    (await tryReadProjectManifest(f.find('package-yaml'))).manifest
  ).toStrictEqual(
    { name: 'foo', version: '1.0.0' }
  )

  expect(
    (await tryReadProjectManifest(import.meta.dirname)).manifest
  ).toBeNull()
})

test('readProjectManifest() converts devEngines runtime to devDependencies', async () => {
  const dir = f.prepare('package-json-with-dev-engines')
  const { manifest, writeProjectManifest } = await tryReadProjectManifest(dir)
  expect(manifest).toStrictEqual(
    {
      devDependencies: {
        node: 'runtime:24',
      },
      devEngines: {
        runtime: {
          name: 'node',
          version: '24',
          onFail: 'download',
        },
      },
    }
  )
  await writeProjectManifest(manifest!)
  const pkgJson = JSON.parse(fs.readFileSync(path.join(dir, 'package.json'), 'utf8'))
  expect(pkgJson).toStrictEqual({
    devDependencies: {},
    devEngines: {
      runtime: {
        name: 'node',
        version: '24',
        onFail: 'download',
      },
    },
  })
})

test.each([
  {
    name: 'creates devEngines when it is missing',
    manifest: {
      devDependencies: {
        node: 'runtime:22',
      },
    },
    expected: {
      runtime: {
        name: 'node',
        version: '22',
        onFail: 'download',
      },
    },
  },
  {
    name: 'updates devEngines.runtime when it is a single node entry',
    manifest: {
      devEngines: {
        runtime: {
          name: 'node',
          version: '16',
        },
      },
      devDependencies: {
        node: 'runtime:22',
      },
    },
    expected: {
      runtime: {
        name: 'node',
        version: '22',
        onFail: 'download',
      },
    },
  },
  {
    name: 'converts devEngines.runtime to an array when it is a single non-node entry',
    manifest: {
      devEngines: {
        runtime: {
          name: 'deno',
          version: '1',
        },
      },
      devDependencies: {
        node: 'runtime:22',
      },
    },
    expected: {
      runtime: [
        {
          name: 'deno',
          version: '1',
        },
        {
          name: 'node',
          version: '22',
          onFail: 'download',
        },
      ],
    },
  },
  {
    name: 'updates devEngines.runtime when it is an array',
    manifest: {
      devEngines: {
        runtime: [
          {
            name: 'deno',
            version: '1',
          },
          {
            name: 'node',
            version: '16',
            onFail: 'download',
          },
        ],
      },
      devDependencies: {
        node: 'runtime:22',
      },
    },
    expected: {
      runtime: [
        {
          name: 'deno',
          version: '1',
        },
        {
          name: 'node',
          version: '22',
          onFail: 'download',
        },
      ],
    },
  },
])('readProjectManifest() converts devDependencies to devEngines: $name', async ({ manifest, expected }) => {
  const dir = f.prepare('package-json')

  const { writeProjectManifest } = await tryReadProjectManifest(dir)
  await writeProjectManifest(manifest as ProjectManifest)

  const pkgJson = JSON.parse(fs.readFileSync(path.join(dir, 'package.json'), 'utf8'))

  expect(pkgJson.devEngines).toStrictEqual(expected)
  expect(pkgJson.devDependencies).toStrictEqual({})
})

test('preserve tab indentation in json file', async () => {
  process.chdir(temporaryDirectory())

  fs.writeFileSync('package.json', '{\n\t"name": "foo"\n}\n', 'utf8')

  const { manifest, writeProjectManifest } = await readProjectManifest(process.cwd())

  await writeProjectManifest({ ...manifest, dependencies: { bar: '1.0.0' } })

  const rawManifest = fs.readFileSync('package.json', 'utf8')
  expect(rawManifest).toBe('{\n\t"name": "foo",\n\t"dependencies": {\n\t\t"bar": "1.0.0"\n\t}\n}\n')
})

test('preserve space indentation in json file', async () => {
  process.chdir(temporaryDirectory())

  fs.writeFileSync('package.json', '{\n  "name": "foo"\n}\n', 'utf8')

  const { manifest, writeProjectManifest } = await readProjectManifest(process.cwd())

  await writeProjectManifest({ ...manifest, dependencies: { bar: '1.0.0' } })

  const rawManifest = fs.readFileSync('package.json', 'utf8')
  expect(rawManifest).toBe('{\n  "name": "foo",\n  "dependencies": {\n    "bar": "1.0.0"\n  }\n}\n')
})

test('preserve tab indentation in json5 file', async () => {
  process.chdir(temporaryDirectory())

  fs.writeFileSync('package.json5', "{\n\tname: 'foo',\n}\n", 'utf8')

  const { manifest, writeProjectManifest } = await readProjectManifest(process.cwd())

  await writeProjectManifest({ ...manifest, dependencies: { bar: '1.0.0' } })

  const rawManifest = fs.readFileSync('package.json5', 'utf8')
  expect(rawManifest).toBe("{\n\tname: 'foo',\n\tdependencies: {\n\t\tbar: '1.0.0',\n\t},\n}\n")
})

test('preserve space indentation in json5 file', async () => {
  process.chdir(temporaryDirectory())

  fs.writeFileSync('package.json5', "{\n  name: 'foo'\n}\n", 'utf8')

  const { manifest, writeProjectManifest } = await readProjectManifest(process.cwd())

  await writeProjectManifest({ ...manifest, dependencies: { bar: '1.0.0' } })

  const rawManifest = fs.readFileSync('package.json5', 'utf8')
  expect(rawManifest).toBe("{\n  name: 'foo',\n  dependencies: {\n    bar: '1.0.0',\n  },\n}\n")
})

test('preserve comments in json5 file', async () => {
  const originalManifest = fs.readFileSync(
    f.find('commented-package-json5/package.json5'), 'utf8')
  const modifiedManifest = fs.readFileSync(
    f.find('commented-package-json5/modified.json5'), 'utf8')

  process.chdir(temporaryDirectory())
  fs.writeFileSync('package.json5', originalManifest, 'utf8')

  const { manifest, writeProjectManifest } = await readProjectManifest(process.cwd())

  // Have to make a change to get it to write anything:
  const newManifest = Object.assign({}, manifest, { type: 'commonjs' })

  await writeProjectManifest(newManifest)

  const resultingManifest = fs.readFileSync('package.json5', 'utf8')
  expect(resultingManifest).toBe(modifiedManifest)
})

test('do not save manifest if it had no changes', async () => {
  process.chdir(temporaryDirectory())

  fs.writeFileSync(
    'package.json5',
    JSON.stringify({
      dependencies: { foo: '*', bar: '*' },
      devDependencies: {},
    }),
    'utf8'
  )

  const { writeProjectManifest } = await readProjectManifest(process.cwd())

  const stat1 = fs.statSync('package.json5')

  await writeProjectManifest({
    dependencies: { bar: '*', foo: '*' },
    peerDependencies: {},
  })

  const stat2 = fs.statSync('package.json5')

  expect(stat1.ino).toBe(stat2.ino)
})

test('fail on invalid JSON', async () => {
  let err!: Error & { code: string }
  try {
    await readProjectManifest(f.find('invalid-package-json'))
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }

  expect(err).toBeTruthy()
  expect(err.code).toBe('ERR_PNPM_JSON_PARSE')
  expect(err.message).toContain('Expected \',\' or \'}\' after property value in JSON at position 20 ')
})

test('fail on invalid JSON5', async () => {
  let err!: Error & { code: string }
  try {
    await readProjectManifest(f.find('invalid-package-json5'))
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }

  expect(err).toBeTruthy()
  expect(err.code).toBe('ERR_PNPM_JSON5_PARSE')
  expect(err.message).toMatch(/^JSON5: invalid character 'v' at 3:3 in/)
})

test('fail on invalid YAML', async () => {
  let err!: Error & { code: string }
  try {
    await readProjectManifest(f.find('invalid-package-yaml'))
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }

  expect(err).toBeTruthy()
  expect(err.code).toBe('ERR_PNPM_YAML_PARSE')
  expect(err.message).toMatch(/^missed comma between flow collection entries \(3:3\)/)
})

test('preserve trailing new line at the end of package.json', async () => {
  process.chdir(temporaryDirectory())

  fs.writeFileSync('package.json', '{}', 'utf8')

  const { manifest, writeProjectManifest } = await readProjectManifest(process.cwd())

  await writeProjectManifest({ ...manifest, dependencies: { bar: '1.0.0' } })

  const rawManifest = fs.readFileSync('package.json', 'utf8')
  expect(rawManifest).toBe('{"dependencies":{"bar":"1.0.0"}}')
})

test('preserve trailing new line at the end of package.json5', async () => {
  process.chdir(temporaryDirectory())

  fs.writeFileSync('package.json5', '{}', 'utf8')

  const { manifest, writeProjectManifest } = await readProjectManifest(process.cwd())

  await writeProjectManifest({ ...manifest, dependencies: { bar: '1.0.0' } })

  const rawManifest = fs.readFileSync('package.json5', 'utf8')
  expect(rawManifest).toBe("{dependencies:{bar:'1.0.0'}}")
})

test('canceling changes to a manifest', async () => {
  process.chdir(temporaryDirectory())

  fs.writeFileSync('package.json', JSON.stringify({ name: 'foo' }), 'utf8')

  const { writeProjectManifest } = await readProjectManifest(process.cwd())

  await writeProjectManifest({ name: 'bar' })
  expect(fs.readFileSync('package.json', 'utf8')).toBe(JSON.stringify({ name: 'bar' }))

  await writeProjectManifest({ name: 'foo' })
  expect(fs.readFileSync('package.json', 'utf8')).toBe(JSON.stringify({ name: 'foo' }))
})
