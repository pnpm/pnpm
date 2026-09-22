/// <reference path="../../../__typings__/index.d.ts"/>
import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { fixtures } from '@pnpm/test-fixtures'
import type { ProjectManifest } from '@pnpm/types'
import { readExactProjectManifest, readExactProjectManifestSync, readProjectManifest, tryReadProjectManifest } from '@pnpm/workspace.project-manifest-reader'
import { temporaryDirectory } from 'tempy'

const f = fixtures(import.meta.dirname)

test.each([
  'package-json/package.json',
  'package-json5/package.json5',
  'package-yaml/package.yaml',
])('readExactProjectManifestSync() reads %s', (manifestPath) => {
  expect(readExactProjectManifestSync(f.find(manifestPath)).manifest).toStrictEqual({
    name: 'foo',
    version: '1.0.0',
  })
})

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

test('readProjectManifest() converts engines runtime to dependencies', async () => {
  const dir = f.prepare('package-json-with-engines')
  const { manifest, writeProjectManifest } = await tryReadProjectManifest(dir)
  expect(manifest).toStrictEqual(
    {
      dependencies: {
        node: 'runtime:24',
      },
      engines: {
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
    dependencies: {},
    engines: {
      runtime: {
        name: 'node',
        version: '24',
        onFail: 'download',
      },
    },
  })
})

test('writeProjectManifest() removes a single devEngines runtime when its dependency was removed', async () => {
  const dir = f.prepare('package-json-with-dev-engines')
  const { manifest, writeProjectManifest } = await tryReadProjectManifest(dir)

  delete manifest!.devDependencies!.node
  await writeProjectManifest(manifest!)

  const pkgJson = JSON.parse(fs.readFileSync(path.join(dir, 'package.json'), 'utf8'))
  expect(pkgJson).toStrictEqual({
    devEngines: {},
  })
})

test('writeProjectManifest() removes an empty-version devEngines runtime when its dependency was removed', async () => {
  const dir = f.prepare('package-json')
  fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify({
    devDependencies: {
      node: 'runtime:',
    },
    devEngines: {
      runtime: {
        name: 'node',
        version: '',
        onFail: 'download',
      },
    },
  }))
  const { manifest, writeProjectManifest } = await tryReadProjectManifest(dir)

  delete manifest!.devDependencies!.node
  await writeProjectManifest(manifest!)

  const pkgJson = JSON.parse(fs.readFileSync(path.join(dir, 'package.json'), 'utf8'))
  expect(pkgJson).toStrictEqual({
    devEngines: {},
  })
})

test('writeProjectManifest() rejects malformed dependency fields without pruning runtime entries', async () => {
  const dir = f.prepare('package-json-with-dev-engines')
  const manifestPath = path.join(dir, 'package.json')
  const raw = fs.readFileSync(manifestPath, 'utf8')
  const { manifest, writeProjectManifest } = await tryReadProjectManifest(dir)
  const invalidManifest = manifest as unknown as Record<string, unknown>
  invalidManifest.devDependencies = []

  await expect(writeProjectManifest(invalidManifest as unknown as ProjectManifest)).rejects.toMatchObject({
    code: 'ERR_PNPM_INVALID_DEPENDENCIES_FIELD',
  })
  expect(fs.readFileSync(manifestPath, 'utf8')).toBe(raw)
})

test('writeProjectManifest() removes only the removed runtime from a devEngines runtime array', async () => {
  const dir = f.prepare('package-json')
  fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify({
    devEngines: {
      runtime: [
        {
          name: 'node',
          version: '24',
          onFail: 'download',
        },
        {
          name: 'deno',
          version: '2',
          onFail: 'download',
        },
      ],
    },
  }))
  const { manifest, writeProjectManifest } = await tryReadProjectManifest(dir)

  delete manifest!.devDependencies!.node
  await writeProjectManifest(manifest!)

  const pkgJson = JSON.parse(fs.readFileSync(path.join(dir, 'package.json'), 'utf8'))
  expect(pkgJson).toStrictEqual({
    devDependencies: {},
    devEngines: {
      runtime: [
        {
          name: 'deno',
          version: '2',
          onFail: 'download',
        },
      ],
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
    name: 'trims a whitespace-only devDependency runtime selector',
    manifest: {
      devDependencies: {
        node: 'runtime:  ',
      },
    },
    expected: {
      runtime: {
        name: 'node',
        version: '',
        onFail: 'download',
      },
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
    devDependencies: {},
    peerDependencies: {},
  })

  const stat2 = fs.statSync('package.json5')

  expect(stat1.ino).toBe(stat2.ino)
})

test('writeProjectManifest() keeps a dependency field that was already empty on read', async () => {
  process.chdir(temporaryDirectory())

  fs.writeFileSync('package.json', '{\n  "name": "foo",\n  "peerDependencies": {}\n}\n', 'utf8')

  const { manifest, writeProjectManifest } = await readProjectManifest(process.cwd())

  await writeProjectManifest({ ...manifest, dependencies: { bar: '1.0.0' } })

  expect(fs.readFileSync('package.json', 'utf8')).toBe('{\n  "name": "foo",\n  "peerDependencies": {},\n  "dependencies": {\n    "bar": "1.0.0"\n  }\n}\n')
})

test('writeProjectManifest() drops a dependency field the write emptied', async () => {
  process.chdir(temporaryDirectory())

  fs.writeFileSync('package.json', '{\n  "name": "foo",\n  "dependencies": {\n    "bar": "1.0.0"\n  },\n  "peerDependencies": {}\n}\n', 'utf8')

  const { manifest, writeProjectManifest } = await readProjectManifest(process.cwd())

  await writeProjectManifest({ ...manifest, dependencies: {} })

  expect(fs.readFileSync('package.json', 'utf8')).toBe('{\n  "name": "foo",\n  "peerDependencies": {}\n}\n')
})

test('writeProjectManifest() keeps a dependency field that was already empty on read when engines runtime is present', async () => {
  process.chdir(temporaryDirectory())

  fs.writeFileSync('package.json', JSON.stringify({
    name: 'foo',
    dependencies: {},
    engines: {
      runtime: {
        name: 'node',
        version: '24.6.0',
        onFail: 'download',
      },
    },
  }, null, 2) + '\n', 'utf8')

  const { manifest, writeProjectManifest } = await readProjectManifest(process.cwd())

  await writeProjectManifest({ ...manifest, devDependencies: { bar: '1.0.0' } })

  expect(JSON.parse(fs.readFileSync('package.json', 'utf8'))).toStrictEqual({
    name: 'foo',
    dependencies: {},
    devDependencies: {
      bar: '1.0.0',
    },
    engines: {
      runtime: {
        name: 'node',
        version: '24.6.0',
        onFail: 'download',
      },
    },
  })
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

test.each(['directory', 'exact'])('preserves package.yaml comments and dependency order through the %s reader', async (reader) => {
  const dir = temporaryDirectory()
  const file = path.join(dir, 'package.yaml')
  const original = `name: example
dependencies:
  # runtime dependencies
  zebra: 1.0.0 # pinned
  alpha: 1.0.0
`
  await fs.promises.writeFile(file, original)
  const { manifest, writeProjectManifest } = reader === 'exact'
    ? await readExactProjectManifest(file)
    : await readProjectManifest(dir)
  manifest.dependencies!.zebra = '2.0.0'
  manifest.dependencies!.beta = '1.0.0'
  await writeProjectManifest(manifest)
  expect(await fs.promises.readFile(file, 'utf8')).toBe(original.replace('zebra: 1.0.0', 'zebra: 2.0.0') + '  beta: 1.0.0\n')
  delete manifest.dependencies!.alpha
  await writeProjectManifest(manifest)
  expect(await fs.promises.readFile(file, 'utf8')).toBe(original.replace('zebra: 1.0.0', 'zebra: 2.0.0').replace('  alpha: 1.0.0\n', '') + '  beta: 1.0.0\n')
})

test('preserves comments on legacy YAML scalar keys and metadata values', async () => {
  const dir = temporaryDirectory()
  const file = path.join(dir, 'package.yaml')
  const original = `name: example
metadata:
  null: empty # null key
  date: 2020-01-01 # timestamp value
  data: !!binary SGVsbG8= # binary value
`
  await fs.promises.writeFile(file, original)
  const { manifest, writeProjectManifest } = await readProjectManifest(dir)
  await writeProjectManifest({ ...manifest, version: '1.0.0' })
  const result = await fs.promises.readFile(file, 'utf8')
  expect(result).toContain('null: empty # null key')
  expect(result).toContain('# timestamp value')
  expect(result).toContain('# binary value')
  expect((await readProjectManifest(dir)).manifest).toStrictEqual({ ...manifest, version: '1.0.0' })
})

test('preserve CRLF line endings in package.json and package.json5', async () => {
  const dir = temporaryDirectory()
  const jsonPath = path.join(dir, 'package.json')
  await fs.promises.writeFile(jsonPath, '{\r\n\t"name": "foo",\r\n\t"version": "1.0.0"\r\n}\r\n')
  const { manifest, writeProjectManifest } = await readProjectManifest(dir)
  manifest.version = '2.0.0'
  await writeProjectManifest(manifest)
  expect(await fs.promises.readFile(jsonPath, 'utf8')).toBe('{\r\n\t"name": "foo",\r\n\t"version": "2.0.0"\r\n}\r\n')

  const dir5 = temporaryDirectory()
  const json5Path = path.join(dir5, 'package.json5')
  await fs.promises.writeFile(json5Path, "{\r\n\tname: 'foo',\r\n\tversion: '1.0.0',\r\n}\r\n")
  const reader5 = await readProjectManifest(dir5)
  reader5.manifest.version = '2.0.0'
  await reader5.writeProjectManifest(reader5.manifest)
  expect(await fs.promises.readFile(json5Path, 'utf8')).toBe("{\r\n\tname: 'foo',\r\n\tversion: '2.0.0',\r\n}\r\n")
})

test('readProjectManifest() succeeds with malformed dependency fields and allows writing corrected manifest', async () => {
  const dir = temporaryDirectory()
  const jsonPath = path.join(dir, 'package.json')
  await fs.promises.writeFile(jsonPath, JSON.stringify({
    name: 'test-package',
    dependencies: 'invalid',
  }, null, 2) + '\n')

  const { manifest, writeProjectManifest } = await readProjectManifest(dir)
  const m = manifest as Record<string, unknown>
  delete m.dependencies

  await writeProjectManifest(m as ProjectManifest)
  expect(JSON.parse(await fs.promises.readFile(jsonPath, 'utf8'))).toStrictEqual({
    name: 'test-package',
  })
})

