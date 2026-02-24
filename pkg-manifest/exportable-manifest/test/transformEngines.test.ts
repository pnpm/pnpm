import { transformEngines, DevEnginesRuntimeConflictError } from '../lib/transform/engines.js'

describe('transformEngines', () => {
  test('moves engines.runtime to devEngines.runtime', () => {
    const manifest = {
      name: 'test-package',
      version: '1.0.0',
      engines: {
        node: '>=18',
        runtime: { name: 'bun', version: '1.0.0' },
      },
    }

    const result = transformEngines(manifest)

    expect(result).toStrictEqual({
      name: 'test-package',
      version: '1.0.0',
      engines: {
        node: '>=18',
      },
      devEngines: {
        runtime: { name: 'bun', version: '1.0.0' },
      },
    })
  })

  test('preserves existing devEngines when moving engines.runtime', () => {
    const manifest = {
      name: 'test-package',
      version: '1.0.0',
      engines: {
        node: '>=18',
        runtime: { name: 'bun', version: '1.0.0' },
      },
      devEngines: {
        cpu: [{ name: 'x64' }, { name: 'arm64' }],
      },
    }

    const result = transformEngines(manifest)

    expect(result).toStrictEqual({
      name: 'test-package',
      version: '1.0.0',
      engines: {
        node: '>=18',
      },
      devEngines: {
        cpu: [{ name: 'x64' }, { name: 'arm64' }],
        runtime: { name: 'bun', version: '1.0.0' },
      },
    })
  })

  test('does not modify manifest when engines.runtime is not present', () => {
    const manifest = {
      name: 'test-package',
      version: '1.0.0',
      engines: {
        node: '>=18',
      },
    }

    const result = transformEngines(manifest)

    expect(result).toStrictEqual({
      name: 'test-package',
      version: '1.0.0',
      engines: {
        node: '>=18',
      },
    })
  })

  test('does not modify manifest when engines field is empty', () => {
    const manifest = {
      name: 'test-package',
      version: '1.0.0',
      engines: {},
    }

    const result = transformEngines(manifest)

    expect(result).toStrictEqual({
      name: 'test-package',
      version: '1.0.0',
      engines: {},
    })
  })

  test('throws error when both engines.runtime and devEngines.runtime are defined', () => {
    const manifest = {
      name: 'test-package',
      version: '1.0.0',
      engines: {
        node: '>=18',
        runtime: { name: 'bun', version: '1.0.0' },
      },
      devEngines: {
        runtime: { name: 'deno', version: '2.0.0' },
      },
    }

    expect(() => transformEngines(manifest)).toThrow(DevEnginesRuntimeConflictError)
  })

  test('removes engines field when only runtime was present', () => {
    const manifest = {
      name: 'test-package',
      version: '1.0.0',
      engines: {
        runtime: { name: 'bun', version: '1.0.0' },
      },
    }

    const result = transformEngines(manifest)

    expect(result).toStrictEqual({
      name: 'test-package',
      version: '1.0.0',
      engines: {},
      devEngines: {
        runtime: { name: 'bun', version: '1.0.0' },
      },
    })
  })

  test('handles manifest with other fields', () => {
    const manifest = {
      name: 'test-package',
      version: '1.0.0',
      description: 'A test package',
      dependencies: {
        foo: '1.0.0',
      },
      engines: {
        node: '>=18',
        npm: '>=8',
        runtime: { name: 'bun', version: '1.0.0' },
      },
      scripts: {
        test: 'echo test',
      },
    }

    const result = transformEngines(manifest)

    expect(result).toStrictEqual({
      name: 'test-package',
      version: '1.0.0',
      description: 'A test package',
      dependencies: {
        foo: '1.0.0',
      },
      engines: {
        node: '>=18',
        npm: '>=8',
      },
      devEngines: {
        runtime: { name: 'bun', version: '1.0.0' },
      },
      scripts: {
        test: 'echo test',
      },
    })
  })
})
