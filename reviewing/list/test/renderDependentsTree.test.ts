import { stripVTControlCharacters as stripAnsi } from 'util'
import { renderDependentsTree, renderDependentsJson, renderDependentsParseable } from '../lib/renderDependentsTree.js'
import { type DependentsTree } from '@pnpm/reviewing.dependencies-hierarchy'

// Shared fixture: target → mid-a → root-project (2 levels of dependents)
function deepTree (): DependentsTree[] {
  return [
    {
      name: 'target',
      version: '1.0.0',
      dependents: [
        {
          name: 'mid-a',
          version: '2.0.0',
          dependents: [
            { name: 'root-project', version: '0.0.0', depField: 'dependencies' },
          ],
        },
        { name: 'root-project', version: '0.0.0', depField: 'devDependencies' },
      ],
    },
  ]
}

describe('renderDependentsTree', () => {
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

    const output = stripAnsi(await renderDependentsTree(results, { long: false }))
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

    const output = stripAnsi(await renderDependentsTree(results, { long: false }))
    const lines = output.split('\n')

    expect(lines[0]).toBe('foo@1.0.0')
    // Second line should be part of the tree, not a message
    expect(lines[1]).not.toBe('')
    expect(lines[1]).toContain('my-project')
  })

  test('depth limits how deep the tree is rendered', async () => {
    const withDepth = stripAnsi(await renderDependentsTree(deepTree(), { long: false, depth: 1 }))
    const withoutDepth = stripAnsi(await renderDependentsTree(deepTree(), { long: false }))

    // Without depth, root-project appears twice: once nested under mid-a, once as direct dependent
    const fullLines = withoutDepth.split('\n')
    const rootProjectOccurrences = fullLines.filter(l => l.includes('root-project@0.0.0'))
    expect(rootProjectOccurrences).toHaveLength(2)

    // With depth 1, mid-a's children are not expanded, so root-project appears only once (as direct dependent)
    const limitedLines = withDepth.split('\n')
    const limitedRootProjectOccurrences = limitedLines.filter(l => l.includes('root-project@0.0.0'))
    expect(limitedRootProjectOccurrences).toHaveLength(1)
    // mid-a should still be visible
    expect(withDepth).toContain('mid-a@2.0.0')
  })

  test('renders displayName instead of name when provided', async () => {
    const results: DependentsTree[] = [
      {
        name: 'foo',
        displayName: 'my-component',
        version: '1.0.0',
        dependents: [
          {
            name: 'bar',
            displayName: 'other-component',
            version: '2.0.0',
            dependents: [
              { name: 'my-project', version: '0.0.0', depField: 'dependencies' },
            ],
          },
        ],
      },
    ]

    const output = stripAnsi(await renderDependentsTree(results, { long: false }))
    expect(output).toContain('my-component@1.0.0')
    expect(output).not.toContain('foo@1.0.0')
    expect(output).toContain('other-component@2.0.0')
    expect(output).not.toContain('bar@2.0.0')
    // Importer without displayName should still render its name
    expect(output).toContain('my-project@0.0.0')
  })

  test('falls back to name when displayName is undefined', async () => {
    const results: DependentsTree[] = [
      {
        name: 'foo',
        version: '1.0.0',
        dependents: [
          { name: 'my-project', version: '0.0.0', depField: 'dependencies' },
        ],
      },
    ]

    const output = stripAnsi(await renderDependentsTree(results, { long: false }))
    expect(output).toContain('foo@1.0.0')
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

    const output = stripAnsi(await renderDependentsTree(results, { long: false }))
    const lines = output.split('\n')

    expect(lines[0]).toBe('bar@2.0.0')
    expect(lines[1]).toBe('Found via license check')
  })
})

describe('whySummary', () => {
  test('single package, single version', async () => {
    const results: DependentsTree[] = [
      {
        name: 'foo',
        version: '1.0.0',
        dependents: [{ name: 'my-project', version: '0.0.0', depField: 'dependencies' }],
      },
    ]
    const output = stripAnsi(await renderDependentsTree(results, { long: false }))
    expect(output).toContain('Found 1 version of foo')
    expect(output).not.toContain('instances')
  })

  test('single package, multiple versions', async () => {
    const results: DependentsTree[] = [
      {
        name: 'foo',
        version: '1.0.0',
        dependents: [{ name: 'my-project', version: '0.0.0', depField: 'dependencies' }],
      },
      {
        name: 'foo',
        version: '2.0.0',
        dependents: [{ name: 'my-project', version: '0.0.0', depField: 'dependencies' }],
      },
    ]
    const output = stripAnsi(await renderDependentsTree(results, { long: false }))
    expect(output).toContain('Found 2 versions of foo')
    expect(output).not.toContain('instances')
  })

  test('single package, same version with multiple peer variants shows instance count', async () => {
    const results: DependentsTree[] = [
      {
        name: 'foo',
        version: '1.0.0',
        peersSuffixHash: 'aaaa',
        dependents: [{ name: 'my-project', version: '0.0.0', depField: 'dependencies' }],
      },
      {
        name: 'foo',
        version: '1.0.0',
        peersSuffixHash: 'bbbb',
        dependents: [{ name: 'other', version: '0.0.0', depField: 'dependencies' }],
      },
    ]
    const output = stripAnsi(await renderDependentsTree(results, { long: false }))
    expect(output).toContain('Found 1 version, 2 instances of foo')
  })

  test('multiple different packages each get their own summary line', async () => {
    const results: DependentsTree[] = [
      {
        name: 'foo',
        version: '1.0.0',
        dependents: [{ name: 'my-project', version: '0.0.0', depField: 'dependencies' }],
      },
      {
        name: 'bar',
        version: '2.0.0',
        dependents: [{ name: 'my-project', version: '0.0.0', depField: 'dependencies' }],
      },
      {
        name: 'bar',
        version: '3.0.0',
        dependents: [{ name: 'my-project', version: '0.0.0', depField: 'dependencies' }],
      },
    ]
    const output = stripAnsi(await renderDependentsTree(results, { long: false }))
    expect(output).toContain('Found 1 version of foo')
    expect(output).toContain('Found 2 versions of bar')
  })

  test('summary uses displayName when provided', async () => {
    const results: DependentsTree[] = [
      {
        name: 'foo',
        displayName: 'my-component',
        version: '1.0.0',
        dependents: [{ name: 'my-project', version: '0.0.0', depField: 'dependencies' }],
      },
      {
        name: 'foo',
        displayName: 'my-component',
        version: '2.0.0',
        dependents: [{ name: 'my-project', version: '0.0.0', depField: 'dependencies' }],
      },
    ]
    const output = stripAnsi(await renderDependentsTree(results, { long: false }))
    expect(output).toContain('Found 2 versions of my-component')
    expect(output).not.toContain('Found 2 versions of foo')
  })

  test('empty results produce no summary', async () => {
    const output = await renderDependentsTree([], { long: false })
    expect(output).toBe('')
  })
})

describe('renderDependentsJson', () => {
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

    const parsed = JSON.parse(await renderDependentsJson(results, { long: false }))
    expect(parsed).toHaveLength(1)
    expect(parsed[0].searchMessage).toBe('Matched by custom finder')
  })

  test('depth truncates dependents in JSON output', async () => {
    const parsed = JSON.parse(await renderDependentsJson(deepTree(), { long: false, depth: 1 }))
    expect(parsed).toHaveLength(1)
    const tree = parsed[0]
    // Direct dependents (depth 0) should be present
    expect(tree.dependents).toHaveLength(2)
    // mid-a should have its dependents stripped (depth 1 is beyond the limit)
    const midA = tree.dependents.find((d: any) => d.name === 'mid-a') // eslint-disable-line
    expect(midA).toBeDefined()
    expect(midA.dependents).toBeUndefined()
    // root-project (direct dependent) should still be present
    const root = tree.dependents.find((d: any) => d.name === 'root-project') // eslint-disable-line
    expect(root).toBeDefined()
  })

  test('no depth option preserves full dependents in JSON output', async () => {
    const parsed = JSON.parse(await renderDependentsJson(deepTree(), { long: false }))
    const tree = parsed[0]
    const midA = tree.dependents.find((d: any) => d.name === 'mid-a') // eslint-disable-line
    expect(midA.dependents).toHaveLength(1)
    expect(midA.dependents[0].name).toBe('root-project')
  })

  test('includes displayName in JSON output', async () => {
    const results: DependentsTree[] = [
      {
        name: 'foo',
        displayName: 'my-component',
        version: '1.0.0',
        dependents: [
          {
            name: 'bar',
            displayName: 'other-component',
            version: '2.0.0',
            dependents: [
              { name: 'my-project', version: '0.0.0', depField: 'dependencies' },
            ],
          },
        ],
      },
    ]

    const parsed = JSON.parse(await renderDependentsJson(results, { long: false }))
    expect(parsed[0].name).toBe('foo')
    expect(parsed[0].displayName).toBe('my-component')
    expect(parsed[0].dependents[0].name).toBe('bar')
    expect(parsed[0].dependents[0].displayName).toBe('other-component')
    // Nodes without displayName should not have the field
    expect(parsed[0].dependents[0].dependents[0].displayName).toBeUndefined()
  })

  test('does not include searchMessage when undefined', async () => {
    const results: DependentsTree[] = [
      {
        name: 'foo',
        version: '1.0.0',
        dependents: [],
      },
    ]

    const parsed = JSON.parse(await renderDependentsJson(results, { long: false }))
    expect(parsed[0].searchMessage).toBeUndefined()
  })
})

describe('renderDependentsParseable', () => {
  test('depth limits parseable output depth', () => {
    const output = renderDependentsParseable(deepTree(), { long: false, depth: 1 })
    const lines = output.split('\n')
    // With depth 1, mid-a cannot recurse further — it becomes a leaf
    // So we should get two lines:
    // 1. mid-a > target (mid-a treated as leaf since depth prevents expanding its children)
    // 2. root-project > target (direct dependent)
    expect(lines).toHaveLength(2)
    expect(lines.some(l => l === 'mid-a@2.0.0 > target@1.0.0')).toBe(true)
    expect(lines.some(l => l === 'root-project@0.0.0 > target@1.0.0')).toBe(true)
  })

  test('no depth option renders full paths in parseable output', () => {
    const output = renderDependentsParseable(deepTree(), { long: false })
    const lines = output.split('\n')
    // Without depth limit, mid-a is expanded to root-project
    expect(lines).toHaveLength(2)
    expect(lines.some(l => l === 'root-project@0.0.0 > mid-a@2.0.0 > target@1.0.0')).toBe(true)
    expect(lines.some(l => l === 'root-project@0.0.0 > target@1.0.0')).toBe(true)
  })

  test('uses displayName in parseable output', () => {
    const results: DependentsTree[] = [
      {
        name: 'foo',
        displayName: 'my-component',
        version: '1.0.0',
        dependents: [
          {
            name: 'bar',
            displayName: 'other-component',
            version: '2.0.0',
            dependents: [
              { name: 'my-project', version: '0.0.0', depField: 'dependencies' },
            ],
          },
        ],
      },
    ]

    const output = renderDependentsParseable(results, { long: false })
    const lines = output.split('\n')
    expect(lines).toHaveLength(1)
    expect(lines[0]).toBe('my-project@0.0.0 > other-component@2.0.0 > my-component@1.0.0')
  })

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

    const output = renderDependentsParseable(results, { long: false })
    const lines = output.split('\n')
    // Parseable output should still contain the path
    expect(lines).toHaveLength(1)
    expect(lines[0]).toContain('dep-a@1.0.0')
    expect(lines[0]).toContain('my-project@0.0.0')
  })
})
