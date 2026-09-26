import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { prepareEmpty } from '@pnpm/prepare'
import PATH from 'path-name'

import { getConfig } from '../src/index.js'
import {
  type OsProxy,
  parseMacosScutilProxy,
  parseWindowsInternetSettings,
  resolveProxyFromSources,
} from '../src/osProxy.js'

const osProxy: OsProxy = {
  httpsProxy: 'http://os-https.example:9',
  httpProxy: 'http://os-http.example:9',
  noProxy: 'os.example',
}

function configEnv (overrides: Record<string, string | undefined> = {}): Record<string, string | undefined> {
  const env: Record<string, string | undefined> = {
    ...process.env,
    PNPM_HOME: import.meta.dirname,
    [PATH]: path.join(import.meta.dirname, 'bin'),
  }
  for (const key of [
    'HTTPS_PROXY',
    'https_proxy',
    'HTTP_PROXY',
    'http_proxy',
    'PROXY',
    'proxy',
    'NO_PROXY',
    'no_proxy',
    'PNPM_CONFIG_HTTPS_PROXY',
    'PNPM_CONFIG_HTTP_PROXY',
    'PNPM_CONFIG_PROXY',
    'PNPM_CONFIG_NO_PROXY',
    'pnpm_config_https_proxy',
    'pnpm_config_http_proxy',
    'pnpm_config_proxy',
    'pnpm_config_no_proxy',
    'NPM_CONFIG_HTTPS_PROXY',
    'NPM_CONFIG_HTTP_PROXY',
    'NPM_CONFIG_PROXY',
    'npm_config_https_proxy',
    'npm_config_http_proxy',
    'npm_config_proxy',
  ]) {
    delete env[key]
  }
  return { ...env, ...overrides }
}

test('environment proxy wins over the operating system proxy', () => {
  const resolved = resolveProxyFromSources({
    httpsProxy: undefined,
    httpProxy: undefined,
    proxy: undefined,
    noProxy: undefined,
    noproxy: undefined,
    envHttpsProxy: 'http://env.example:8080',
    envHttpProxy: undefined,
    envProxy: undefined,
    envNoProxy: undefined,
    os: osProxy,
  })

  expect(resolved.httpsProxy).toBe('http://env.example:8080')
  expect(resolved.httpProxy).toBe('http://env.example:8080')
  expect(resolved.noProxy).toBe('os.example')
})

test('config proxy wins over the environment and the operating system', () => {
  const resolved = resolveProxyFromSources({
    httpsProxy: 'http://npmrc.example:8080',
    httpProxy: undefined,
    proxy: undefined,
    noProxy: 'npmrc.example',
    noproxy: undefined,
    envHttpsProxy: 'http://env.example:8080',
    envHttpProxy: 'http://env-http.example:8080',
    envProxy: undefined,
    envNoProxy: 'env.example',
    os: osProxy,
  })

  expect(resolved.httpsProxy).toBe('http://npmrc.example:8080')
  expect(resolved.httpProxy).toBe('http://npmrc.example:8080')
  expect(resolved.noProxy).toBe('npmrc.example')
})

test('proxy false is not re-enabled by the operating system', () => {
  const resolved = resolveProxyFromSources({
    httpsProxy: undefined,
    httpProxy: undefined,
    proxy: false,
    noProxy: undefined,
    noproxy: undefined,
    envHttpsProxy: 'http://env.example:8080',
    envHttpProxy: 'http://env-http.example:8080',
    envProxy: undefined,
    envNoProxy: undefined,
    os: osProxy,
  })

  expect(resolved.httpsProxy).toBe(false)
  expect(resolved.httpProxy).toBe(false)
})

test('an empty environment proxy shadows the operating system', () => {
  const resolved = resolveProxyFromSources({
    httpsProxy: undefined,
    httpProxy: undefined,
    proxy: undefined,
    noProxy: undefined,
    noproxy: undefined,
    envHttpsProxy: '',
    envHttpProxy: 'http://env-http.example:8080',
    envProxy: undefined,
    envNoProxy: '',
    os: osProxy,
  })

  expect(resolved.httpsProxy).toBe('')
  expect(resolved.httpProxy).toBe('')
  expect(resolved.noProxy).toBe('')
})

test('the operating system proxy is used only for slots config and the environment left unset', () => {
  const resolved = resolveProxyFromSources({
    httpsProxy: undefined,
    httpProxy: undefined,
    proxy: undefined,
    noProxy: undefined,
    noproxy: undefined,
    envHttpsProxy: undefined,
    envHttpProxy: 'http://env-http.example:8080',
    envProxy: undefined,
    envNoProxy: undefined,
    os: osProxy,
  })

  expect(resolved.httpsProxy).toBe('http://os-https.example:9')
  expect(resolved.httpProxy).toBe('http://env-http.example:8080')
  expect(resolved.noProxy).toBe('os.example')
})

test('getConfig prefers https_proxy over the operating system proxy', async () => {
  prepareEmpty()

  const { config } = await getConfig({
    cliOptions: {},
    env: configEnv({ HTTPS_PROXY: 'http://env.example:8080' }),
    osProxy,
    packageManager: { name: 'pnpm', version: '1.0.0' },
    workspaceDir: process.cwd(),
  })

  expect(config.httpsProxy).toBe('http://env.example:8080')
  expect(config.httpProxy).toBe('http://env.example:8080')
  expect(config.packageManagerNetworkConfig).toMatchObject({
    httpsProxy: 'http://env.example:8080',
    httpProxy: 'http://env.example:8080',
    noProxy: 'os.example',
  })
})

test('getConfig prefers .npmrc proxy settings over the environment and the operating system', async () => {
  prepareEmpty()

  fs.writeFileSync('.npmrc', 'https-proxy=http://npmrc.example:8080\nno-proxy=npmrc.example\n', 'utf8')

  const { config } = await getConfig({
    cliOptions: {},
    env: configEnv({ HTTPS_PROXY: 'http://env.example:8080', NO_PROXY: 'env.example' }),
    osProxy,
    packageManager: { name: 'pnpm', version: '1.0.0' },
    workspaceDir: process.cwd(),
  })

  expect(config.httpsProxy).toBe('http://npmrc.example:8080')
  expect(config.httpProxy).toBe('http://npmrc.example:8080')
  expect(config.noProxy).toBe('npmrc.example')
})

test('getConfig uses the operating system proxy when config and the environment are unset', async () => {
  prepareEmpty()

  const { config } = await getConfig({
    cliOptions: {},
    env: configEnv(),
    osProxy,
    packageManager: { name: 'pnpm', version: '1.0.0' },
    workspaceDir: process.cwd(),
  })

  expect(config.httpsProxy).toBe('http://os-https.example:9')
  expect(config.httpProxy).toBe('http://os-http.example:9')
  expect(config.noProxy).toBe('os.example')
  expect(config.packageManagerNetworkConfig).toMatchObject({
    httpsProxy: 'http://os-https.example:9',
    httpProxy: 'http://os-http.example:9',
    noProxy: 'os.example',
  })
})

test('getConfig keeps proxy=false off when the operating system has a proxy', async () => {
  prepareEmpty()

  fs.writeFileSync('.npmrc', 'proxy=false\n', 'utf8')

  const { config } = await getConfig({
    cliOptions: {},
    env: configEnv({ HTTP_PROXY: 'http://env.example:8080' }),
    osProxy,
    packageManager: { name: 'pnpm', version: '1.0.0' },
    workspaceDir: process.cwd(),
  })

  expect(config.httpsProxy).toBe(false)
  expect(config.httpProxy).toBe(false)
})

test('windows and macOS proxy text parse to the same slots', () => {
  const windows = parseWindowsInternetSettings([
    'HKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings',
    '    ProxyEnable    REG_DWORD    0x1',
    '    ProxyServer    REG_SZ    http=10.1.1.1:80;https=10.1.1.2:443;socks=10.1.1.3:1080',
    '    ProxyOverride    REG_SZ    localhost;<local>;*.corp.example',
  ].join('\n'))
  expect(windows).toEqual({
    httpProxy: '10.1.1.1:80',
    httpsProxy: '10.1.1.2:443',
    noProxy: 'localhost,corp.example',
  })
  expect(parseWindowsInternetSettings('    ProxyEnable    REG_DWORD    0x0\n    ProxyServer    REG_SZ    proxy.example:8080\n')).toEqual({})

  const macos = parseMacosScutilProxy([
    '<dictionary> {',
    '  ExceptionsList : <array> {',
    '    0 : *.local',
    '    1 : <local>',
    '    2 : example.com',
    '  }',
    '  HTTPEnable : 1',
    '  HTTPPort : 8080',
    '  HTTPProxy : 10.0.0.1',
    '  HTTPSEnable : 0',
    '  HTTPSPort : 8443',
    '  HTTPSProxy : 10.0.0.2',
    '}',
  ].join('\n'))
  expect(macos).toEqual({
    httpProxy: '10.0.0.1:8080',
    httpsProxy: undefined,
    noProxy: 'local,example.com',
  })
})
