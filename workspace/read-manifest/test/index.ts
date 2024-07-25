import { readWorkspaceManifest } from '@pnpm/workspace.read-manifest'
import path from 'node:path'

test('readWorkspaceManifest() works with a valid workspace file', async () => {
  const manifest = await readWorkspaceManifest(path.join(__dirname, '__fixtures__/ok'))

  expect(manifest).toEqual({
    packages: ['packages/**', 'types'],
  })
})

test('readWorkspaceManifest() throws on string content', async () => {
  await expect(
    readWorkspaceManifest(path.join(__dirname, '__fixtures__/string'))
  ).rejects.toThrow('Expected object but found - string')
})

test('readWorkspaceManifest() throws on array content', async () => {
  await expect(
    readWorkspaceManifest(path.join(__dirname, '__fixtures__/array'))
  ).rejects.toThrow('Expected object but found - array')
})

test('readWorkspaceManifest() throws on empty packages field', async () => {
  await expect(
    readWorkspaceManifest(path.join(__dirname, '__fixtures__/packages-empty'))
  ).rejects.toThrow('packages field missing or empty')
})

test('readWorkspaceManifest() throws on string packages field', async () => {
  await expect(
    readWorkspaceManifest(path.join(__dirname, '__fixtures__/packages-string'))
  ).rejects.toThrow('packages field is not an array')
})

test('readWorkspaceManifest() throws on empty package', async () => {
  await expect(
    readWorkspaceManifest(path.join(__dirname, '__fixtures__/packages-contains-empty'))
  ).rejects.toThrow('Missing or empty package')
})

test('readWorkspaceManifest() throws on numeric package', async () => {
  await expect(
    readWorkspaceManifest(path.join(__dirname, '__fixtures__/packages-contains-number'))
  ).rejects.toThrow('Invalid package type - number')
})

test('readWorkspaceManifest() works when no workspace file is present', async () => {
  const manifest = await readWorkspaceManifest(path.join(__dirname, '__fixtures__/no-workspace-file'))

  expect(manifest).toBeUndefined()
})

test('readWorkspaceManifest() works when workspace file is empty', async () => {
  const manifest = await readWorkspaceManifest(path.join(__dirname, '__fixtures__/empty'))

  expect(manifest).toBeUndefined()
})

test('readWorkspaceManifest() works when workspace file is null', async () => {
  const manifest = await readWorkspaceManifest(path.join(__dirname, '__fixtures__/null'))

  expect(manifest).toBeNull()
})

describe('readWorkspaceManifest() catalog field', () => {
  test('works on simple catalog', async () => {
    await expect(readWorkspaceManifest(path.join(__dirname, '__fixtures__/catalog-ok'))).resolves.toEqual({
      packages: ['packages/**', 'types'],
      catalog: {
        foo: '^1.0.0',
      },
    })
  })

  test('throws on invalid array', async () => {
    await expect(
      readWorkspaceManifest(path.join(__dirname, '__fixtures__/catalog-invalid-array'))
    ).rejects.toThrow('Expected catalog field to be an object, but found - array')
  })

  test('throws on invalid object', async () => {
    await expect(
      readWorkspaceManifest(path.join(__dirname, '__fixtures__/catalog-invalid-object'))
    ).rejects.toThrow('Expected catalog field to be an object, but found - number')
  })

  test('throws on invalid specifier', async () => {
    await expect(
      readWorkspaceManifest(path.join(__dirname, '__fixtures__/catalog-invalid-specifier'))
    ).rejects.toThrow('Invalid catalog entry for foo. Expected string, but found: object')
  })
})

describe('readWorkspaceManifest() catalogs field', () => {
  test('works with simple named catalogs', async () => {
    await expect(readWorkspaceManifest(path.join(__dirname, '__fixtures__/catalogs-ok'))).resolves.toEqual({
      packages: ['packages/**', 'types'],
      catalog: {
        bar: '^1.0.0',
      },
      catalogs: {
        foo: {
          bar: '^2.0.0',
        },
      },
    })
  })

  test('throws on invalid array', async () => {
    await expect(
      readWorkspaceManifest(path.join(__dirname, '__fixtures__/catalogs-invalid-array'))
    ).rejects.toThrow('Expected catalogs field to be an object, but found - array')
  })

  test('throws on invalid value', async () => {
    await expect(
      readWorkspaceManifest(path.join(__dirname, '__fixtures__/catalogs-invalid-object'))
    ).rejects.toThrow('Expected catalogs field to be an object, but found - number')
  })

  test('throws on invalid named catalog array', async () => {
    await expect(
      readWorkspaceManifest(path.join(__dirname, '__fixtures__/catalogs-invalid-named-catalog-array'))
    ).rejects.toThrow('Expected named catalog foo to be an object, but found - array')
  })

  test('throws on invalid named catalog object', async () => {
    await expect(
      readWorkspaceManifest(path.join(__dirname, '__fixtures__/catalogs-invalid-named-catalog-object'))
    ).rejects.toThrow('Expected named catalog foo to be an object, but found - number')
  })

  test('throws on invalid named catalog specifier', async () => {
    await expect(
      readWorkspaceManifest(path.join(__dirname, '__fixtures__/catalogs-invalid-named-catalog-specifier'))
    ).rejects.toThrow('Catalog \'foo\' has invalid entry \'bar\'. Expected string specifier, but found: object')
  })
})

describe('readWorkspaceManifest() reads default catalog defined alongside named catalogs', () => {
  test('works when implicit default catalog is configured alongside named catalogs', async () => {
    await expect(readWorkspaceManifest(path.join(__dirname, '__fixtures__/catalogs-ok'))).resolves.toEqual({
      packages: ['packages/**', 'types'],
      catalog: {
        bar: '^1.0.0',
      },
      catalogs: {
        foo: {
          bar: '^2.0.0',
        },
      },
    })
  })
})
