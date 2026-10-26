import assert from 'node:assert/strict'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { afterEach, describe, mock, test } from 'node:test'

import { pnpmProxyConfig, pnpmRegistryConfig } from '../lib/pnpmConfig.js'

const CA = fs.readFileSync(path.join(import.meta.dirname, '../../../crates/testing-utils/src/fixtures/tls/ca.pem'), 'utf8').trim()

const tmpDirs: string[] = []

afterEach(() => {
  for (const dir of tmpDirs.splice(0)) {
    fs.rmSync(dir, { recursive: true, force: true })
  }
  mock.restoreAll()
})

describe('pnpmRegistryConfig', () => {
  test('defaults to the npm registry', () => {
    const { cwd, home } = layout()

    assert.deepEqual(pnpmRegistryConfig({ readProjectNpmrc: true, cwd, homeDir: home, env: {} }), {
      registry: 'https://registry.npmjs.org/',
    })
  })

  test('reads the registry and credentials of the user .npmrc, expanding its placeholders', () => {
    const { cwd, home } = layout({
      user: ['registry=https://registry.example.test/npm', '//registry.example.test/npm/:_authToken=${TOKEN}'],
    })

    assert.deepEqual(pnpmRegistryConfig({ readProjectNpmrc: true, cwd, homeDir: home, env: { TOKEN: 'user-token' } }), {
      registry: 'https://registry.example.test/npm/',
      headers: { authorization: 'Bearer user-token' },
    })
  })

  test('reads the user .npmrc that the environment names', () => {
    const { cwd, root } = layout()
    const userconfig = path.join(root, 'elsewhere.npmrc')
    fs.writeFileSync(userconfig, 'registry=https://registry.example.test/\n')

    const config = pnpmRegistryConfig({ readProjectNpmrc: true, cwd, env: { pnpm_config_userconfig: userconfig } })

    assert.equal(config.registry, 'https://registry.example.test/')
  })

  test('takes @pnpm:registry over the registry of the environment, and that over registry', () => {
    const { cwd, home } = layout({ user: ['registry=https://user.example.test/'] })
    const env = { npm_config_registry: 'https://env.example.test/' }

    assert.equal(pnpmRegistryConfig({ readProjectNpmrc: true, cwd, homeDir: home, env }).registry, 'https://env.example.test/')

    fs.appendFileSync(path.join(home, '.npmrc'), '@pnpm:registry=https://scope.example.test/\n')
    assert.equal(pnpmRegistryConfig({ readProjectNpmrc: true, cwd, homeDir: home, env }).registry, 'https://scope.example.test/')
  })

  test('sends the @pnpm scope credentials over the registry-wide ones', () => {
    const { cwd, home } = layout({
      user: [
        'registry=https://registry.example.test/',
        '//registry.example.test/:_authToken=registry-token',
        '//registry.example.test/:@pnpm:_authToken=scope-token',
      ],
    })

    const config = pnpmRegistryConfig({ readProjectNpmrc: true, cwd, homeDir: home, env: {} })

    assert.deepEqual(config.headers, { authorization: 'Bearer scope-token' })
  })

  test('takes the credentials of the longest prefix, then those without the port', () => {
    const { cwd, home } = layout({
      user: [
        'registry=https://registry.example.test:8443/npm/private/',
        '//registry.example.test:8443/:_authToken=root-token',
        '//registry.example.test/npm/private/:_authToken=portless-token',
        '//registry.example.test:8443/npm/:_authToken=npm-token',
      ],
    })

    const config = pnpmRegistryConfig({ readProjectNpmrc: true, cwd, homeDir: home, env: {} })

    assert.deepEqual(config.headers, { authorization: 'Bearer npm-token' })
  })

  test('turns _auth and username with _password into Basic credentials', () => {
    const auth = Buffer.from('user:pass').toString('base64')
    const { cwd, home } = layout({
      user: ['registry=https://registry.example.test/', `//registry.example.test/:_auth=${auth}`],
    })
    assert.deepEqual(pnpmRegistryConfig({ readProjectNpmrc: true, cwd, homeDir: home, env: {} }).headers, {
      authorization: `Basic ${auth}`,
    })

    fs.writeFileSync(path.join(home, '.npmrc'), [
      'registry=https://registry.example.test/',
      '//registry.example.test/:username=user',
      `//registry.example.test/:_password=${Buffer.from('p@ss').toString('base64')}`,
    ].join('\n'))
    assert.deepEqual(pnpmRegistryConfig({ readProjectNpmrc: true, cwd, homeDir: home, env: {} }).headers, {
      authorization: `Basic ${Buffer.from('user:p@ss').toString('base64')}`,
    })
  })

  test('sends no credentials over plain HTTP to another machine', () => {
    for (const [registry, sent] of [
      ['http://registry.example.test/', false],
      ['http://127.0.0.1:4873/', true],
      ['http://localhost:4873/', true],
    ] as const) {
      const host = new URL(registry).host
      const { cwd, home } = layout({ user: [`registry=${registry}`, `//${host}/:_authToken=token`] })

      const config = pnpmRegistryConfig({ readProjectNpmrc: true, cwd, homeDir: home, env: {} })

      assert.equal(config.headers != null, sent, registry)
    }
  })

  test('reads the .npmrc of the workspace root over the user one', () => {
    const { cwd, home, project } = layout({
      user: ['registry=https://user.example.test/'],
      project: ['registry=https://project.example.test/', '//project.example.test/:_authToken=project-token'],
    })
    fs.writeFileSync(path.join(project, 'pnpm-workspace.yaml'), '')
    fs.writeFileSync(path.join(cwd, '.npmrc'), 'registry=https://package.example.test/\n')

    assert.deepEqual(pnpmRegistryConfig({ readProjectNpmrc: true, cwd, homeDir: home, env: {} }), {
      registry: 'https://project.example.test/',
      headers: { authorization: 'Bearer project-token' },
    })
  })

  test('reads the .npmrc of the current directory outside a workspace', () => {
    const { cwd, home } = layout({ project: ['registry=https://project.example.test/'] })
    fs.writeFileSync(path.join(cwd, '.npmrc'), 'registry=https://package.example.test/\n')

    const config = pnpmRegistryConfig({ readProjectNpmrc: true, cwd, homeDir: home, env: {} })

    assert.equal(config.registry, 'https://package.example.test/')
  })

  test('reads no project .npmrc unless asked to', () => {
    const { cwd, home, project } = layout({
      user: ['registry=https://user.example.test/'],
      project: ['registry=https://project.example.test/'],
    })
    fs.writeFileSync(path.join(project, 'pnpm-workspace.yaml'), '')

    const config = pnpmRegistryConfig({ readProjectNpmrc: false, cwd, homeDir: home, env: {} })

    assert.equal(config.registry, 'https://user.example.test/')
  })

  test('drops a project .npmrc entry that names an environment variable', () => {
    const warn = mock.method(console, 'warn', () => {})
    const { cwd, home, project } = layout({
      project: ['registry=https://project.example.test/', '//project.example.test/:_authToken=${TOKEN}'],
    })
    fs.writeFileSync(path.join(project, 'pnpm-workspace.yaml'), '')

    const config = pnpmRegistryConfig({ readProjectNpmrc: true, cwd, homeDir: home, env: { TOKEN: 'secret' } })

    assert.deepEqual(config, { registry: 'https://project.example.test/' })
    assert.match(String(warn.mock.calls[0]?.arguments[0]), /Ignored \/\/project\.example\.test\/:_authToken from .*\.npmrc/)
  })

  test('reads only the top-level settings', () => {
    const { cwd, home } = layout({ user: ['[section]', 'registry=https://section.example.test/'] })

    const config = pnpmRegistryConfig({ readProjectNpmrc: true, cwd, homeDir: home, env: {} })

    assert.equal(config.registry, 'https://registry.npmjs.org/')
  })

  describe('ca', () => {
    test('reads a cafile of the project .npmrc relative to it', () => {
      const { cwd, home, project } = layout({ project: ['cafile=certs/ca.pem'] })
      fs.writeFileSync(path.join(project, 'pnpm-workspace.yaml'), '')
      fs.mkdirSync(path.join(project, 'certs'))
      fs.writeFileSync(path.join(project, 'certs', 'ca.pem'), CA)

      assert.deepEqual(pnpmRegistryConfig({ readProjectNpmrc: true, cwd, homeDir: home, env: {} }).ca, [CA])
    })

    test('reads every ca[] entry of the user .npmrc, with its line breaks escaped', () => {
      const { cwd, home } = layout({ user: [`ca[]="${escaped(CA)}"`, `ca[]="${escaped(CA)}"`] })

      assert.deepEqual(pnpmRegistryConfig({ readProjectNpmrc: true, cwd, homeDir: home, env: {} }).ca, [CA, CA])
    })

    test('takes the top-level CA settings of the project .npmrc over the user ones', () => {
      const { cwd, home, project } = layout({ user: ['cafile=/nonexistent/ca.pem'], project: [`ca="${escaped(CA)}"`] })
      fs.writeFileSync(path.join(project, 'pnpm-workspace.yaml'), '')

      assert.deepEqual(pnpmRegistryConfig({ readProjectNpmrc: true, cwd, homeDir: home, env: {} }).ca, [CA])
    })

    test('adds the CA scoped to the registry, and not one scoped to another', () => {
      const { cwd, home } = layout({
        user: [
          'registry=https://registry.example.test/npm/',
          `//registry.example.test/npm/:ca="${escaped(CA)}"`,
          '//other.example.test/:cafile=/nonexistent/ca.pem',
        ],
      })

      assert.deepEqual(pnpmRegistryConfig({ readProjectNpmrc: true, cwd, homeDir: home, env: {} }).ca, [CA])
    })
  })
})

/**
 * A home directory and a project directory, with the current directory a
 * package inside the project.
 */
describe('pnpmProxyConfig', () => {
  test('falls back to the proxy variables of the environment', () => {
    const { home } = layout()

    assert.deepEqual(pnpmProxyConfig({ homeDir: home, env: { https_proxy: 'http://env-https.test/', NO_PROXY: 'env.test' } }), {
      HTTPS_PROXY: 'http://env-https.test/',
      HTTP_PROXY: 'http://env-https.test/',
      NO_PROXY: 'env.test',
    })
    assert.deepEqual(pnpmProxyConfig({ homeDir: home, env: {} }), {})
  })

  test('takes PNPM_CONFIG_* over the global config.yaml, that over the user .npmrc, and leaves out the project .npmrc', () => {
    const { cwd, home, project } = layout({
      user: ['https-proxy=http://npmrc-https.test/', 'http-proxy=http://npmrc-http.test/', 'no-proxy=npmrc.test'],
      project: ['https-proxy=http://project.test/', 'no-proxy=project.test'],
    })
    fs.writeFileSync(path.join(project, 'pnpm-workspace.yaml'), 'httpsProxy: http://project.test/\n')
    const configHome = path.join(home, 'config')
    fs.mkdirSync(path.join(configHome, 'pnpm'), { recursive: true })
    fs.writeFileSync(path.join(configHome, 'pnpm', 'config.yaml'), 'httpsProxy: http://yaml-https.test/\nnoProxy: yaml.test\n')
    const env = { XDG_CONFIG_HOME: configHome, HTTPS_PROXY: 'http://env.test/' }

    mock.method(process, 'cwd', () => cwd)
    assert.deepEqual(pnpmProxyConfig({ homeDir: home, env }), {
      HTTPS_PROXY: 'http://yaml-https.test/',
      HTTP_PROXY: 'http://npmrc-http.test/',
      NO_PROXY: 'yaml.test',
    })
    assert.deepEqual(pnpmProxyConfig({ homeDir: home, env: { ...env, PNPM_CONFIG_HTTPS_PROXY: 'http://pnpm-env.test/', pnpm_config_no_proxy: 'pnpm-env.test' } }), {
      HTTPS_PROXY: 'http://pnpm-env.test/',
      HTTP_PROXY: 'http://npmrc-http.test/',
      NO_PROXY: 'pnpm-env.test',
    })
  })

  test('reads a value that is empty, false, or null as unset, falling through to the environment', () => {
    const { home } = layout({ user: ['https-proxy=http://npmrc.test/', 'no-proxy=npmrc.test'] })
    const configHome = path.join(home, 'config')
    fs.mkdirSync(path.join(configHome, 'pnpm'), { recursive: true })
    fs.writeFileSync(path.join(configHome, 'pnpm', 'config.yaml'), 'httpsProxy: ""\nnoProxy: false\n')

    assert.deepEqual(pnpmProxyConfig({ homeDir: home, env: { XDG_CONFIG_HOME: configHome, HTTPS_PROXY: 'http://env.test/', NO_PROXY: 'env.test' } }), {
      HTTPS_PROXY: 'http://env.test/',
      HTTP_PROXY: 'http://env.test/',
      NO_PROXY: 'env.test',
    })
  })

  test('takes the legacy proxy of the .npmrc, and turns the proxy off with proxy=false', () => {
    const legacy = layout({ user: ['proxy=http://legacy.test/'] })
    assert.deepEqual(pnpmProxyConfig({ homeDir: legacy.home, env: { HTTPS_PROXY: 'http://env.test/' } }), {
      HTTPS_PROXY: 'http://legacy.test/',
      HTTP_PROXY: 'http://legacy.test/',
    })

    const disabled = layout({ user: ['proxy=false'] })
    assert.deepEqual(pnpmProxyConfig({ homeDir: disabled.home, env: { HTTPS_PROXY: 'http://env.test/', HTTP_PROXY: 'http://env.test/' } }), {})
  })

  test('bypasses the proxy for every host when the global config.yaml sets noProxy: true', () => {
    const { home } = layout()
    const configHome = path.join(home, 'config')
    fs.mkdirSync(path.join(configHome, 'pnpm'), { recursive: true })
    fs.writeFileSync(path.join(configHome, 'pnpm', 'config.yaml'), 'httpProxy: http://yaml.test/\nnoProxy: true\n')

    assert.deepEqual(pnpmProxyConfig({ homeDir: home, env: { XDG_CONFIG_HOME: configHome } }), {
      HTTP_PROXY: 'http://yaml.test/',
      NO_PROXY: '*',
    })
  })
})

function layout (npmrcs: { user?: string[], project?: string[] } = {}): { root: string, home: string, project: string, cwd: string } {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'get-pnpm-pnpm-config-'))
  tmpDirs.push(root)
  const home = path.join(root, 'home')
  const project = path.join(root, 'project')
  const cwd = path.join(project, 'packages', 'a')
  fs.mkdirSync(home)
  fs.mkdirSync(cwd, { recursive: true })
  if (npmrcs.user != null) fs.writeFileSync(path.join(home, '.npmrc'), `${npmrcs.user.join('\n')}\n`)
  if (npmrcs.project != null) fs.writeFileSync(path.join(project, '.npmrc'), `${npmrcs.project.join('\n')}\n`)
  return { root, home, project, cwd }
}

function escaped (pem: string): string {
  return pem.replaceAll('\n', '\\n')
}
