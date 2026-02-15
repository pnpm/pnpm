import { stripVTControlCharacters as stripAnsi } from 'util'
import { renderWhyTree, renderWhyJson, renderWhyParseable } from '../lib/renderWhyTree.js'
import { type DependentsTree } from '@pnpm/reviewing.dependencies-hierarchy'

describe('renderWhyTree', () => {
  test('renders searchMessage below the root label', async () => {
    const results: DependentsTree[] = [
      {
        name: 'foo',
        version: '1.0.0',
        searchMessage: 'Matched by custom finder',
        dependents: [
          { name: 'my-project', version: '0.0.0', depField: 'dependencies' },
        ],
      },
    ]

    const output = stripAnsi(await renderWhyTree(results, { long: false }))
    const lines = output.split('\n')

    // Root label should be the package name@version
    expect(lines[0]).toContain('foo@1.0.0')
    // Search message should appear on a subsequent line
    expect(lines.some(l => l.includes('Matched by custom finder'))).toBe(true)
    // Dependent should still be rendered
    expect(lines.some(l => l.includes('my-project@0.0.0'))).toBe(true)
  })

  test('does not render extra line when searchMessage is undefined', async () => {
    const results: DependentsTree[] = [
      {
        name: 'foo',
        version: '1.0.0',
        dependents: [
          { name: 'my-project', version: '0.0.0', depField: 'dependencies' },
        ],
      },
    ]

    const output = stripAnsi(await renderWhyTree(results, { long: false }))
    const lines = output.split('\n')

    expect(lines[0]).toBe('foo@1.0.0')
    // Second line should be part of the tree, not a message
    expect(lines[1]).not.toBe('')
    expect(lines[1]).toContain('my-project')
  })

  test('renders package with no dependents and a searchMessage', async () => {
    const results: DependentsTree[] = [
      {
        name: 'bar',
        version: '2.0.0',
        searchMessage: 'Found via license check',
        dependents: [],
      },
    ]

    const output = stripAnsi(await renderWhyTree(results, { long: false }))
    const lines = output.split('\n')

    expect(lines[0]).toBe('bar@2.0.0')
    expect(lines[1]).toBe('Found via license check')
  })
})

describe('renderWhyJson', () => {
  test('includes searchMessage in JSON output', async () => {
    const results: DependentsTree[] = [
      {
        name: 'foo',
        version: '1.0.0',
        searchMessage: 'Matched by custom finder',
        dependents: [
          { name: 'my-project', version: '0.0.0', depField: 'dependencies' },
        ],
      },
    ]

    const parsed = JSON.parse(await renderWhyJson(results, { long: false }))
    expect(parsed).toHaveLength(1)
    expect(parsed[0].searchMessage).toBe('Matched by custom finder')
  })

  test('does not include searchMessage when undefined', async () => {
    const results: DependentsTree[] = [
      {
        name: 'foo',
        version: '1.0.0',
        dependents: [],
      },
    ]

    const parsed = JSON.parse(await renderWhyJson(results, { long: false }))
    expect(parsed[0].searchMessage).toBeUndefined()
  })
})

describe('renderWhyParseable', () => {
  test('renders parseable output with searchMessage result', () => {
    const results: DependentsTree[] = [
      {
        name: 'dep-a',
        version: '1.0.0',
        searchMessage: 'Found via custom check',
        dependents: [
          { name: 'my-project', version: '0.0.0', depField: 'dependencies' },
        ],
      },
    ]

    const output = renderWhyParseable(results, { long: false })
    const lines = output.split('\n')
    // Parseable output should still contain the path
    expect(lines).toHaveLength(1)
    expect(lines[0]).toContain('dep-a@1.0.0')
    expect(lines[0]).toContain('my-project@0.0.0')
  })
})
