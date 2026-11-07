import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import { stripTypeScriptTypes } from 'node:module'
import { test } from 'node:test'

const guide = await readFile(new globalThis.URL('../../docs/cloudflare.md', import.meta.url), 'utf8')
const source = guide.split('```ts title="src/index.ts"\n')[1].split('\n```')[0]
const javascript = stripTypeScriptTypes(source.replace(/^import .*;\n/m, ''))
  .replace('export class ', 'class ')
  .replace('export default ', 'const worker = ')

function fixture ({ startupError = false, forwardingError = false, workerError = false } = {}) {
  const errors = []
  const logger = { error: (...args) => errors.push(args) }
  class Container {
    constructor () { this.ctx = { container: { running: !startupError } } }
    async startAndWaitForPorts () { throw new Error('private startup details') }
    async fetch () {
      if (forwardingError) throw new Error('private forwarding details')
      return new globalThis.Response('registry answer', { headers: { 'x-registry': 'yes' } })
    }
  }
  const getContainer = (namespace) => {
    if (workerError) throw new Error('private Worker details')
    return namespace ?? new classes.PnprContainer({}, {})
  }
  const classes = new Function('Container', 'getContainer', 'console', `${javascript}\nreturn { PnprContainer, worker };`)(Container, getContainer, logger)
  return { ...classes, errors }
}

for (const failure of ['startupError', 'forwardingError', 'workerError']) {
  test(`the deployment Worker hides ${failure} and supplies a retry delay`, async () => {
    const { worker, errors } = fixture({ [failure]: true })
    const response = await worker.fetch(new globalThis.Request('https://registry.example/npm/widgets'), {})
    assert.equal(response.status, 503)
    assert.equal(response.headers.get('retry-after'), '5')
    assert.equal(await response.text(), 'Registry temporarily unavailable, retry shortly\n')
    assert.equal(errors.length, 1)
    assert.equal(errors[0][0], 'Registry request failed')
    assert.match(errors[0][1].stack, /private .* details/)
  })
}

test('the deployment Worker preserves a successful registry response', async () => {
  const { worker } = fixture()
  const response = await worker.fetch(new globalThis.Request('https://registry.example/npm/widgets'), {})
  assert.equal(response.status, 200)
  assert.equal(response.headers.get('x-registry'), 'yes')
  assert.equal(await response.text(), 'registry answer')
})

const { worker } = fixture()

for (const method of ['GET', 'HEAD', 'PUT', 'DELETE', 'POST']) {
  for (const path of ['/npm/%E0%A4%A', '/npm/%', '/npm/%GG', '/npm/widget@%FF', '/npm/~example/%C0%AF', '/other/%ED%A0%80']) {
    test(`${method} ${path} returns 400 without forwarding`, async () => {
      const response = await worker.fetch(new globalThis.Request(`https://registry.example${path}`, { method }), {
        PNPR: { fetch () { assert.fail('malformed path reached the container') } },
      })
      assert.equal(response.status, 400)
      assert.equal(await response.text(), 'Malformed percent-encoded path\n')
    })
  }
}

for (const path of ['/npm/widget', '/npm/@example%2Fwidget', '/npm/%E2%9C%93', '/npm/%252F', '/npm/widget?query=%', '/-/ping']) {
  test(`forwards valid path unchanged: ${path}`, async () => {
    const request = new globalThis.Request(`https://registry.example${path}`)
    let calls = 0
    const response = await worker.fetch(request, {
      PNPR: { async fetch (forwarded) {
        calls++
        assert.equal(forwarded.url, request.url)
        return new globalThis.Response('registry response', { status: 202 })
      } },
    })
    assert.equal(calls, 1)
    assert.equal(response.status, 202)
    assert.equal(await response.text(), 'registry response')
  })
}

