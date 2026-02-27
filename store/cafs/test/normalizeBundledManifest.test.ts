import { normalizeBundledManifest } from '../src/normalizeBundledManifest.js'

describe('normalizeBundledManifest', () => {
  it('returns undefined for an empty manifest', () => {
    expect(normalizeBundledManifest({})).toBeUndefined()
  })

  it('returns undefined when manifest has only excluded fields', () => {
    expect(normalizeBundledManifest({
      description: 'a package',
      keywords: ['test'],
      license: 'MIT',
      author: 'test',
      repository: 'test/test',
    })).toBeUndefined()
  })

  it('picks included fields and excludes others', () => {
    const result = normalizeBundledManifest({
      name: 'foo',
      version: '1.0.0',
      description: 'should be excluded',
      license: 'MIT',
      bin: { foo: './bin/foo.js' },
      engines: { node: '>=18' },
      cpu: ['x64'],
      os: ['linux'],
      libc: ['glibc'],
      dependencies: { bar: '^1.0.0' },
      devDependencies: { qux: '^3.0.0' },
      optionalDependencies: { baz: '^2.0.0' },
      peerDependencies: { react: '^18' },
      peerDependenciesMeta: { react: { optional: true } },
      bundledDependencies: ['bar'],
      directories: { bin: './bin' },
    })
    expect(result).toStrictEqual({
      name: 'foo',
      version: '1.0.0',
      bin: { foo: './bin/foo.js' },
      engines: { node: '>=18' },
      cpu: ['x64'],
      os: ['linux'],
      libc: ['glibc'],
      dependencies: { bar: '^1.0.0' },
      devDependencies: { qux: '^3.0.0' },
      optionalDependencies: { baz: '^2.0.0' },
      peerDependencies: { react: '^18' },
      peerDependenciesMeta: { react: { optional: true } },
      bundledDependencies: ['bar'],
      directories: { bin: './bin' },
    })
    // Excluded fields should not be present
    expect(result).not.toHaveProperty('description')
    expect(result).not.toHaveProperty('license')
  })

  it('only picks lifecycle scripts, not all scripts', () => {
    const result = normalizeBundledManifest({
      name: 'foo',
      version: '1.0.0',
      scripts: {
        preinstall: 'echo pre',
        install: 'echo install',
        postinstall: 'echo post',
        test: 'jest',
        build: 'tsc',
        start: 'node index.js',
        prepare: 'tsc',
      },
    })
    expect(result!.scripts).toStrictEqual({
      preinstall: 'echo pre',
      install: 'echo install',
      postinstall: 'echo post',
    })
  })

  it('omits scripts key when no lifecycle scripts exist', () => {
    const result = normalizeBundledManifest({
      name: 'foo',
      version: '1.0.0',
      scripts: {
        test: 'jest',
        build: 'tsc',
      },
    })
    expect(result).not.toHaveProperty('scripts')
  })

  it('normalizes version with semver.clean', () => {
    expect(normalizeBundledManifest({
      name: 'foo',
      version: '  =v1.2.3  ',
    })!.version).toBe('1.2.3')
  })

  it('keeps version as-is when semver.clean returns null', () => {
    expect(normalizeBundledManifest({
      name: 'foo',
      version: 'not-semver',
    })!.version).toBe('not-semver')
  })

  it('defaults missing version to 0.0.0', () => {
    expect(normalizeBundledManifest({
      name: 'foo',
    })!.version).toBe('0.0.0')
  })

  it('skips null/undefined fields', () => {
    const result = normalizeBundledManifest({
      name: 'foo',
      version: '1.0.0',
      bin: undefined,
      engines: undefined,
    })
    expect(result).not.toHaveProperty('bin')
    expect(result).not.toHaveProperty('engines')
  })
})
