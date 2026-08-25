import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { WORKSPACE_MANIFEST_FILENAME } from '@pnpm/constants'
import { prepare } from '@pnpm/prepare'
import { tempDir } from '@pnpm/prepare-temp-dir'
import { findPackages } from '@pnpm/workspace.projects-reader'
import { updateWorkspaceManifest } from '@pnpm/workspace.workspace-manifest-writer'

// The Rust CLI's workspace-manifest-writer edits single-line flow
// collections with a text splice rather than a document round-trip, so these
// expectations are the parity contract between the two writers. Its matching
// suite is `flow_style` in pnpm/crates/workspace-manifest-writer/src/tests.rs.
async function editManifest (original: string, opts: Parameters<typeof updateWorkspaceManifest>[1]): Promise<string> {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  fs.writeFileSync(filePath, original)
  await updateWorkspaceManifest(dir, opts)
  return fs.readFileSync(filePath, 'utf8')
}

test('a catalog entry is added to a flow mapping', async () => {
  const out = await editManifest('catalog: { foo: ^1.0.0 }\n', {
    updatedCatalogs: { default: { bar: '^2.0.0' } },
  })
  expect(out).toBe('catalog: { bar: ^2.0.0, foo: ^1.0.0 }\n')
})

test('a catalog entry is updated in a flow mapping, keeping its trailing comment', async () => {
  const out = await editManifest('catalog: { foo: ^1.0.0 } # pins\n', {
    updatedCatalogs: { default: { foo: '^2.0.0' } },
  })
  expect(out).toBe('catalog: { foo: ^2.0.0 } # pins\n')
})

test('a named catalog entry is added to a nested flow mapping', async () => {
  const out = await editManifest('catalogs: { myCatalog: { foo: ^1.0.0 } }\n', {
    updatedCatalogs: { myCatalog: { bar: '^2.0.0' } },
  })
  expect(out).toBe('catalogs: { myCatalog: { bar: ^2.0.0, foo: ^1.0.0 } }\n')
})

test('a new named catalog is added to a flow catalogs mapping', async () => {
  const out = await editManifest('catalogs: { myCatalog: { foo: ^1.0.0 } }\n', {
    updatedCatalogs: { newCatalog: { bar: '^2.0.0' } },
  })
  expect(out).toBe('catalogs: { myCatalog: { foo: ^1.0.0 }, newCatalog: { bar: ^2.0.0 } }\n')
})

test('a config dependency is added to a flow mapping', async () => {
  const out = await editManifest('configDependencies: { foo: 1.0.0 }\n', {
    updatedFields: { configDependencies: { foo: '1.0.0', bar: '2.0.0' } },
  })
  expect(out).toBe('configDependencies: { bar: 2.0.0, foo: 1.0.0 }\n')
})

test('an allowBuilds entry is added to a flow mapping', async () => {
  const out = await editManifest('allowBuilds: { foo: true }\n', {
    updatedFields: { allowBuilds: { foo: true, bar: false } },
  })
  expect(out).toBe('allowBuilds: { bar: false, foo: true }\n')
})

test('an allowBuilds entry is updated in a flow mapping', async () => {
  const out = await editManifest('allowBuilds: { foo: true }\n', {
    updatedFields: { allowBuilds: { foo: false } },
  })
  expect(out).toBe('allowBuilds: { foo: false }\n')
})

test('a patched dependency is added to a flow mapping', async () => {
  const out = await editManifest('patchedDependencies: { foo: patches/foo.patch }\n', {
    updatedFields: { patchedDependencies: { foo: 'patches/foo.patch', bar: 'patches/bar.patch' } },
  })
  expect(out).toBe('patchedDependencies: { bar: patches/bar.patch, foo: patches/foo.patch }\n')
})

test('an omitted patched dependency is dropped from a flow mapping', async () => {
  const out = await editManifest('patchedDependencies: { foo: patches/foo.patch, bar: patches/bar.patch }\n', {
    updatedFields: { patchedDependencies: { bar: 'patches/bar.patch' } },
  })
  expect(out).toBe('patchedDependencies: { bar: patches/bar.patch }\n')
})

test('minimumReleaseAgeExclude stays a flow sequence', async () => {
  const out = await editManifest('minimumReleaseAgeExclude: [foo@1.0.0]\n', {
    addedMinimumReleaseAgeExcludes: ['bar@2.0.0'],
  })
  expect(out).toBe('minimumReleaseAgeExclude: [ foo@1.0.0, bar@2.0.0 ]\n')
})

test('ignoreGhsas stays a flow sequence under a block auditConfig', async () => {
  const out = await editManifest('auditConfig:\n  ignoreGhsas: [GHSA-aaaa-bbbb-cccc, GHSA-dddd-eeee-ffff]\n', {
    updatedFields: { auditConfig: { ignoreGhsas: ['GHSA-gggg-hhhh-iiii'] } },
  })
  expect(out).toBe('auditConfig:\n  ignoreGhsas: [ GHSA-gggg-hhhh-iiii ]\n')
})

test('ignoreGhsas is added to a flow auditConfig', async () => {
  const out = await editManifest('auditConfig: {}\n', {
    updatedFields: { auditConfig: { ignoreGhsas: ['GHSA-aaaa-bbbb-cccc'] } },
  })
  expect(out).toBe('auditConfig: { ignoreGhsas: [ GHSA-aaaa-bbbb-cccc ] }\n')
})

test('the entries of a flow named catalog are pruned in place', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  prepare({ dependencies: { abc: 'catalog:foo' } }, { tempDir: dir })
  fs.writeFileSync(filePath, 'catalogs: { foo: { abc: 0.1.2, ghi: 7.8.9 } }\n')
  await updateWorkspaceManifest(dir, {
    catalogPrune: true,
    allProjects: await findPackages(dir),
  })
  expect(fs.readFileSync(filePath, 'utf8')).toBe('catalogs: { foo: { abc: 0.1.2 } }\n')
})

test('an entry removed from a flow mapping leaves a parent-scoped override alone', async () => {
  const out = await editManifest('overrides: { foo: link:../foo, bar: { nested: value } }\n', {
    updatedFields: { overrides: { bar: { nested: 'value' } } as unknown as Record<string, string> },
  })
  expect(out).toBe('overrides: { bar: { nested: value } }\n')
})

test('a flow mapping emptied by an edit drops the whole block', async () => {
  const out = await editManifest('packages:\n  - pkg\noverrides: { foo: link:../foo }\n', {
    updatedFields: { overrides: undefined },
  })
  expect(out).toBe('packages:\n  - pkg\n')
})

// A multi-line flow collection is the one shape the Rust writer refuses to
// edit entry by entry, but dropping the whole block agrees in both.
test('a multi-line flow mapping is dropped whole when it empties', async () => {
  const out = await editManifest("packages:\n  - '*'\noverrides: {\n  foo: link:../foo, # pinned\n}\n", {
    updatedFields: { overrides: undefined },
  })
  expect(out).toBe("packages:\n  - '*'\n")
})
