/* eslint-disable no-template-curly-in-string */
import { stripNpmrcFallbacks } from '../lib/publish.js'

describe('stripNpmrcFallbacks', () => {
  it('should strip fallback values from environment variables in string values', () => {
    const npmrc = {
      registry: 'https://${REGISTRY:-registry.npmjs.org}',
      '//registry.npmjs.org/:_authToken': '${NPM_TOKEN:-default-token}',
    }

    const result = stripNpmrcFallbacks(npmrc)

    expect(result).toEqual({
      registry: 'https://${REGISTRY}',
      '//registry.npmjs.org/:_authToken': '${NPM_TOKEN}',
    })
  })

  it('should preserve non-string values unchanged', () => {
    const npmrc = {
      'strict-ssl': false,
      timeout: 60000,
      registry: '${REGISTRY:-https://registry.npmjs.org}',
    }

    const result = stripNpmrcFallbacks(npmrc)

    expect(result).toEqual({
      'strict-ssl': false,
      timeout: 60000,
      registry: '${REGISTRY}',
    })
  })

  it('should handle empty object', () => {
    const npmrc = {}

    const result = stripNpmrcFallbacks(npmrc)

    expect(result).toEqual({})
  })

  it('should handle real-world npmrc configuration', () => {
    const npmrc = {
      registry: '${NPM_REGISTRY:-https://registry.npmjs.org}',
      '//registry.npmjs.org/:_authToken': '${NPM_TOKEN:-fallback}',
      'strict-ssl': true,
      'save-exact': true,
      '//custom-registry.com/:_authToken': '${CUSTOM_TOKEN:-fallback-value}',
    }

    const result = stripNpmrcFallbacks(npmrc)

    expect(result).toEqual({
      registry: '${NPM_REGISTRY}',
      '//registry.npmjs.org/:_authToken': '${NPM_TOKEN}',
      'strict-ssl': true,
      'save-exact': true,
      '//custom-registry.com/:_authToken': '${CUSTOM_TOKEN}',
    })
  })
})
