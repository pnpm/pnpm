import { DEFAULT_OPTS as BASE_OPTS, REGISTRY_URL } from '@pnpm/testing.command-defaults'

export const DEFAULT_OPTS = {
  ...BASE_OPTS,
  registriesByScope: { default: 'https://registry.npmjs.org/' },
  registry: 'https://registry.npmjs.org/',
  strictSsl: true,
  peersSuffixMaxLength: 1000,
}

export const AUDIT_REGISTRY = 'http://audit.registry/'
export const AUDIT_REGISTRY_OPTS = {
  ...DEFAULT_OPTS,
  registry: AUDIT_REGISTRY,
  registriesByScope: { default: AUDIT_REGISTRY },
  configByUri: {},
}

export const MOCK_REGISTRY = REGISTRY_URL
export const MOCK_REGISTRY_OPTS = {
  ...DEFAULT_OPTS,
  registry: MOCK_REGISTRY,
  registriesByScope: { default: MOCK_REGISTRY },
  configByUri: {},
}
