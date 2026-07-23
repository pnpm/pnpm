import fs from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'

import { afterEach, describe, expect, jest, test } from '@jest/globals'

jest.unstable_mockModule('@pnpm/logger', () => ({
  globalWarn: jest.fn(),
}))

const { globalWarn } = await import('@pnpm/logger')
const {
  findOutdatedGitHubActions,
  isGitHubActionSelector,
  normalizeGitHubActionSelector,
  updateGitHubActions,
} = await import('@pnpm/deps.github-actions')

const dirs: string[] = []

// The homepage and git URL fall back to GITHUB_SERVER_URL, which is set on
// GitHub Actions runners — the tests assume the https://github.com default.
delete process.env.GITHUB_SERVER_URL

afterEach(async () => {
  delete process.env.GITHUB_SERVER_URL
  jest.mocked(globalWarn).mockClear()
  await Promise.all(dirs.splice(0).map(async (dir) => fs.rm(dir, { force: true, recursive: true })))
})

describe('GitHub Actions dependencies', () => {
  test('distinguishes action selectors from npm package selectors', () => {
    expect(isGitHubActionSelector('actions/checkout')).toBe(true)
    expect(isGitHubActionSelector('@scope/package')).toBe(false)
    expect(isGitHubActionSelector('!@scope/package')).toBe(false)
    expect(isGitHubActionSelector('typescript')).toBe(false)
  })

  test('normalizes action selectors without changing package selectors', () => {
    expect(normalizeGitHubActionSelector('actions/checkout@v4')).toBe('actions/checkout')
    expect(normalizeGitHubActionSelector('!actions/checkout@v4')).toBe('!actions/checkout')
    expect(normalizeGitHubActionSelector('actions/checkout')).toBe('actions/checkout')
    expect(normalizeGitHubActionSelector('@scope/package')).toBe('@scope/package')
  })

  test('finds actions in workflows and referenced local composite actions', async () => {
    const dir = await fixture({
      '.github/workflows/ci.yml': `jobs:
  test:
    steps:
      - uses: actions/checkout@v4.1.0
      - uses: ./.github/actions/setup
      - uses: docker://alpine:3.20
`,
      '.github/actions/setup/action.yml': `runs:
  using: composite
  steps:
    - uses: owner/tool/subpath@aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa # v2.0.0
`,
    })

    const refs = new Map([
      ['actions/checkout', repoRefs([
        ['v4.1.0', 'a'.repeat(40)],
        ['v4.2.0', 'b'.repeat(40)],
        ['v5.0.0', 'c'.repeat(40)],
      ])],
      ['owner/tool', repoRefs([
        ['v2.0.0', 'a'.repeat(40)],
        ['v2.1.0', 'b'.repeat(40)],
      ])],
    ])

    await expect(findOutdatedGitHubActions({
      dir,
      readRepoRefs: async (repo) => refs.get(repo)!,
    })).resolves.toEqual([
      {
        current: '4.1.0',
        homepage: 'https://github.com/actions/checkout',
        latest: '5.0.0',
        name: 'actions/checkout',
        wanted: '4.2.0',
      },
      {
        current: '2.0.0',
        homepage: 'https://github.com/owner/tool',
        latest: '2.1.0',
        name: 'owner/tool/subpath',
        wanted: '2.1.0',
      },
    ])
  })

  test('builds homepages from the configured server URL', async () => {
    const dir = await fixture({
      '.github/workflows/ci.yml': `jobs:
  test:
    steps:
      - uses: actions/checkout@v4.1.0
`,
    })

    await expect(findOutdatedGitHubActions({
      dir,
      readRepoRefs: async () => repoRefs([
        ['v4.1.0', 'a'.repeat(40)],
        ['v4.2.0', 'b'.repeat(40)],
      ]),
      serverUrl: 'https://github.example.com/',
    })).resolves.toEqual([
      {
        current: '4.1.0',
        homepage: 'https://github.example.com/actions/checkout',
        latest: '4.2.0',
        name: 'actions/checkout',
        wanted: '4.2.0',
      },
    ])
  })

  test('falls back to the GITHUB_SERVER_URL environment variable for the server URL', async () => {
    process.env.GITHUB_SERVER_URL = 'https://ghes.example.com'
    const dir = await fixture({
      '.github/workflows/ci.yml': `jobs:
  test:
    steps:
      - uses: actions/checkout@v4.1.0
`,
    })

    const outdated = await findOutdatedGitHubActions({
      dir,
      readRepoRefs: async () => repoRefs([
        ['v4.1.0', 'a'.repeat(40)],
        ['v4.2.0', 'b'.repeat(40)],
      ]),
    })
    expect(outdated[0].homepage).toBe('https://ghes.example.com/actions/checkout')
  })

  test('skips actions whose repository refs cannot be read and warns', async () => {
    const dir = await fixture({
      '.github/workflows/ci.yml': `jobs:
  test:
    steps:
      - uses: actions/checkout@v4.1.0
      - uses: owner/private-action@v1.0.0
`,
    })

    await expect(findOutdatedGitHubActions({
      dir,
      readRepoRefs: async (repo) => {
        if (repo === 'owner/private-action') throw new Error('Repository not found.')
        return repoRefs([
          ['v4.1.0', 'a'.repeat(40)],
          ['v4.2.0', 'b'.repeat(40)],
        ])
      },
    })).resolves.toEqual([
      {
        current: '4.1.0',
        homepage: 'https://github.com/actions/checkout',
        latest: '4.2.0',
        name: 'actions/checkout',
        wanted: '4.2.0',
      },
    ])
    expect(globalWarn).toHaveBeenCalledTimes(1)
    expect(globalWarn).toHaveBeenCalledWith('Skipping the GitHub Actions from "owner/private-action": Repository not found.')
  })

  test('updates within the current major and preserves SHA comments and unrelated formatting', async () => {
    const dir = await fixture({
      '.github/workflows/ci.yml': `name: CI

jobs:
  test:
    strategy: { matrix: { node: [22, 24] } }
    steps:
      - uses: 'actions/checkout@aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa' # v4.1.0 # keep
      - name: nested input
        with:
          uses: actions/checkout@v4
      - uses: owner/floating@v2
`,
    })
    const refs = new Map([
      ['actions/checkout', repoRefs([
        ['v4.1.0', 'a'.repeat(40)],
        ['v4.2.0', 'b'.repeat(40)],
        ['v5.0.0', 'c'.repeat(40)],
      ])],
      ['owner/floating', repoRefs([
        ['v2.1.0', 'd'.repeat(40)],
        ['v3.0.0', 'e'.repeat(40)],
      ])],
    ])

    await updateGitHubActions({
      dir,
      readRepoRefs: async (repo) => refs.get(repo)!,
    })

    await expect(fs.readFile(path.join(dir, '.github/workflows/ci.yml'), 'utf8')).resolves.toBe(`name: CI

jobs:
  test:
    strategy: { matrix: { node: [22, 24] } }
    steps:
      - uses: 'actions/checkout@bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb' # v4.2.0 # keep
      - name: nested input
        with:
          uses: actions/checkout@v4
      - uses: owner/floating@dddddddddddddddddddddddddddddddddddddddd # v2.1.0
`)
  })

  test('--latest pins a floating major tag to the newest release commit', async () => {
    const dir = await fixture({
      '.github/workflows/ci.yml': `jobs:
  test:
    steps:
      - uses: actions/checkout@v4
`,
    })
    await updateGitHubActions({
      dir,
      latest: true,
      readRepoRefs: async () => repoRefs([
        ['v4.2.0', 'a'.repeat(40)],
        ['v5.0.0', 'b'.repeat(40)],
      ]),
    })
    await expect(fs.readFile(path.join(dir, '.github/workflows/ci.yml'), 'utf8')).resolves.toContain(`uses: actions/checkout@${'b'.repeat(40)} # v5.0.0`)
  })

  test('pins an already-current floating major tag', async () => {
    const dir = await fixture({
      '.github/workflows/ci.yml': `jobs:
  test:
    steps:
      - uses: actions/checkout@v4
`,
    })
    await updateGitHubActions({
      dir,
      readRepoRefs: async () => repoRefs([
        ['v4.2.0', 'a'.repeat(40)],
        ['v5.0.0', 'b'.repeat(40)],
      ]),
    })
    await expect(fs.readFile(path.join(dir, '.github/workflows/ci.yml'), 'utf8')).resolves.toContain(`uses: actions/checkout@${'a'.repeat(40)} # v4.2.0`)
  })

  test('keeps pre-1.0 updates within the caret-compatible range unless latest is requested', async () => {
    const dir = await fixture({
      '.github/workflows/ci.yml': `jobs:
  test:
    steps:
      - uses: owner/tool@v0.5.7
`,
    })
    const refs = repoRefs([
      ['v0.5.7', 'a'.repeat(40)],
      ['v0.5.9', 'b'.repeat(40)],
      ['v0.6.0', 'c'.repeat(40)],
    ])

    await expect(findOutdatedGitHubActions({ compatible: true, dir, readRepoRefs: async () => refs })).resolves.toEqual([
      {
        current: '0.5.7',
        homepage: 'https://github.com/owner/tool',
        latest: '0.5.9',
        name: 'owner/tool',
        wanted: '0.5.9',
      },
    ])

    await updateGitHubActions({ dir, readRepoRefs: async () => refs })
    await expect(fs.readFile(path.join(dir, '.github/workflows/ci.yml'), 'utf8')).resolves.toContain(`uses: owner/tool@${'b'.repeat(40)} # v0.5.9`)

    await updateGitHubActions({ dir, latest: true, readRepoRefs: async () => refs })
    await expect(fs.readFile(path.join(dir, '.github/workflows/ci.yml'), 'utf8')).resolves.toContain(`uses: owner/tool@${'c'.repeat(40)} # v0.6.0`)
  })

  test('updates prerelease tags containing dots', async () => {
    const dir = await fixture({
      '.github/workflows/ci.yml': `jobs:
  test:
    steps:
      - uses: actions/checkout@v5.0.0-alpha.1
`,
    })

    await updateGitHubActions({
      dir,
      readRepoRefs: async () => repoRefs([
        ['v5.0.0-alpha.1', 'a'.repeat(40)],
        ['v5.0.0-alpha.2', 'b'.repeat(40)],
      ]),
    })

    await expect(fs.readFile(path.join(dir, '.github/workflows/ci.yml'), 'utf8')).resolves.toContain(`uses: actions/checkout@${'b'.repeat(40)} # v5.0.0-alpha.2`)
  })

  test('updates flow-style steps without commenting out their delimiters', async () => {
    const dir = await fixture({
      '.github/workflows/ci.yml': `jobs:
  test:
    steps: [{ uses: actions/checkout@v4 }]
`,
    })
    const refs = repoRefs([
      ['v4.2.0', 'a'.repeat(40)],
      ['v5.0.0', 'b'.repeat(40)],
    ])

    await updateGitHubActions({ dir, readRepoRefs: async () => refs })

    await expect(fs.readFile(path.join(dir, '.github/workflows/ci.yml'), 'utf8')).resolves.toBe(`jobs:
  test:
    steps: [{ uses: actions/checkout@${'a'.repeat(40)} # v4.2.0
                    }]
`)
    await expect(findOutdatedGitHubActions({ compatible: true, dir, readRepoRefs: async () => refs })).resolves.toEqual([])
  })

  test('limits concurrent repository lookups', async () => {
    const actionCount = 12
    const dir = await fixture({
      '.github/workflows/ci.yml': `jobs:
  test:
    steps:
${Array.from({ length: actionCount }, (_, index) => `      - uses: owner/action-${index}@v1`).join('\n')}
`,
    })
    let active = 0
    let maxActive = 0

    const outdated = await findOutdatedGitHubActions({
      dir,
      readRepoRefs: async () => {
        active++
        maxActive = Math.max(maxActive, active)
        await new Promise((resolve) => setTimeout(resolve, 5))
        active--
        return repoRefs([
          ['v1.0.0', 'a'.repeat(40)],
          ['v2.0.0', 'b'.repeat(40)],
        ])
      },
    })

    expect(outdated).toHaveLength(actionCount)
    expect(maxActive).toBeLessThanOrEqual(8)
  })

  test('reports invalid workflow YAML with a stable contextual error', async () => {
    const dir = await fixture({
      '.github/workflows/ci.yml': 'jobs: [\n',
    })
    const workflow = await fs.realpath(path.join(dir, '.github/workflows/ci.yml'))

    await expect(findOutdatedGitHubActions({ dir })).rejects.toMatchObject({
      code: 'ERR_PNPM_GITHUB_ACTIONS_WORKFLOW_PARSE',
      message: expect.stringContaining(workflow),
    })
  })

  test('rejects workflow directory symlinks outside the project', async () => {
    const dir = await fixture({})
    const outsideDir = await fs.mkdtemp(path.join(os.tmpdir(), 'pnpm-actions-outside-'))
    dirs.push(outsideDir)
    const outsideWorkflow = path.join(outsideDir, 'ci.yml')
    const original = `jobs:
  test:
    steps:
      - uses: actions/checkout@v4
`
    await fs.writeFile(outsideWorkflow, original)
    await fs.mkdir(path.join(dir, '.github'), { recursive: true })
    await fs.symlink(outsideDir, path.join(dir, '.github/workflows'), 'junction')

    await expect(updateGitHubActions({ dir })).rejects.toMatchObject({
      code: 'ERR_PNPM_GITHUB_ACTIONS_WORKFLOW_OUTSIDE_ROOT',
    })
    await expect(fs.readFile(outsideWorkflow, 'utf8')).resolves.toBe(original)
  })

  test('does not mutate an external hardlink target', async () => {
    const dir = await fixture({})
    const outsideDir = await fs.mkdtemp(path.join(os.tmpdir(), 'pnpm-actions-outside-'))
    dirs.push(outsideDir)
    const outsideWorkflow = path.join(outsideDir, 'ci.yml')
    const workflow = path.join(dir, '.github/workflows/ci.yml')
    const original = `jobs:
  test:
    steps:
      - uses: actions/checkout@v4
`
    await fs.writeFile(outsideWorkflow, original)
    await fs.mkdir(path.dirname(workflow), { recursive: true })
    await fs.link(outsideWorkflow, workflow)

    await updateGitHubActions({
      dir,
      readRepoRefs: async () => repoRefs([
        ['v4.2.0', 'a'.repeat(40)],
        ['v5.0.0', 'b'.repeat(40)],
      ]),
    })

    await expect(fs.readFile(workflow, 'utf8')).resolves.toContain(`uses: actions/checkout@${'a'.repeat(40)} # v4.2.0`)
    await expect(fs.readFile(outsideWorkflow, 'utf8')).resolves.toBe(original)
  })
})

async function fixture (files: Record<string, string>): Promise<string> {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), 'pnpm-actions-'))
  dirs.push(dir)
  await Promise.all(Object.entries(files).map(async ([relativePath, content]) => {
    const filePath = path.join(dir, relativePath)
    await fs.mkdir(path.dirname(filePath), { recursive: true })
    await fs.writeFile(filePath, content)
  }))
  return dir
}

function repoRefs (versions: Array<[string, string]>): Record<string, string> {
  return Object.fromEntries(versions.map(([tag, commit]) => [`refs/tags/${tag}`, commit]))
}
