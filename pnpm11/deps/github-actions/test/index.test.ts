import fs from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { pathToFileURL } from 'node:url'

import { afterAll, afterEach, beforeEach, describe, expect, jest, test } from '@jest/globals'
import { safeExeca as execa } from 'execa'

jest.unstable_mockModule('@pnpm/logger', () => ({
  globalWarn: jest.fn(),
}))

const { globalWarn } = await import('@pnpm/logger')
const {
  expandGitHubActionSelectors,
  findOutdatedGitHubActions,
  hasPositiveGitHubActionSelector,
  isGitHubActionSelector,
  normalizeGitHubActionSelector,
  updateGitHubActions,
  withoutGitHubActionSelectors,
} = await import('@pnpm/deps.github-actions')

const dirs: string[] = []

// The homepage and git URL fall back to GITHUB_SERVER_URL, which is set on
// GitHub Actions runners — the tests assume the https://github.com default.
const originalGithubServerUrl = process.env.GITHUB_SERVER_URL

beforeEach(() => {
  delete process.env.GITHUB_SERVER_URL
})

afterAll(() => {
  if (originalGithubServerUrl == null) {
    delete process.env.GITHUB_SERVER_URL
  } else {
    process.env.GITHUB_SERVER_URL = originalGithubServerUrl
  }
})

afterEach(async () => {
  jest.mocked(globalWarn).mockClear()
  await Promise.all(dirs.splice(0).map(async (dir) => fs.rm(dir, { force: true, recursive: true })))
})

describe('GitHub Actions dependencies', () => {
  test('distinguishes action selectors from npm package selectors', () => {
    expect(isGitHubActionSelector('actions/checkout')).toBe(true)
    expect(isGitHubActionSelector('!actions/cache/save')).toBe(true)
    expect(isGitHubActionSelector('@scope/package')).toBe(false)
    expect(isGitHubActionSelector('!@scope/package')).toBe(false)
    expect(isGitHubActionSelector('pkg:npm/pkg-a')).toBe(false)
    expect(isGitHubActionSelector('!pkg:npm/pkg-a')).toBe(false)
    expect(isGitHubActionSelector('jsr:@std/path')).toBe(false)
    expect(isGitHubActionSelector('typescript')).toBe(false)
  })

  test('normalizes and expands action selectors without changing package selectors', () => {
    expect(normalizeGitHubActionSelector('actions/checkout@v4')).toBe('actions/checkout')
    expect(normalizeGitHubActionSelector('!actions/checkout@v4')).toBe('!actions/checkout')
    expect(normalizeGitHubActionSelector('actions/checkout')).toBe('actions/checkout')
    expect(normalizeGitHubActionSelector('@scope/package')).toBe('@scope/package')
    expect(normalizeGitHubActionSelector('pkg:npm/pkg-a@1.0.0')).toBe('pkg:npm/pkg-a@1.0.0')
    expect(expandGitHubActionSelectors(['actions/cache@v4', '!actions/cache/save', '!actions/checkout'])).toEqual([
      'actions/cache',
      'actions/cache/*',
      '!actions/cache/save',
      '!actions/checkout',
      '!actions/checkout/*',
    ])
    expect(hasPositiveGitHubActionSelector(['!actions/checkout'])).toBe(false)
    expect(hasPositiveGitHubActionSelector(['actions/checkout'])).toBe(true)
    expect(withoutGitHubActionSelectors(['actions/checkout', '!lodash'])).toEqual([])
    expect(withoutGitHubActionSelectors(['!lodash'])).toEqual(['!lodash'])
    expect(withoutGitHubActionSelectors(['!actions/checkout', '!lodash'])).toEqual(['!lodash'])
    expect(withoutGitHubActionSelectors(['react', 'actions/checkout', '!lodash'])).toEqual(['react', '!lodash'])
  })

  test('updates workflows using YAML anchors, aliases, merge keys, and multibyte comments once', async () => {
    const dir = await fixture({
      '.github/workflows/ci.yml': `x-checkout: &top_checkout
  uses: actions/checkout@${'a'.repeat(40)} # v4.1.1000
jobs:
  first:
    runs-on: ubuntu-latest
    steps: &shared
      - uses: 'actions/checkout@${'a'.repeat(40)}' # v4.1.1000
      - <<: *top_checkout
        name: Merged checkout
# café: 🌍🌍🌍🌍
  second:
    runs-on: ubuntu-latest
    steps: *shared
`,
    })
    await updateGitHubActions({
      dir,
      readRepoRefs: async () => repoRefs([
        ['v4.1.1000', 'a'.repeat(40)],
        ['v4.2.0', 'b'.repeat(40)],
      ]),
    })
    await expect(fs.readFile(path.join(dir, '.github/workflows/ci.yml'), 'utf8')).resolves.toBe(`x-checkout: &top_checkout
  uses: actions/checkout@${'b'.repeat(40)} # v4.2.0
jobs:
  first:
    runs-on: ubuntu-latest
    steps: &shared
      - uses: 'actions/checkout@${'b'.repeat(40)}' # v4.2.0
      - <<: *top_checkout
        name: Merged checkout
# café: 🌍🌍🌍🌍
  second:
    runs-on: ubuntu-latest
    steps: *shared
`)
  })

  test('does not re-walk shared YAML merge-key DAGs exponentially', async () => {
    const branches = Array.from({ length: 20 }, (_, idx) => idx)
    const dagLines = ['m0: &m0 { runs-on: ubuntu-latest }']
    for (let level = 1; level <= 8; level++) {
      const refs = branches.map(() => `*m${level - 1}`).join(', ')
      dagLines.push(`m${level}: &m${level} { <<: [${refs}] }`)
    }
    const dir = await fixture({
      '.github/workflows/ci.yml': `${dagLines.join('\n')}\njobs:\n  build:\n    <<: *m8\n`,
    })
    await expect(findOutdatedGitHubActions({
      dir,
      readRepoRefs: async () => ({}),
    })).resolves.toEqual([])
  })

  test('discovers actions on a shallower merge branch after a deep branch hits the depth cap', async () => {
    const dir = await fixture({
      '.github/workflows/ci.yml': `base: &base
  uses: actions/checkout@${'a'.repeat(40)} # v4.1.0
shared: &shared
  <<: *base
d7: &d7 { <<: *shared }
d6: &d6 { <<: *d7 }
d5: &d5 { <<: *d6 }
d4: &d4 { <<: *d5 }
d3: &d3 { <<: *d4 }
d2: &d2 { <<: *d3 }
d1: &d1 { <<: *d2 }
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - <<: [*d1, *shared]
        name: Checkout
`,
    })
    await updateGitHubActions({
      dir,
      readRepoRefs: async () => repoRefs([
        ['v4.1.0', 'a'.repeat(40)],
        ['v4.2.0', 'b'.repeat(40)],
      ]),
    })
    await expect(fs.readFile(path.join(dir, '.github/workflows/ci.yml'), 'utf8')).resolves.toContain(
      `uses: actions/checkout@${'b'.repeat(40)} # v4.2.0`
    )
  })

  test('updates actions inherited through root and jobs-map merges with direct overrides', async () => {
    const shaOld = 'a'.repeat(40)
    const shaNew = 'b'.repeat(40)
    const dir = await fixture({
      '.github/workflows/root-merged.yml': `template: &template
  jobs:
    lint:
      runs-on: ubuntu-latest
      steps:
        - uses: actions/checkout@${shaOld} # v4.1.0
<<: *template
`,
      '.github/workflows/jobs-merged.yml': `ignored_root: &ignored_root
  jobs:
    shadowed_root_job:
      runs-on: ubuntu-latest
      steps:
        - uses: actions/checkout@${shaOld} # v4.1.0
shared_jobs: &shared_jobs
  inherited_job:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@${shaOld} # v4.1.0
  overridden_job:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@${shaOld} # v4.1.0
<<: *ignored_root
jobs:
  <<: *shared_jobs
  overridden_job:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/setup-node@${shaOld} # v4.1.0
`,
    })
    await updateGitHubActions({
      dir,
      readRepoRefs: async () => repoRefs([
        ['v4.1.0', shaOld],
        ['v4.2.0', shaNew],
      ]),
    })
    await expect(fs.readFile(path.join(dir, '.github/workflows/root-merged.yml'), 'utf8')).resolves.toContain(
      `uses: actions/checkout@${shaNew} # v4.2.0`
    )
    const jobsMerged = await fs.readFile(path.join(dir, '.github/workflows/jobs-merged.yml'), 'utf8')
    expect(jobsMerged).toBe(`ignored_root: &ignored_root
  jobs:
    shadowed_root_job:
      runs-on: ubuntu-latest
      steps:
        - uses: actions/checkout@${shaOld} # v4.1.0
shared_jobs: &shared_jobs
  inherited_job:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@${shaNew} # v4.2.0
  overridden_job:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@${shaOld} # v4.1.0
<<: *ignored_root
jobs:
  <<: *shared_jobs
  overridden_job:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/setup-node@${shaNew} # v4.2.0
`)
  })

  test('ignores quoted and str-tagged << keys while honoring explicit !!merge tags and next-line anchors', async () => {
    const shaOld = 'a'.repeat(40)
    const shaNew = 'b'.repeat(40)
    const dir = await fixture({
      '.github/workflows/ci.yml': `inactive_root: &inactive_root
  jobs:
    unused:
      runs-on: ubuntu-latest
      steps:
        - uses: actions/setup-python@${shaOld} # v4.1.0
inactive_step: &inactive_step
  uses: actions/cache@${shaOld} # v4.1.0
pin: &pin
  actions/checkout@${shaOld} # v4.1.0
active_step: &active_step
  uses: *pin
active_jobs: &active_jobs
  "bu\\u0069ld":
    runs-on: ubuntu-latest
    steps:
      - "<<": *inactive_step
        !!merge <<: *active_step
      - '<<': *inactive_step
        run: echo single-quoted
      - !!str <<: *inactive_step
        run: echo str-tagged
  "li\\u006et":
    runs-on: ubuntu-latest
    steps:
      - uses: actions/cache@${shaOld} # v4.1.0
"<<": *inactive_root
jobs:
  '<<': *inactive_root
  !!merge <<: *active_jobs
  lint:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/setup-node@${shaOld} # v4.1.0
`,
    })
    const updated = await updateGitHubActions({
      dir,
      readRepoRefs: async () => repoRefs([
        ['v4.1.0', shaOld],
        ['v4.2.0', shaNew],
      ]),
    })
    expect(updated.map((action) => action.name).sort()).toEqual(['actions/checkout', 'actions/setup-node'])
    const content = await fs.readFile(path.join(dir, '.github/workflows/ci.yml'), 'utf8')
    expect(content).toContain(`uses: actions/setup-python@${shaOld} # v4.1.0`)
    expect(content).toContain(`uses: actions/cache@${shaOld} # v4.1.0`)
    expect(content).toContain(`pin: &pin\n  actions/checkout@${shaNew} # v4.2.0`)
    expect(content).toContain(`uses: actions/setup-node@${shaNew} # v4.2.0`)
  })

  test('stops fallback after merge depth cutoff while keeping direct overrides', async () => {
    const shaOld = 'a'.repeat(40)
    const shaNew = 'b'.repeat(40)
    const lines = [
      `s0: &s0 { uses: actions/checkout@${shaOld} }`,
      `shadowed_step: &shadowed_step { uses: actions/setup-node@${shaOld} }`,
      'j0: &j0',
      '  ci:',
      '    runs-on: ubuntu-latest',
      '    steps:',
      `      - uses: actions/checkout@${shaOld}`,
      'shadowed_jobs: &shadowed_jobs',
      '  ci:',
      '    runs-on: ubuntu-latest',
      '    steps:',
      `      - uses: actions/setup-python@${shaOld}`,
    ]
    for (let depth = 1; depth <= 65; depth++) {
      lines.push(`s${depth}: &s${depth} { <<: *s${depth - 1} }`)
      lines.push(`j${depth}: &j${depth} { <<: *j${depth - 1} }`)
    }
    lines.push(
      'jobs:',
      '  <<: [*j65, *shadowed_jobs]',
      '  direct:',
      '    runs-on: ubuntu-latest',
      '    steps:',
      '      - <<: [*s65, *shadowed_step]',
      '        name: Deep cutoff step',
      `      - uses: actions/cache@${shaOld}`,
      '        <<: [*s65, *shadowed_step]'
    )
    const dir = await fixture({
      '.github/workflows/ci.yml': `${lines.join('\n')}\n`,
    })
    const updated = await updateGitHubActions({
      dir,
      readRepoRefs: async () => repoRefs([
        ['v4.1.0', shaOld],
        ['v4.2.0', shaNew],
      ]),
    })
    expect(updated.map((action) => action.name)).toEqual(['actions/cache'])
    const content = await fs.readFile(path.join(dir, '.github/workflows/ci.yml'), 'utf8')
    expect(content).toContain(`actions/setup-node@${shaOld}`)
    expect(content).toContain(`actions/setup-python@${shaOld}`)
    expect(content).toContain(`actions/cache@${shaNew} # v4.2.0`)
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

  test('follows self-repository references to local composite actions', async () => {
    const dir = await fixture({
      '.github/workflows/ci.yml': `jobs:
  test:
    steps:
      - uses: $/.github/actions/setup
`,
      '.github/actions/setup/action.yml': `runs:
  using: composite
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
    })).resolves.toEqual([
      {
        current: '4.1.0',
        homepage: 'https://github.com/actions/checkout',
        latest: '4.2.0',
        name: 'actions/checkout',
        wanted: '4.2.0',
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

  test('redacts credentials and control characters from the skip warning', async () => {
    const dir = await fixture({
      '.github/workflows/ci.yml': `jobs:
  test:
    steps:
      - uses: owner/private-action@v1.0.0
`,
    })

    await findOutdatedGitHubActions({
      dir,
      readRepoRefs: async () => {
        throw new Error('fatal: unable to access \u001b[31mhttps://user:token@ghes.example.com/owner/private-action.git\u001b[0m\nnot found')
      },
    })
    expect(globalWarn).toHaveBeenCalledWith('Skipping the GitHub Actions from "owner/private-action": fatal: unable to access [31mhttps://ghes.example.com/owner/private-action.git[0mnot found')
  })

  test('rejects a server URL that is not http(s)', async () => {
    const dir = await fixture({
      '.github/workflows/ci.yml': `jobs:
  test:
    steps:
      - uses: actions/checkout@v4.1.0
`,
    })

    await expect(findOutdatedGitHubActions({
      dir,
      serverUrl: 'ext::sh -c date',
    })).rejects.toMatchObject({ code: 'ERR_PNPM_GITHUB_ACTIONS_SERVER_PROTOCOL' })
  })

  test.each([findOutdatedGitHubActions, updateGitHubActions])('redacts credentials from homepages (%p)', async (operation) => {
    const dir = await fixture({ '.github/workflows/ci.yml': 'jobs: { test: { steps: [{ uses: actions/checkout@v4.1.0 }] } }\n' })
    const results = await operation({
      dir,
      serverUrl: 'https://username:secret@github.example.com',
      readRepoRefs: async () => repoRefs([['v4.1.0', 'a'.repeat(40)], ['v4.2.0', 'b'.repeat(40)]]),
    })
    expect(results[0].homepage).toBe('https://github.example.com/actions/checkout')
  })

  test.each([
    'http://username:secret@github.example.com',
    'http://127.example.com',
    'http://localhost.example.com',
    'https://',
    'ext::sh -c secret',
  ])('rejects insecure or invalid servers before looking up refs: %s', async (serverUrl) => {
    const dir = await fixture({ '.github/workflows/ci.yml': 'jobs: { test: { steps: [{ uses: actions/checkout@v4.1.0 }] } }\n' })
    const readRepoRefs = jest.fn(async () => ({}))
    await expect(updateGitHubActions({ dir, serverUrl, readRepoRefs })).rejects.toMatchObject({
      code: 'ERR_PNPM_GITHUB_ACTIONS_SERVER_PROTOCOL',
      message: 'The GitHub Actions server URL must use HTTPS, except for HTTP on loopback hosts',
    })
    expect(readRepoRefs).not.toHaveBeenCalled()
  })

  test.each(['localhost', '127.0.0.1', '127.2.3.4', '[::1]'])('allows HTTP on loopback host %s', async (host) => {
    const dir = await fixture({ '.github/workflows/ci.yml': 'jobs: { test: { steps: [{ uses: actions/checkout@v4.1.0 }] } }\n' })
    const readRepoRefs = jest.fn(async () => repoRefs([['v4.1.0', 'a'.repeat(40)], ['v4.2.0', 'b'.repeat(40)]]))
    const results = await findOutdatedGitHubActions({ dir, serverUrl: `http://${host}:8080`, readRepoRefs })
    expect(readRepoRefs).toHaveBeenCalledTimes(1)
    expect(results[0].homepage).toBe(`http://${host}:8080/actions/checkout`)
  })

  test.each([
    ['changed action', 'jobs: { test: { steps: [{ uses: actions/checkout@v4.2.0 }] } }\n'],
    ['truncated file', 'jobs: {}\n'],
    ['multibyte replacement', 'jobs: { test: { steps: [{ uses: 🦀🦀🦀🦀🦀🦀🦀🦀🦀🦀🦀🦀 }] } }\n'],
  ])('rejects stale workflow ranges after lookup: %s', async (_scenario, changed) => {
    const dir = await fixture({ '.github/workflows/ci.yml': 'jobs: { test: { steps: [{ uses: actions/checkout@v4.1.0 }] } }\n' })
    const workflow = path.join(dir, '.github/workflows/ci.yml')
    await expect(updateGitHubActions({
      dir,
      readRepoRefs: async () => {
        await fs.writeFile(workflow, changed)
        return repoRefs([['v4.1.0', 'a'.repeat(40)], ['v4.2.0', 'b'.repeat(40)]])
      },
    })).rejects.toMatchObject({
      code: 'ERR_PNPM_GITHUB_ACTIONS_WORKFLOW_CHANGED',
      message: `GitHub Actions workflow ${await fs.realpath(workflow)} changed while resolving updates; retry the command`,
    })
    await expect(fs.readFile(workflow, 'utf8')).resolves.toBe(changed)
  })

  test('preserves unrelated workflow edits made during lookup', async () => {
    const original = 'name: 🦀\njobs:\n  test:\n    steps:\n      - uses: actions/checkout@v4.1.0\n'
    const dir = await fixture({ '.github/workflows/ci.yml': original })
    const workflow = path.join(dir, '.github/workflows/ci.yml')
    const changed = original.replace('🦀', '🌍') + '# concurrent edit\n'
    await updateGitHubActions({
      dir,
      readRepoRefs: async () => {
        await fs.writeFile(workflow, changed)
        return repoRefs([['v4.1.0', 'a'.repeat(40)], ['v4.2.0', 'b'.repeat(40)]])
      },
    })
    await expect(fs.readFile(workflow, 'utf8')).resolves.toBe(changed.replace('actions/checkout@v4.1.0', `actions/checkout@${'b'.repeat(40)} # v4.2.0`))
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

  test('updates the comment version when the action name contains the same text', async () => {
    const dir = await fixture({
      '.github/workflows/ci.yml': `jobs:
  test:
    steps:
      - uses: owner/setup-v4.1.0@${'a'.repeat(40)} # v4.1.0
`,
    })
    await updateGitHubActions({
      dir,
      readRepoRefs: async () => repoRefs([
        ['v4.1.0', 'a'.repeat(40)],
        ['v4.2.0', 'b'.repeat(40)],
      ]),
    })
    await expect(fs.readFile(path.join(dir, '.github/workflows/ci.yml'), 'utf8')).resolves.toContain(`uses: owner/setup-v4.1.0@${'b'.repeat(40)} # v4.2.0`)
  })

  test('updates partial version comments and non-version comments without mangling', async () => {
    const dir = await fixture({
      '.github/workflows/ci.yml': `jobs:
  test:
    steps:
      - uses: actions/checkout@${'a'.repeat(40)} # v4
      - uses: actions/checkout@${'a'.repeat(40)} # v4.1
      - uses: actions/checkout@${'a'.repeat(40)} # 4
      - uses: actions/checkout@${'a'.repeat(40)} # v4 — pin for CI
      - uses: actions/checkout@${'a'.repeat(40)} # pinned manually
      - uses: actions/checkout@${'a'.repeat(40)} # 2 runners only
      - uses: actions/checkout@${'a'.repeat(40)} # 3.11 toolchain
      - uses: actions/checkout@${'a'.repeat(40)} #
`,
    })
    await updateGitHubActions({
      dir,
      readRepoRefs: async () => repoRefs([
        ['v4.1.0', 'a'.repeat(40)],
        ['v4.2.0', 'b'.repeat(40)],
      ]),
    })
    await expect(fs.readFile(path.join(dir, '.github/workflows/ci.yml'), 'utf8')).resolves.toBe(`jobs:
  test:
    steps:
      - uses: actions/checkout@${'b'.repeat(40)} # v4.2.0
      - uses: actions/checkout@${'b'.repeat(40)} # v4.2.0
      - uses: actions/checkout@${'b'.repeat(40)} # v4.2.0
      - uses: actions/checkout@${'b'.repeat(40)} # v4.2.0 — pin for CI
      - uses: actions/checkout@${'b'.repeat(40)} # v4.2.0 pinned manually
      - uses: actions/checkout@${'b'.repeat(40)} # v4.2.0 2 runners only
      - uses: actions/checkout@${'b'.repeat(40)} # v4.2.0 3.11 toolchain
      - uses: actions/checkout@${'b'.repeat(40)} # v4.2.0
`)
  })

  test('preserves distinct outdated versions across workflows while collapsing duplicates', async () => {
    const dir = await fixture({
      '.github/workflows/ci.yml': `jobs:
  test:
    steps:
      - uses: actions/checkout@v4.1.0
      - uses: actions/checkout@v4.1.0
`,
      '.github/workflows/release.yml': `jobs:
  release:
    steps:
      - uses: actions/checkout@v3.6.0
`,
    })
    const refs = repoRefs([
      ['v3.6.0', '1'.repeat(40)],
      ['v3.6.1', '2'.repeat(40)],
      ['v4.1.0', 'a'.repeat(40)],
      ['v4.2.0', 'b'.repeat(40)],
      ['v5.0.0', 'c'.repeat(40)],
    ])
    await expect(findOutdatedGitHubActions({
      dir,
      readRepoRefs: async () => refs,
    })).resolves.toEqual([
      {
        current: '3.6.0',
        homepage: 'https://github.com/actions/checkout',
        latest: '5.0.0',
        name: 'actions/checkout',
        wanted: '3.6.1',
      },
      {
        current: '4.1.0',
        homepage: 'https://github.com/actions/checkout',
        latest: '5.0.0',
        name: 'actions/checkout',
        wanted: '4.2.0',
      },
    ])
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

describe('minimumReleaseAge', () => {
  const now = Date.now()
  const checkoutRefs = repoRefs([
    ['v4.1.0', 'a'.repeat(40)],
    ['v4.2.0', 'b'.repeat(40)],
    ['v5.0.0', 'c'.repeat(40)],
  ])
  const tagDates: Record<string, Date> = {
    'v4.2.0': new Date(now - 2 * 24 * 60 * 60 * 1000),
    'v5.0.0': new Date(now - 60 * 1000),
  }
  const workflow = `jobs:
  test:
    steps:
      - uses: actions/checkout@${'a'.repeat(40)} # v4.1.0
`

  test('offers only versions older than the minimum release age', async () => {
    const dir = await fixture({ '.github/workflows/ci.yml': workflow })
    const readTagDates = jest.fn(async (_repo: string, tags: string[]) => Object.fromEntries(tags.map((tag) => [tag, tagDates[tag]])))

    await expect(findOutdatedGitHubActions({
      dir,
      minimumReleaseAge: 1440,
      readRepoRefs: async () => checkoutRefs,
      readTagDates,
    })).resolves.toMatchObject([{ current: '4.1.0', latest: '4.2.0', name: 'actions/checkout' }])
    expect(readTagDates).toHaveBeenCalledWith('actions/checkout', ['v4.2.0', 'v5.0.0'])
  })

  test('updates to the newest version old enough', async () => {
    const dir = await fixture({ '.github/workflows/ci.yml': workflow })

    await updateGitHubActions({
      dir,
      latest: true,
      minimumReleaseAge: 1440,
      readRepoRefs: async () => checkoutRefs,
      readTagDates: async () => tagDates,
    })

    await expect(fs.readFile(path.join(dir, '.github/workflows/ci.yml'), 'utf8')).resolves.toContain(`actions/checkout@${'b'.repeat(40)} # v4.2.0`)
  })

  test.each(['actions/checkout', 'actions/*', 'actions/checkout@5.0.0'])('minimumReleaseAgeExclude %s skips the check', async (exclude) => {
    const dir = await fixture({ '.github/workflows/ci.yml': workflow })

    await expect(findOutdatedGitHubActions({
      dir,
      minimumReleaseAge: 1440,
      minimumReleaseAgeExclude: [exclude],
      readRepoRefs: async () => checkoutRefs,
      readTagDates: async () => tagDates,
    })).resolves.toMatchObject([{ latest: '5.0.0' }])
  })

  test('skips actions whose release dates cannot be read and warns', async () => {
    const dir = await fixture({ '.github/workflows/ci.yml': workflow })

    await expect(findOutdatedGitHubActions({
      dir,
      minimumReleaseAge: 1440,
      readRepoRefs: async () => checkoutRefs,
      readTagDates: async () => {
        throw new Error('fetch failed')
      },
    })).resolves.toEqual([])
    expect(globalWarn).toHaveBeenCalledWith('Skipping the GitHub Actions from "actions/checkout": cannot read the release dates that minimumReleaseAge needs: fetch failed')
  })

  test('reads the tagger date of annotated tags and the commit date of lightweight ones', async () => {
    const repo = await fixture({})
    const day = 24 * 60 * 60
    const old = Math.floor(now / 1000) - 400 * day
    const git = async (args: string[], date: number) => execa('git', ['-c', 'user.name=pnpm', '-c', 'user.email=pnpm@example.com', ...args], {
      cwd: repo,
      // The user's own git config may sign tags or commits.
      env: { ...process.env, GIT_AUTHOR_DATE: `${date} +0000`, GIT_COMMITTER_DATE: `${date} +0000`, GIT_CONFIG_GLOBAL: path.join(repo, '.no-global-config'), GIT_CONFIG_NOSYSTEM: '1' },
    })
    await git(['init', '--quiet'], old)
    await git(['commit', '--quiet', '--allow-empty', '-m', 'one'], old)
    await git(['tag', 'v1.0.0'], old)
    await git(['commit', '--quiet', '--allow-empty', '-m', 'two'], old + day)
    await git(['tag', 'v1.1.0'], old + day)
    // An old commit tagged just now is a new release.
    await git(['tag', '-a', '-m', 'v1.2.0', 'v1.2.0'], Math.floor(now / 1000))
    const dir = await fixture({
      '.github/workflows/ci.yml': `jobs:
  test:
    steps:
      - uses: owner/tool@v1.0.0
`,
    })
    const originalEnv = { ...process.env }
    Object.assign(process.env, {
      GIT_CONFIG_COUNT: '1',
      GIT_CONFIG_KEY_0: `url.${pathToFileURL(repo).href}.insteadOf`,
      GIT_CONFIG_VALUE_0: 'https://github.com/owner/tool.git',
    })
    try {
      await expect(findOutdatedGitHubActions({ dir, minimumReleaseAge: 1440 })).resolves.toMatchObject([{ latest: '1.1.0' }])
      await expect(findOutdatedGitHubActions({ dir })).resolves.toMatchObject([{ latest: '1.2.0' }])
    } finally {
      for (const name of ['GIT_CONFIG_COUNT', 'GIT_CONFIG_KEY_0', 'GIT_CONFIG_VALUE_0']) {
        if (originalEnv[name] == null) {
          delete process.env[name]
        } else {
          process.env[name] = originalEnv[name]
        }
      }
    }
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
