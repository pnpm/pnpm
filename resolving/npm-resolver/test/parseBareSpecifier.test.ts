import { describe, expect, test } from '@jest/globals'

import {
  type NamedRegistryPackageSpec,
  parseBareSpecifier,
  parseNamedRegistrySpecifierToRegistryPackageSpec,
} from '../lib/parseBareSpecifier.js'

const GH_ALIASES: ReadonlySet<string> = new Set(['gh'])
const DEFAULT_TAG = 'latest'
const NPM_REGISTRY = 'https://registry.npmjs.org/'

describe('parseBareSpecifier', () => {
  test('npm:<version_selector> falls back to the outer alias as the package name', () => {
    // Mirrors the named-registry shape (`gh:^1.0.0` paired with `@acme/foo`),
    // so `npm:^1.0.0` paired with `is-positive` resolves the outer alias.
    expect(parseBareSpecifier('npm:^1.0.0', 'is-positive', DEFAULT_TAG, NPM_REGISTRY)).toMatchObject({
      name: 'is-positive',
      type: 'range',
    })
    expect(parseBareSpecifier('npm:1.0.0', '@acme/foo', DEFAULT_TAG, NPM_REGISTRY)).toMatchObject({
      name: '@acme/foo',
      type: 'version',
      fetchSpec: '1.0.0',
    })
  })
})

describe('parseNamedRegistrySpecifierToRegistryPackageSpec', () => {
  test('returns null on non-named-registry specifiers', () => {
    expect(parseNamedRegistrySpecifierToRegistryPackageSpec('^1.0.0', GH_ALIASES, undefined, DEFAULT_TAG)).toBeNull()
    expect(parseNamedRegistrySpecifierToRegistryPackageSpec('1.0.0', GH_ALIASES, undefined, DEFAULT_TAG)).toBeNull()
    expect(parseNamedRegistrySpecifierToRegistryPackageSpec('latest', GH_ALIASES, undefined, DEFAULT_TAG)).toBeNull()
    expect(parseNamedRegistrySpecifierToRegistryPackageSpec('npm:foo', GH_ALIASES, undefined, DEFAULT_TAG)).toBeNull()
    expect(parseNamedRegistrySpecifierToRegistryPackageSpec('npm:@foo/bar', GH_ALIASES, undefined, DEFAULT_TAG)).toBeNull()
    expect(parseNamedRegistrySpecifierToRegistryPackageSpec('jsr:@foo/bar', GH_ALIASES, undefined, DEFAULT_TAG)).toBeNull()
    expect(parseNamedRegistrySpecifierToRegistryPackageSpec('catalog:', GH_ALIASES, undefined, DEFAULT_TAG)).toBeNull()
    expect(parseNamedRegistrySpecifierToRegistryPackageSpec('workspace:*', GH_ALIASES, undefined, DEFAULT_TAG)).toBeNull()
  })

  test('does not intercept github: git shorthand (that scheme belongs to the git resolver)', () => {
    // `hosted-git-info` / `npm-package-arg` own the `github:` scheme as a GitHub git repository shortcut.
    // Even if a caller accidentally passed it in, it is not in the built-in `gh` alias set.
    expect(parseNamedRegistrySpecifierToRegistryPackageSpec('github:owner/repo', GH_ALIASES, undefined, DEFAULT_TAG)).toBeNull()
    expect(parseNamedRegistrySpecifierToRegistryPackageSpec('github:owner/repo#main', GH_ALIASES, undefined, DEFAULT_TAG)).toBeNull()
    expect(parseNamedRegistrySpecifierToRegistryPackageSpec('github:@acme/foo', GH_ALIASES, undefined, DEFAULT_TAG)).toBeNull()
  })

  test('parses <alias>:<version_selector> when a scoped package alias is given', () => {
    expect(parseNamedRegistrySpecifierToRegistryPackageSpec('gh:^1.0.0', GH_ALIASES, '@acme/foo', DEFAULT_TAG)).toStrictEqual({
      name: '@acme/foo',
      fetchSpec: '>=1.0.0 <2.0.0-0',
      type: 'range',
      registryName: 'gh',
    } as NamedRegistryPackageSpec)
    expect(parseNamedRegistrySpecifierToRegistryPackageSpec('gh:1.0.0', GH_ALIASES, '@acme/foo', DEFAULT_TAG)).toStrictEqual({
      name: '@acme/foo',
      fetchSpec: '1.0.0',
      type: 'version',
      registryName: 'gh',
    } as NamedRegistryPackageSpec)
    expect(parseNamedRegistrySpecifierToRegistryPackageSpec('gh:latest', GH_ALIASES, '@acme/foo', DEFAULT_TAG)).toStrictEqual({
      name: '@acme/foo',
      fetchSpec: 'latest',
      type: 'tag',
      registryName: 'gh',
    } as NamedRegistryPackageSpec)
  })

  test('parses <alias>:@<owner>/<name> and falls back to the default tag', () => {
    expect(parseNamedRegistrySpecifierToRegistryPackageSpec('gh:@acme/foo', GH_ALIASES, undefined, DEFAULT_TAG)).toStrictEqual({
      name: '@acme/foo',
      fetchSpec: 'latest',
      type: 'tag',
      registryName: 'gh',
    } as NamedRegistryPackageSpec)
  })

  test('parses <alias>:@<owner>/<name>@<version_selector>', () => {
    expect(parseNamedRegistrySpecifierToRegistryPackageSpec('gh:@acme/foo@^1.0.0', GH_ALIASES, undefined, DEFAULT_TAG)).toStrictEqual({
      name: '@acme/foo',
      fetchSpec: '>=1.0.0 <2.0.0-0',
      type: 'range',
      registryName: 'gh',
    } as NamedRegistryPackageSpec)
    expect(parseNamedRegistrySpecifierToRegistryPackageSpec('gh:@acme/foo@1.0.0', GH_ALIASES, undefined, DEFAULT_TAG)).toStrictEqual({
      name: '@acme/foo',
      fetchSpec: '1.0.0',
      type: 'version',
      registryName: 'gh',
    } as NamedRegistryPackageSpec)
    expect(parseNamedRegistrySpecifierToRegistryPackageSpec('gh:@acme/foo@beta', GH_ALIASES, undefined, DEFAULT_TAG)).toStrictEqual({
      name: '@acme/foo',
      fetchSpec: 'beta',
      type: 'tag',
      registryName: 'gh',
    } as NamedRegistryPackageSpec)
  })

  test('preserves the original package name (no scope rewrite, unlike jsr)', () => {
    // Named registries publish the package under its original name, unlike the JSR
    // npm compatibility registry which remaps `@scope/name` to `@jsr/scope__name`.
    expect(parseNamedRegistrySpecifierToRegistryPackageSpec('gh:@acme/foo@1.0.0', GH_ALIASES, undefined, DEFAULT_TAG)?.name).toBe('@acme/foo')
  })

  test('throws when the scope has no package name', () => {
    expect(() => parseNamedRegistrySpecifierToRegistryPackageSpec('gh:@acme@^1.0.0', GH_ALIASES, undefined, DEFAULT_TAG)).toThrow(expect.objectContaining({
      code: 'ERR_PNPM_INVALID_NAMED_REGISTRY_PACKAGE_NAME',
    }))
    expect(() => parseNamedRegistrySpecifierToRegistryPackageSpec('gh:@acme', GH_ALIASES, undefined, DEFAULT_TAG)).toThrow(expect.objectContaining({
      code: 'ERR_PNPM_INVALID_NAMED_REGISTRY_PACKAGE_NAME',
    }))
    expect(() => parseNamedRegistrySpecifierToRegistryPackageSpec('gh:@acme/', GH_ALIASES, undefined, DEFAULT_TAG)).toThrow(expect.objectContaining({
      code: 'ERR_PNPM_INVALID_NAMED_REGISTRY_PACKAGE_NAME',
    }))
  })

  test('does not claim <alias>:<version_selector> when no package alias is provided', () => {
    // No alias means we cannot know the package name — we must not hijack such specifiers.
    expect(parseNamedRegistrySpecifierToRegistryPackageSpec('gh:^1.0.0', GH_ALIASES, undefined, DEFAULT_TAG)).toBeNull()
  })

  test('falls back to the dependency alias for <alias>:<version_selector>, scoped or not', () => {
    // Mirrors the `npm:^1.0.0` shape — works with both scoped and unscoped aliases.
    expect(parseNamedRegistrySpecifierToRegistryPackageSpec('gh:^1.0.0', GH_ALIASES, '@acme/foo', DEFAULT_TAG)).toMatchObject({
      name: '@acme/foo',
      registryName: 'gh',
    })
    expect(parseNamedRegistrySpecifierToRegistryPackageSpec('work:^4.0.0', new Set(['work']), 'lodash', DEFAULT_TAG)).toMatchObject({
      name: 'lodash',
      registryName: 'work',
    })
  })

  test('parses unscoped <alias>:<name>[@<version_selector>]', () => {
    // Arbitrary named registries (vlt-style) accept unscoped names too,
    // not just GitHub Packages-style scopes.
    expect(parseNamedRegistrySpecifierToRegistryPackageSpec('work:lodash@^4.0.0', new Set(['work']), undefined, DEFAULT_TAG)).toMatchObject({
      name: 'lodash',
      fetchSpec: '>=4.0.0 <5.0.0-0',
      type: 'range',
      registryName: 'work',
    })
    expect(parseNamedRegistrySpecifierToRegistryPackageSpec('work:lodash', new Set(['work']), undefined, DEFAULT_TAG)).toMatchObject({
      name: 'lodash',
      fetchSpec: 'latest',
      type: 'tag',
      registryName: 'work',
    })
  })

  test('matches any alias in the configured set and reports it back to the caller', () => {
    expect(parseNamedRegistrySpecifierToRegistryPackageSpec('work:@acme/foo@^1.0.0', new Set(['gh', 'work']), undefined, DEFAULT_TAG)).toStrictEqual({
      name: '@acme/foo',
      fetchSpec: '>=1.0.0 <2.0.0-0',
      type: 'range',
      registryName: 'work',
    } as NamedRegistryPackageSpec)
  })

  test('returns null when the alias is not in the configured set', () => {
    // Unrecognized prefixes must fall through so other resolvers (git, npm:, etc.) can try.
    expect(parseNamedRegistrySpecifierToRegistryPackageSpec('work:@acme/foo', GH_ALIASES, undefined, DEFAULT_TAG)).toBeNull()
  })

  test('includes the user-facing alias in error messages for user-defined aliases', () => {
    // When `work:@acme` fails validation, the user's alias must appear in the error
    // so they can find the offending specifier — not a generic `gh` reference.
    expect(() => parseNamedRegistrySpecifierToRegistryPackageSpec('work:@acme', new Set(['work']), undefined, DEFAULT_TAG)).toThrow(expect.objectContaining({
      code: 'ERR_PNPM_INVALID_NAMED_REGISTRY_PACKAGE_NAME',
      message: expect.stringContaining("'work:'"),
    }))
  })
})
