import assert from 'node:assert/strict'
import { Buffer } from 'node:buffer'
import { readFile } from 'node:fs/promises'
import { stripTypeScriptTypes } from 'node:module'
import { test } from 'node:test'

const guide = await readFile(new globalThis.URL('../../docs/cloudflare.md', import.meta.url), 'utf8')
const source = guide.match(/```ts title="src\/index.ts"\n([\s\S]*?)\n```/)[1]
const stubbed = source.replace(
  'import { Container, getContainer } from "@cloudflare/containers";',
  'class Container {}; function getContainer(env) { return env }',
)
const { default: worker } = await import(`data:text/javascript;base64,${Buffer.from(stripTypeScriptTypes(stubbed)).toString('base64')}`)

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

test('container errors propagate', async () => {
  const error = new URIError('origin failure')
  await assert.rejects(worker.fetch(new globalThis.Request('https://registry.example/npm/widget'), {
    PNPR: { fetch () { throw error } },
  }), error)
})
