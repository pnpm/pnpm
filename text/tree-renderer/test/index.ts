import { renderTree, type TreeNode, type TreeNodeGroup } from '@pnpm/text.tree-renderer'

test('single root with no children', () => {
  expect(renderTree({ label: 'root' })).toBe('root\n')
})

test('single root with empty nodes array', () => {
  expect(renderTree({ label: 'root', nodes: [] })).toBe('root\n')
})

test('single child (leaf)', () => {
  expect(renderTree({
    label: 'root',
    nodes: [{ label: 'child' }],
  })).toBe(
    'root\n' +
    '└── child\n'
  )
})

test('multiple children', () => {
  expect(renderTree({
    label: 'root',
    nodes: [
      { label: 'a' },
      { label: 'b' },
      { label: 'c' },
    ],
  })).toBe(
    'root\n' +
    '├── a\n' +
    '├── b\n' +
    '└── c\n'
  )
})

test('nested children with correct prefix propagation', () => {
  expect(renderTree({
    label: 'root',
    nodes: [
      {
        label: 'a',
        nodes: [
          { label: 'a1' },
          { label: 'a2' },
        ],
      },
      { label: 'b' },
    ],
  })).toBe(
    'root\n' +
    '├─┬ a\n' +
    '│ ├── a1\n' +
    '│ └── a2\n' +
    '└── b\n'
  )
})

test('last child with children uses └─┬', () => {
  expect(renderTree({
    label: 'root',
    nodes: [
      { label: 'a' },
      {
        label: 'b',
        nodes: [
          { label: 'b1' },
        ],
      },
    ],
  })).toBe(
    'root\n' +
    '├── a\n' +
    '└─┬ b\n' +
    '  └── b1\n'
  )
})

test('deep nesting (3+ levels)', () => {
  expect(renderTree({
    label: 'root',
    nodes: [
      {
        label: 'a',
        nodes: [
          {
            label: 'b',
            nodes: [
              { label: 'c' },
              { label: 'd' },
            ],
          },
        ],
      },
    ],
  })).toBe(
    'root\n' +
    '└─┬ a\n' +
    '  └─┬ b\n' +
    '    ├── c\n' +
    '    └── d\n'
  )
})

test('sibling trees with deep nesting', () => {
  expect(renderTree({
    label: 'root',
    nodes: [
      {
        label: 'a',
        nodes: [
          {
            label: 'a1',
            nodes: [{ label: 'a1x' }],
          },
        ],
      },
      {
        label: 'b',
        nodes: [
          { label: 'b1' },
        ],
      },
    ],
  })).toBe(
    'root\n' +
    '├─┬ a\n' +
    '│ └─┬ a1\n' +
    '│   └── a1x\n' +
    '└─┬ b\n' +
    '  └── b1\n'
  )
})

test('multiline labels on node with children', () => {
  expect(renderTree({
    label: 'root',
    nodes: [
      {
        label: 'pkg@1.0.0\nA description\nhttps://example.com',
        nodes: [{ label: 'child' }],
      },
      {
        label: 'leaf@2.0.0\nAnother description',
      },
    ],
  })).toBe(
    'root\n' +
    '├─┬ pkg@1.0.0\n' +
    '│ │ A description\n' +
    '│ │ https://example.com\n' +
    '│ └── child\n' +
    '└── leaf@2.0.0\n' +
    '    Another description\n'
  )
})

test('multiline label on leaf node', () => {
  expect(renderTree({
    label: 'root',
    nodes: [
      {
        label: 'pkg@1.0.0\nA description',
      },
      {
        label: 'last@2.0.0\nAnother description',
      },
    ],
  })).toBe(
    'root\n' +
    '├── pkg@1.0.0\n' +
    '│   A description\n' +
    '└── last@2.0.0\n' +
    '    Another description\n'
  )
})

test('multiline label on root', () => {
  expect(renderTree({
    label: 'root\nsecond line',
    nodes: [{ label: 'child' }],
  })).toBe(
    'root\n' +
    '│ second line\n' +
    '└── child\n'
  )
})

test('string nodes in array', () => {
  expect(renderTree({
    label: 'root',
    nodes: [
      'string-child-1',
      { label: 'object-child' },
      'string-child-2',
    ],
  })).toBe(
    'root\n' +
    '├── string-child-1\n' +
    '├── object-child\n' +
    '└── string-child-2\n'
  )
})

test('treeChars formatter option', () => {
  const wrapped = (s: string) => `[${s}]`
  expect(renderTree({
    label: 'root',
    nodes: [
      { label: 'a' },
      { label: 'b' },
    ],
  }, { treeChars: wrapped })).toBe(
    'root\n' +
    '[├── ]a\n' +
    '[└── ]b\n'
  )
})

test('treeChars formatter with nested children', () => {
  const wrapped = (s: string) => `[${s}]`
  expect(renderTree({
    label: 'root',
    nodes: [
      {
        label: 'a',
        nodes: [{ label: 'a1' }],
      },
      { label: 'b' },
    ],
  }, { treeChars: wrapped })).toBe(
    'root\n' +
    '[├─┬ ]a\n' +
    '[│ └── ]a1\n' +
    '[└── ]b\n'
  )
})

test('treeChars formatter with multiline labels', () => {
  const wrapped = (s: string) => `[${s}]`
  expect(renderTree({
    label: 'root',
    nodes: [
      {
        label: 'pkg\ndescription',
      },
      { label: 'b' },
    ],
  }, { treeChars: wrapped })).toBe(
    'root\n' +
    '[├── ]pkg\n' +
    '[│   ]description\n' +
    '[└── ]b\n'
  )
})

test('unicode: false uses ASCII characters', () => {
  expect(renderTree({
    label: 'root',
    nodes: [
      {
        label: 'a',
        nodes: [
          { label: 'a1' },
          { label: 'a2' },
        ],
      },
      { label: 'b' },
    ],
  }, { unicode: false })).toBe(
    'root\n' +
    '+-- a\n' +
    '| +-- a1\n' +
    '| `-- a2\n' +
    '`-- b\n'
  )
})

test('string input is treated as label', () => {
  expect(renderTree('just a string')).toBe('just a string\n')
})

test('matches archy output for pnpm list-like structure', () => {
  // Simulate the pnpm list structure: root → group headers + deps as flat siblings
  const tree: TreeNode = {
    label: 'fixture@1.0.0 /path',
    nodes: [
      { label: 'dependencies:', nodes: [] },
      {
        label: 'write-json-file@2.3.0',
        nodes: [
          { label: 'detect-indent@5.0.0' },
          { label: 'graceful-fs@4.2.2' },
        ],
      },
      { label: 'devDependencies:', nodes: [] },
      { label: 'is-positive@3.1.0' },
    ],
  }
  expect(renderTree(tree)).toBe(
    'fixture@1.0.0 /path\n' +
    '├── dependencies:\n' +
    '├─┬ write-json-file@2.3.0\n' +
    '│ ├── detect-indent@5.0.0\n' +
    '│ └── graceful-fs@4.2.2\n' +
    '├── devDependencies:\n' +
    '└── is-positive@3.1.0\n'
  )
})

test('single group', () => {
  expect(renderTree({
    label: 'root',
    nodes: [
      { group: 'dependencies:', nodes: [{ label: 'a' }, { label: 'b' }] },
    ],
  })).toBe(
    'root\n' +
    '│\n' +
    '│   dependencies:\n' +
    '├── a\n' +
    '└── b\n'
  )
})

test('multiple groups with last/not-last spanning across all', () => {
  expect(renderTree({
    label: 'root',
    nodes: [
      { group: 'dependencies:', nodes: [{ label: 'a' }] },
      { group: 'devDependencies:', nodes: [{ label: 'b' }] },
    ],
  })).toBe(
    'root\n' +
    '│\n' +
    '│   dependencies:\n' +
    '├── a\n' +
    '│\n' +
    '│   devDependencies:\n' +
    '└── b\n'
  )
})

test('groups with nested children', () => {
  expect(renderTree({
    label: 'root',
    nodes: [
      {
        group: 'dependencies:',
        nodes: [
          {
            label: 'write-json-file@2.3.0',
            nodes: [
              { label: 'detect-indent@5.0.0' },
              { label: 'graceful-fs@4.2.2' },
            ],
          },
        ],
      },
      { group: 'devDependencies:', nodes: [{ label: 'is-positive@3.1.0' }] },
    ],
  })).toBe(
    'root\n' +
    '│\n' +
    '│   dependencies:\n' +
    '├─┬ write-json-file@2.3.0\n' +
    '│ ├── detect-indent@5.0.0\n' +
    '│ └── graceful-fs@4.2.2\n' +
    '│\n' +
    '│   devDependencies:\n' +
    '└── is-positive@3.1.0\n'
  )
})

test('groups with treeChars formatter', () => {
  const wrapped = (s: string) => `[${s}]`
  expect(renderTree({
    label: 'root',
    nodes: [
      { group: 'deps:', nodes: [{ label: 'a' }] },
      { group: 'dev:', nodes: [{ label: 'b' }] },
    ],
  }, { treeChars: wrapped })).toBe(
    'root\n' +
    '[│]\n' +
    '[│   ]deps:\n' +
    '[├── ]a\n' +
    '[│]\n' +
    '[│   ]dev:\n' +
    '[└── ]b\n'
  )
})

test('mixed groups and plain nodes', () => {
  expect(renderTree({
    label: 'root',
    nodes: [
      { label: 'plain-first' },
      { group: 'dependencies:', nodes: [{ label: 'a' }] },
    ],
  })).toBe(
    'root\n' +
    '├── plain-first\n' +
    '│\n' +
    '│   dependencies:\n' +
    '└── a\n'
  )
})

test('empty group is skipped', () => {
  expect(renderTree({
    label: 'root',
    nodes: [
      { group: 'empty:', nodes: [] } as TreeNodeGroup,
      { group: 'deps:', nodes: [{ label: 'a' }] },
    ],
  })).toBe(
    'root\n' +
    '│\n' +
    '│   deps:\n' +
    '└── a\n'
  )
})

test('multiline label with only empty groups uses plain continuation', () => {
  // When all groups are empty, items.length is 0, so the multiline
  // continuation prefix should be plain spaces, not │.
  expect(renderTree({
    label: 'root',
    nodes: [
      {
        label: 'pkg@1.0.0\nA description',
        nodes: [
          { group: 'dependencies:', nodes: [] } as TreeNodeGroup,
        ],
      },
      { label: 'other' },
    ],
  })).toBe(
    'root\n' +
    '├── pkg@1.0.0\n' +
    '│   A description\n' +
    '└── other\n'
  )
})

test('child with only empty groups uses ── connector, not ─┬', () => {
  // A child whose nodes array contains only empty groups has no renderable
  // children, so its connector should be ── (leaf) not ─┬ (branch).
  expect(renderTree({
    label: 'root',
    nodes: [
      {
        label: 'a',
        nodes: [
          { group: 'deps:', nodes: [] } as TreeNodeGroup,
          { group: 'dev:', nodes: [] } as TreeNodeGroup,
        ],
      },
    ],
  })).toBe(
    'root\n' +
    '└── a\n'
  )
})

test('group matching pnpm list output', () => {
  const tree: TreeNode = {
    label: 'my-pkg@1.0.0 /path',
    nodes: [
      {
        group: 'dependencies:',
        nodes: [
          {
            label: 'write-json-file@2.3.0',
            nodes: [
              { label: 'detect-indent@5.0.0' },
              { label: 'graceful-fs@4.2.2' },
            ],
          },
        ],
      },
      {
        group: 'devDependencies:',
        nodes: [
          { label: 'is-positive@3.1.0' },
        ],
      },
      {
        group: 'optionalDependencies:',
        nodes: [
          { label: 'is-negative@2.1.0' },
        ],
      },
    ],
  }
  expect(renderTree(tree)).toBe(
    'my-pkg@1.0.0 /path\n' +
    '│\n' +
    '│   dependencies:\n' +
    '├─┬ write-json-file@2.3.0\n' +
    '│ ├── detect-indent@5.0.0\n' +
    '│ └── graceful-fs@4.2.2\n' +
    '│\n' +
    '│   devDependencies:\n' +
    '├── is-positive@3.1.0\n' +
    '│\n' +
    '│   optionalDependencies:\n' +
    '└── is-negative@2.1.0\n'
  )
})
