import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import vm from 'node:vm'

function element () {
  return {
    value: '',
    dataset: {},
    children: [],
    listeners: {},
    append (child) { this.children.push(child) },
    replaceChildren (...children) { this.children = children },
    get firstChild () { return this.children[0] },
    set innerHTML (value) { throw new Error('HTML interpolation is forbidden: ' + value) },
    addEventListener (name, callback) { this.listeners[name] = callback },
  }
}

test('run data is rendered as text and credentials are not persisted', async () => {
  const nodes = new Map(['token', 'status', 'runs', 'detail', 'workspace', 'refresh'].map(id => [id, element()]))
  const body = element()
  nodes.get('runs').querySelector = () => body
  const payload = '<img src=x onerror="stealToken()">'
  const source = readFileSync(new URL('../src/server/pipeline_ui.html', import.meta.url), 'utf8').split('<script>')[1].split('</script>')[0]
  const context = vm.createContext({
    document: { getElementById: id => nodes.get(id), createElement: element },
    fetch: async () => ({ ok: true, json: async () => ({ runs: [{ workspace: payload, runId: '100-default', summary: { pipeline: payload, base: payload, selection: { mode: payload }, tasks: { build: null } } }] }) }),
    localStorage: new Proxy({}, { get () { throw new Error('credentials must stay in memory') } }),
  })
  vm.runInContext(source, context)
  await context.refresh()
  assert.equal(body.children.length, 1)
  const row = body.children[0]
  assert.equal(row.children[1].textContent, payload)
  assert.equal(row.children[2].textContent, payload)
  assert.equal(row.children[3].textContent, payload)
  assert.equal(row.dataset.workspace, payload)
})
