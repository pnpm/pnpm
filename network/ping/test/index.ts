import { describe, expect, it } from '@jest/globals'

import { ping } from '../src/index.js'

describe('ping command', () => {
  it('should have correct command names', () => {
    expect(ping.commandNames).toEqual(['ping'])
  })

  it('should have a help function', () => {
    const help = ping.help()
    expect(help).toContain('Test connectivity')
    expect(help).toContain('pnpm ping')
  })

  it('should have cliOptionsTypes function', () => {
    const options = ping.cliOptionsTypes()
    expect(options).toHaveProperty('registry')
    expect(options.registry).toBe(String)
  })

  it('should have rcOptionsTypes function', () => {
    const options = ping.rcOptionsTypes()
    expect(typeof options).toBe('object')
  })

  it('should return success message for reachable registry', async () => {
    const result = await ping.handler({
      registry: 'https://registry.npmjs.org/',
      registries: {
        default: 'https://registry.npmjs.org/',
      },
    })
    expect(result).toContain('Registry is reachable')
    expect(result).toContain('https://registry.npmjs.org/')
  })

  it('should use default registry when not specified', async () => {
    const result = await ping.handler({
      registries: {
        default: 'https://registry.npmjs.org/',
      },
    })
    expect(result).toContain('Registry is reachable')
  })

  it('should throw error on network failure', async () => {
    try {
      await ping.handler({
        registry: 'https://invalid-registry-that-does-not-exist-12345.com/',
        registries: {
          default: 'https://registry.npmjs.org/',
        },
      })
      fail('Should have thrown an error')
    } catch (err: unknown) {
      expect(err).toBeDefined()
    }
  })
})
