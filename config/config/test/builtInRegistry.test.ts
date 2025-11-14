import { jest } from '@jest/globals'
import { type Conf, addBuiltInRegistry } from '../src/builtInRegistry.js'

test('addBuiltInRegistry prints warnings when npm registry was overridden but jsr registry was not', () => {
  const add = jest.fn()
  const warn = jest.fn()
  const conf: Conf = {
    add,
    sources: {
      project: {
        path: '/home/username/projects/my-project',
        data: {
          registry: 'https://mycompany.example.com/registry/',
        },
      },
    },
  }
  addBuiltInRegistry(conf, { warn })
  expect(add).toHaveBeenCalledWith({
    registry: 'https://registry.npmjs.org/',
    '@jsr:registry': 'https://npm.jsr.io/',
  }, 'pnpm-builtin')
  expect(warn).toHaveBeenCalledWith(
    "Config at /home/username/projects/my-project has overridden the 'registry' key without overriding the '@jsr:registry' key, it could leave a security exploit"
  )
})

test('addBuiltInRegistry does not print warnings when both npm registry and jsr registry were overridden', () => {
  const add = jest.fn()
  const warn = jest.fn()
  const conf: Conf = {
    add,
    sources: {
      project: {
        path: '/home/username/projects/my-project',
        data: {
          registry: 'https://mycompany.example.com/registry/',
          '@jsr:registry': 'https://mycompany.example.com/registry/',
        },
      },
    },
  }
  addBuiltInRegistry(conf, { warn })
  expect(add).toHaveBeenCalledWith({
    registry: 'https://registry.npmjs.org/',
    '@jsr:registry': 'https://npm.jsr.io/',
  }, 'pnpm-builtin')
  expect(warn).not.toHaveBeenCalled()
})

test('addBuiltInRegistry does not print warnings when npm registry was overridden and jsr registry was overridden to an empty string', () => {
  const add = jest.fn()
  const warn = jest.fn()
  const conf: Conf = {
    add,
    sources: {
      project: {
        path: '/home/username/projects/my-project',
        data: {
          registry: 'https://mycompany.example.com/registry/',
          '@jsr:registry': '',
        },
      },
    },
  }
  addBuiltInRegistry(conf, { warn })
  expect(add).toHaveBeenCalledWith({
    registry: 'https://registry.npmjs.org/',
    '@jsr:registry': 'https://npm.jsr.io/',
  }, 'pnpm-builtin')
  expect(warn).not.toHaveBeenCalled()
})

test('addBuiltInRegistry does not print warnings when no default registries were overridden', () => {
  const add = jest.fn()
  const warn = jest.fn()
  const conf: Conf = {
    add,
    sources: {
      project: {
        path: '/home/username/projects/my-project',
        data: {},
      },
    },
  }
  addBuiltInRegistry(conf, { warn })
  expect(add).toHaveBeenCalledWith({
    registry: 'https://registry.npmjs.org/',
    '@jsr:registry': 'https://npm.jsr.io/',
  }, 'pnpm-builtin')
  expect(warn).not.toHaveBeenCalled()
})
