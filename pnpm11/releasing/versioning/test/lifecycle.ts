import fs from 'node:fs/promises'
import path from 'node:path'

import { expect, jest, test } from '@jest/globals'
import {
  applyReleasePlan,
  assembleReleasePlan,
  parseChangeIntent,
  prependChangelogSection,
  readChangeIntents,
  readLedger,
  readPendingChangelog,
  type WorkspaceProject,
  writeChangeIntent,
} from '@pnpm/releasing.versioning'
import { temporaryDirectory } from 'tempy'

async function makeWorkspace (pkgs: Array<{ name: string, version: string, deps?: Record<string, string> }>): Promise<{ workspaceDir: string, projects: WorkspaceProject[] }> {
  const workspaceDir = temporaryDirectory()
  const projects = await Promise.all(pkgs.map(async (pkg) => {
    const rootDir = path.join(workspaceDir, pkg.name.replace(/[@/]/g, '_'))
    await fs.mkdir(rootDir, { recursive: true })
    const manifest = { name: pkg.name, version: pkg.version, dependencies: pkg.deps }
    await fs.writeFile(path.join(rootDir, 'package.json'), JSON.stringify(manifest, null, 2) + '\n')
    return { rootDir, manifest }
  }))
  return { workspaceDir, projects }
}

test('parseChangeIntent reads the changesets file format', () => {
  const intent = parseChangeIntent([
    '---',
    '"@example/ui": minor',
    '"@example/core": patch',
    '---',
    '',
    'Added a `variant` prop to `Button`.',
    '',
  ].join('\n'), 'brave-pandas-smile', '/x/brave-pandas-smile.md')
  expect(intent.releases).toStrictEqual({ '@example/ui': 'minor', '@example/core': 'patch' })
  expect(intent.summary).toBe('Added a `variant` prop to `Button`.')
})

test('parseChangeIntent tolerates a UTF-8 BOM and CRLF line endings', () => {
  const intent = parseChangeIntent('\uFEFF---\r\nfoo: patch\r\n---\r\n\r\nA fix.\r\n', 'id', '/x/id.md')
  expect(intent.releases).toStrictEqual({ foo: 'patch' })
  expect(intent.summary).toBe('A fix.')
})

test('prependChangelogSection keeps the title above the new section even without a trailing newline', async () => {
  const dir = temporaryDirectory()
  await fs.writeFile(path.join(dir, 'CHANGELOG.md'), '# lib')
  await prependChangelogSection(dir, 'lib', '## 1.0.1\n\n### Patch Changes\n\n- A fix.\n')
  const changelog = await fs.readFile(path.join(dir, 'CHANGELOG.md'), 'utf8')
  expect(changelog.startsWith('# lib\n\n## 1.0.1')).toBe(true)
})

test('parseChangeIntent rejects an invalid bump type', () => {
  expect(() => parseChangeIntent('---\nfoo: gigantic\n---\nx', 'id', '/x/id.md')).toThrow(/invalid bump type/)
})

test('writeChangeIntent output round-trips through readChangeIntents', async () => {
  const workspaceDir = temporaryDirectory()
  const id = await writeChangeIntent(workspaceDir, {
    releases: { '@example/ui': 'minor' },
    summary: 'Added a thing.',
  })
  const intents = await readChangeIntents(workspaceDir)
  expect(intents).toHaveLength(1)
  expect(intents[0].id).toBe(id)
  expect(intents[0].releases).toStrictEqual({ '@example/ui': 'minor' })
  expect(intents[0].summary).toBe('Added a thing.')
})

const REPOSITORY = { changelog: { storage: 'repository' } } as const

test('applyReleasePlan bumps manifests, writes changelogs, records the ledger, and deletes consumed intents', async () => {
  const { workspaceDir, projects } = await makeWorkspace([
    { name: 'lib', version: '1.0.0' },
    { name: 'cli', version: '2.0.0', deps: { lib: 'workspace:*' } },
  ])
  await writeChangeIntent(workspaceDir, {
    releases: { lib: 'minor' },
    summary: 'Added a feature.',
  })
  const intents = await readChangeIntents(workspaceDir)
  const plan = assembleReleasePlan({ workspaceDir, projects, intents, ledger: await readLedger(workspaceDir) })

  const applied = await applyReleasePlan(plan, { workspaceDir, projects, allIntents: intents, versioning: REPOSITORY })
  expect(applied.map((release) => `${release.name}@${release.newVersion}`).sort()).toStrictEqual(['cli@2.0.1', 'lib@1.1.0'])

  const libDir = projects.find((project) => project.manifest.name === 'lib')!.rootDir
  const cliDir = projects.find((project) => project.manifest.name === 'cli')!.rootDir
  expect(JSON.parse(await fs.readFile(path.join(libDir, 'package.json'), 'utf8')).version).toBe('1.1.0')
  expect(JSON.parse(await fs.readFile(path.join(cliDir, 'package.json'), 'utf8')).version).toBe('2.0.1')

  const libChangelog = await fs.readFile(path.join(libDir, 'CHANGELOG.md'), 'utf8')
  expect(libChangelog).toContain('# lib')
  expect(libChangelog).toContain('## 1.1.0')
  expect(libChangelog).toContain('### Minor Changes')
  expect(libChangelog).toContain('- Added a feature.')

  const cliChangelog = await fs.readFile(path.join(cliDir, 'CHANGELOG.md'), 'utf8')
  expect(cliChangelog).toContain('## 2.0.1')
  expect(cliChangelog).toContain('- Updated dependencies:')
  expect(cliChangelog).toContain('  - lib@1.1.0')

  const ledger = await readLedger(workspaceDir)
  expect(Object.keys(ledger)).toStrictEqual(['lib@1.1.0'])

  expect(await readChangeIntents(workspaceDir)).toHaveLength(0)
})

test('intent files consumed only by lane prereleases survive until graduation', async () => {
  const { workspaceDir, projects } = await makeWorkspace([
    { name: 'cli', version: '2.0.0' },
  ])
  await writeChangeIntent(workspaceDir, {
    releases: { cli: 'minor' },
    summary: 'Added a feature.',
  })
  const versioning = { lanes: { cli: 'alpha' }, changelog: { storage: 'repository' } } as const

  let intents = await readChangeIntents(workspaceDir)
  const prereleasePlan = assembleReleasePlan({ workspaceDir, projects, intents, ledger: await readLedger(workspaceDir), versioning })
  expect(prereleasePlan.releases[0].newVersion).toBe('2.1.0-alpha.0')
  await applyReleasePlan(prereleasePlan, { workspaceDir, projects, allIntents: intents, versioning })

  // The prose is still needed for the stable changelog at graduation.
  intents = await readChangeIntents(workspaceDir)
  expect(intents).toHaveLength(1)

  // Return to the main lane: the accumulated stable version releases and the
  // intent is garbage-collected.
  const graduatedProjects: WorkspaceProject[] = [{
    rootDir: projects[0].rootDir,
    manifest: { name: 'cli', version: '2.1.0-alpha.0' },
  }]
  const graduationPlan = assembleReleasePlan({ workspaceDir, projects: graduatedProjects, intents, ledger: await readLedger(workspaceDir), versioning: {} })
  expect(graduationPlan.releases[0].newVersion).toBe('2.1.0')
  await applyReleasePlan(graduationPlan, { workspaceDir, projects: graduatedProjects, allIntents: intents, versioning: REPOSITORY })

  const changelog = await fs.readFile(path.join(projects[0].rootDir, 'CHANGELOG.md'), 'utf8')
  expect(changelog).toContain('## 2.1.0-alpha.0')
  expect(changelog).toContain('## 2.1.0')
  expect(await readChangeIntents(workspaceDir)).toHaveLength(0)
})

test('registry storage (the default) parks the section instead of committing CHANGELOG.md and defers intent GC', async () => {
  const { workspaceDir, projects } = await makeWorkspace([{ name: 'lib', version: '1.0.0' }])
  await writeChangeIntent(workspaceDir, { releases: { lib: 'minor' }, summary: 'Added a feature.' })
  const intents = await readChangeIntents(workspaceDir)
  const plan = assembleReleasePlan({ workspaceDir, projects, intents, ledger: await readLedger(workspaceDir) })

  // No versioning => registry storage. No verifyPublished, so nothing is
  // confirmed published and the intent (the only copy of the prose) survives.
  await applyReleasePlan(plan, { workspaceDir, projects, allIntents: intents })

  await expect(fs.readFile(path.join(projects[0].rootDir, 'CHANGELOG.md'), 'utf8')).rejects.toThrow()
  const section = await readPendingChangelog(workspaceDir, 'lib', '1.1.0')
  expect(section).toContain('## 1.1.0')
  expect(section).toContain('- Added a feature.')
  expect(await readChangeIntents(workspaceDir)).toHaveLength(1)
})

test('registry storage collects an intent and its parked section once the registry confirms publication', async () => {
  const { workspaceDir, projects } = await makeWorkspace([{ name: 'lib', version: '1.0.0' }])
  await writeChangeIntent(workspaceDir, { releases: { lib: 'minor' }, summary: 'Added a feature.' })
  const firstIntents = await readChangeIntents(workspaceDir)
  const plan = assembleReleasePlan({ workspaceDir, projects, intents: firstIntents, ledger: await readLedger(workspaceDir) })
  await applyReleasePlan(plan, { workspaceDir, projects, allIntents: firstIntents })

  const released: WorkspaceProject[] = [{ rootDir: projects[0].rootDir, manifest: { name: 'lib', version: '1.1.0' } }]
  const intents = await readChangeIntents(workspaceDir)
  const verifyPublished = jest.fn(async (_name: string, _version: string, section: string) => section.includes('## 1.1.0'))
  const emptyPlan = assembleReleasePlan({ workspaceDir, projects: released, intents, ledger: await readLedger(workspaceDir) })
  expect(emptyPlan.releases).toHaveLength(0)
  await applyReleasePlan(emptyPlan, { workspaceDir, projects: released, allIntents: intents, verifyPublished })

  expect(verifyPublished).toHaveBeenCalledWith('lib', '1.1.0', expect.stringContaining('## 1.1.0'))
  expect(await readChangeIntents(workspaceDir)).toHaveLength(0)
  expect(await readPendingChangelog(workspaceDir, 'lib', '1.1.0')).toBeNull()
})

test('registry storage keeps an intent whose release the registry has not confirmed', async () => {
  const { workspaceDir, projects } = await makeWorkspace([{ name: 'lib', version: '1.0.0' }])
  await writeChangeIntent(workspaceDir, { releases: { lib: 'minor' }, summary: 'Added a feature.' })
  const firstIntents = await readChangeIntents(workspaceDir)
  const plan = assembleReleasePlan({ workspaceDir, projects, intents: firstIntents, ledger: await readLedger(workspaceDir) })
  await applyReleasePlan(plan, { workspaceDir, projects, allIntents: firstIntents })

  const released: WorkspaceProject[] = [{ rootDir: projects[0].rootDir, manifest: { name: 'lib', version: '1.1.0' } }]
  const intents = await readChangeIntents(workspaceDir)
  const emptyPlan = assembleReleasePlan({ workspaceDir, projects: released, intents, ledger: await readLedger(workspaceDir) })
  await applyReleasePlan(emptyPlan, { workspaceDir, projects: released, allIntents: intents, verifyPublished: async () => false })

  expect(await readChangeIntents(workspaceDir)).toHaveLength(1)
  expect(await readPendingChangelog(workspaceDir, 'lib', '1.1.0')).toContain('## 1.1.0')
})

test('registry storage collects the parked section of a dependency-only release once published', async () => {
  const { workspaceDir, projects } = await makeWorkspace([
    { name: 'lib', version: '1.0.0' },
    { name: 'cli', version: '2.0.0', deps: { lib: 'workspace:*' } },
  ])
  await writeChangeIntent(workspaceDir, { releases: { lib: 'minor' }, summary: 'A feature.' })
  const firstIntents = await readChangeIntents(workspaceDir)
  const plan = assembleReleasePlan({ workspaceDir, projects, intents: firstIntents, ledger: await readLedger(workspaceDir) })
  await applyReleasePlan(plan, { workspaceDir, projects, allIntents: firstIntents })

  // cli was bumped only because lib changed: it has a parked section but,
  // carrying no consumed intents, no ledger entry.
  expect(await readPendingChangelog(workspaceDir, 'cli', '2.0.1')).toContain('## 2.0.1')
  expect(Object.keys(await readLedger(workspaceDir))).toStrictEqual(['lib@1.1.0'])

  const released: WorkspaceProject[] = [
    { rootDir: projects[0].rootDir, manifest: { name: 'lib', version: '1.1.0' } },
    { rootDir: projects[1].rootDir, manifest: { name: 'cli', version: '2.0.1', dependencies: { lib: 'workspace:*' } } },
  ]
  const intents = await readChangeIntents(workspaceDir)
  const emptyPlan = assembleReleasePlan({ workspaceDir, projects: released, intents, ledger: await readLedger(workspaceDir) })
  await applyReleasePlan(emptyPlan, { workspaceDir, projects: released, allIntents: intents, verifyPublished: async () => true })

  // The dependency-only section is collected even though it has no ledger entry.
  expect(await readPendingChangelog(workspaceDir, 'cli', '2.0.1')).toBeNull()
  expect(await readPendingChangelog(workspaceDir, 'lib', '1.1.0')).toBeNull()
  expect(await readChangeIntents(workspaceDir)).toHaveLength(0)
})

test('a ledger entry named __proto__ stays an own key and cannot pollute the prototype', async () => {
  const workspaceDir = temporaryDirectory()
  await fs.mkdir(path.join(workspaceDir, '.changeset'))
  await fs.writeFile(path.join(workspaceDir, '.changeset', 'ledger.yaml'), '__proto__:\n  - sneaky\nlib@1.0.1:\n  - one\n')
  const ledger = await readLedger(workspaceDir)
  expect(({} as Record<string, unknown>).sneaky).toBeUndefined()
  expect(Object.keys(ledger).sort()).toStrictEqual(['__proto__', 'lib@1.0.1'])
})

test('a none-only intent is garbage-collected by a run with an empty plan', async () => {
  const { workspaceDir, projects } = await makeWorkspace([
    { name: 'lib', version: '1.0.0' },
  ])
  await writeChangeIntent(workspaceDir, { releases: { lib: 'none' }, summary: 'refactor, no release needed' })
  const intents = await readChangeIntents(workspaceDir)
  const plan = assembleReleasePlan({ workspaceDir, projects, intents, ledger: await readLedger(workspaceDir) })
  expect(plan.releases).toHaveLength(0)
  await applyReleasePlan(plan, { workspaceDir, projects, allIntents: intents })
  expect(await readChangeIntents(workspaceDir)).toHaveLength(0)
})

test('a merge-resurrected intent whose id is already in the ledger stays inert and is garbage-collected', async () => {
  const { workspaceDir, projects } = await makeWorkspace([
    { name: 'lib', version: '1.0.1' },
  ])
  await writeChangeIntent(workspaceDir, { releases: { lib: 'patch' }, summary: 'A fix.' })
  const intents = await readChangeIntents(workspaceDir)
  // Simulate the entry arriving from another line's release via a merge.
  const ledger = { ['lib@1.0.1']: [intents[0].id] }
  const plan = assembleReleasePlan({ workspaceDir, projects, intents, ledger })
  expect(plan.releases).toHaveLength(0)
})
