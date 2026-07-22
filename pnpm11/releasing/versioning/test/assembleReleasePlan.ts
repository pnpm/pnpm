import { expect, test } from '@jest/globals'
import {
  assembleReleasePlan,
  type ChangeIntent,
  checkVersioningInvariants,
  type Ledger,
  materializeWorkspaceRange,
  type WorkspaceProject,
} from '@pnpm/releasing.versioning'

function makeProject (name: string, version: string, deps?: Record<string, string>): WorkspaceProject {
  return {
    rootDir: `/ws/${name}`,
    manifest: { name, version, dependencies: deps },
  }
}

function makeIntent (id: string, releases: ChangeIntent['releases'], summary = `summary of ${id}`): ChangeIntent {
  return { id, filePath: `/ws/.changeset/${id}.md`, releases, summary }
}

const NO_LEDGER: Ledger = {}

test('direct bumps: the highest pending bump type wins per package', () => {
  const plan = assembleReleasePlan({
    workspaceDir: '/ws',
    projects: [makeProject('a', '1.0.0'), makeProject('b', '2.3.4')],
    intents: [
      makeIntent('one', { a: 'patch', b: 'minor' }),
      makeIntent('two', { a: 'minor' }),
    ],
    ledger: NO_LEDGER,
  })
  expect(plan.releases).toHaveLength(2)
  expect(plan.releases.find((release) => release.name === 'a')).toMatchObject({ newVersion: '1.1.0', bumpType: 'minor' })
  expect(plan.releases.find((release) => release.name === 'b')).toMatchObject({ newVersion: '2.4.0', bumpType: 'minor' })
})

test('intents already recorded in the ledger are not consumed again', () => {
  const plan = assembleReleasePlan({
    workspaceDir: '/ws',
    projects: [makeProject('a', '1.0.1')],
    intents: [makeIntent('one', { a: 'patch' })],
    ledger: { 'a@1.0.1': ['one'] },
  })
  expect(plan.releases).toHaveLength(0)
})

test('a none bump type releases nothing', () => {
  const plan = assembleReleasePlan({
    workspaceDir: '/ws',
    projects: [makeProject('a', '1.0.0')],
    intents: [makeIntent('one', { a: 'none' }, 'refactor, no release needed')],
    ledger: NO_LEDGER,
  })
  expect(plan.releases).toHaveLength(0)
})

test('dependent propagation follows the materialized workspace range', () => {
  const plan = assembleReleasePlan({
    workspaceDir: '/ws',
    projects: [
      makeProject('lib', '1.2.0'),
      makeProject('cli', '3.0.0', { lib: 'workspace:^' }),
    ],
    intents: [makeIntent('one', { lib: 'major' })],
    ledger: NO_LEDGER,
  })
  const cli = plan.releases.find((release) => release.name === 'cli')
  expect(cli).toMatchObject({ newVersion: '3.0.1', bumpType: 'patch' })
  expect(cli!.dependencyUpdates).toStrictEqual([{ name: 'lib', newVersion: '2.0.0' }])
})

test('a minor bump does not propagate through workspace:^ on a 1.x dependency', () => {
  const plan = assembleReleasePlan({
    workspaceDir: '/ws',
    projects: [
      makeProject('lib', '1.2.0'),
      makeProject('cli', '3.0.0', { lib: 'workspace:^' }),
    ],
    intents: [makeIntent('one', { lib: 'minor' })],
    ledger: NO_LEDGER,
  })
  expect(plan.releases.map((release) => release.name)).toStrictEqual(['lib'])
})

test('a minor bump propagates through workspace:^ on a 0.x dependency', () => {
  const plan = assembleReleasePlan({
    workspaceDir: '/ws',
    projects: [
      makeProject('lib', '0.2.0'),
      makeProject('cli', '3.0.0', { lib: 'workspace:^' }),
    ],
    intents: [makeIntent('one', { lib: 'minor' })],
    ledger: NO_LEDGER,
  })
  expect(plan.releases.map((release) => release.name).sort()).toStrictEqual(['cli', 'lib'])
})

test('propagation cascades through chains of dependents', () => {
  const plan = assembleReleasePlan({
    workspaceDir: '/ws',
    projects: [
      makeProject('core', '1.0.0'),
      makeProject('mid', '1.0.0', { core: 'workspace:*' }),
      makeProject('top', '1.0.0', { mid: 'workspace:*' }),
    ],
    intents: [makeIntent('one', { core: 'patch' })],
    ledger: NO_LEDGER,
  })
  expect(plan.releases.map((release) => release.name).sort()).toStrictEqual(['core', 'mid', 'top'])
})

test('fixed groups release together at one shared version', () => {
  const plan = assembleReleasePlan({
    workspaceDir: '/ws',
    projects: [makeProject('a', '1.2.0'), makeProject('b', '1.0.5')],
    intents: [makeIntent('one', { a: 'minor' })],
    ledger: NO_LEDGER,
    versioning: { fixed: [['a', 'b']] },
  })
  expect(plan.releases.find((release) => release.name === 'a')!.newVersion).toBe('1.3.0')
  expect(plan.releases.find((release) => release.name === 'b')!.newVersion).toBe('1.3.0')
})

test('ignored packages neither release nor propagate', () => {
  const plan = assembleReleasePlan({
    workspaceDir: '/ws',
    projects: [
      makeProject('lib', '1.0.0'),
      makeProject('frozen', '1.0.0', { lib: 'workspace:*' }),
    ],
    intents: [makeIntent('one', { lib: 'major' })],
    ledger: NO_LEDGER,
    versioning: { ignore: ['frozen'] },
  })
  expect(plan.releases.map((release) => release.name)).toStrictEqual(['lib'])
})

test('an internal dependency without the workspace protocol fails a release, but not a read-only assemble', () => {
  const projects = [
    makeProject('lib', '1.0.0'),
    makeProject('cli', '1.0.0', { lib: '^1.0.0' }),
  ]
  // enforceWorkspaceProtocol off (the default, used by `pnpm change status`):
  // a read-only assemble never fails on an unmigrated dependency.
  expect(assembleReleasePlan({ workspaceDir: '/ws', projects, intents: [], ledger: NO_LEDGER }).releases).toHaveLength(0)
  // The release path enforces the prerequisite.
  expect(() => assembleReleasePlan({
    workspaceDir: '/ws',
    projects,
    intents: [],
    ledger: NO_LEDGER,
    enforceWorkspaceProtocol: true,
  })).toThrow(/workspace: protocol/)
})

test('an npm: alias colliding with a workspace package name is not an internal dependency', () => {
  const plan = assembleReleasePlan({
    workspaceDir: '/ws',
    projects: [
      makeProject('lib', '1.0.0'),
      makeProject('cli', '1.0.0', { lib: 'npm:some-fork@^1.0.0' }),
    ],
    intents: [makeIntent('one', { lib: 'major' })],
    ledger: NO_LEDGER,
  })
  expect(plan.releases.map((release) => release.name)).toStrictEqual(['lib'])
})

test('an intent demanding a release of an unreleasable package fails the plan', () => {
  expect(() => assembleReleasePlan({
    workspaceDir: '/ws',
    projects: [makeProject('lib', '1.0.0'), makeProject('frozen', '1.0.0')],
    intents: [makeIntent('one', { frozen: 'patch', lib: 'patch' })],
    ledger: NO_LEDGER,
    versioning: { ignore: ['frozen'] },
  })).toThrow(/cannot release/)
})

test('a none decline for an unreleasable package is accepted', () => {
  const plan = assembleReleasePlan({
    workspaceDir: '/ws',
    projects: [makeProject('lib', '1.0.0'), makeProject('frozen', '1.0.0')],
    intents: [makeIntent('one', { frozen: 'none', lib: 'patch' })],
    ledger: NO_LEDGER,
    versioning: { ignore: ['frozen'] },
  })
  expect(plan.releases.map((release) => release.name)).toStrictEqual(['lib'])
})

test('maxBump measures the real version distance, including fixed-group jumps', () => {
  expect(() => assembleReleasePlan({
    workspaceDir: '/ws',
    projects: [makeProject('a', '1.0.5'), makeProject('b', '2.0.0')],
    intents: [makeIntent('one', { a: 'minor' })],
    ledger: NO_LEDGER,
    versioning: { fixed: [['a', 'b']], maxBump: 'minor' },
  })).toThrow(/maxBump/)
})

test('an intent naming an unknown package fails the plan', () => {
  expect(() => assembleReleasePlan({
    workspaceDir: '/ws',
    projects: [makeProject('lib', '1.0.0')],
    intents: [makeIntent('one', { ghost: 'patch' })],
    ledger: NO_LEDGER,
  })).toThrow(/not a package in this workspace/)
})

test('maxBump rejects a plan whose effective bump exceeds the cap', () => {
  expect(() => assembleReleasePlan({
    workspaceDir: '/ws',
    projects: [makeProject('lib', '1.0.0')],
    intents: [makeIntent('one', { lib: 'minor' })],
    ledger: NO_LEDGER,
    versioning: { maxBump: 'patch' },
  })).toThrow(/maxBump/)
})

test('two same-named projects releasing to the same version is a hard error', () => {
  const twins = [
    { rootDir: '/ws/a/util', manifest: { name: '@scope/util', version: '1.0.0' } },
    { rootDir: '/ws/b/util', manifest: { name: '@scope/util', version: '1.0.0' } },
  ]
  expect(() => assembleReleasePlan({
    workspaceDir: '/ws',
    projects: twins,
    intents: [makeIntent('one', { './a/util': 'patch', './b/util': 'patch' })],
    ledger: NO_LEDGER,
  })).toThrow(/Two projects both release @scope\/util@1\.0\.1/)
})

test('a package on a lane emits tagged versions with an incrementing counter', () => {
  const enterPlan = assembleReleasePlan({
    workspaceDir: '/ws',
    projects: [makeProject('cli', '2.0.0')],
    intents: [makeIntent('one', { cli: 'minor' })],
    ledger: NO_LEDGER,
    versioning: { lanes: { cli: 'alpha' } },
  })
  expect(enterPlan.releases[0].newVersion).toBe('2.1.0-alpha.0')

  const nextPlan = assembleReleasePlan({
    workspaceDir: '/ws',
    projects: [makeProject('cli', '2.1.0-alpha.0')],
    intents: [
      makeIntent('one', { cli: 'minor' }),
      makeIntent('two', { cli: 'patch' }),
    ],
    ledger: { 'cli@2.1.0-alpha.0': ['one'] },
    versioning: { lanes: { cli: 'alpha' } },
  })
  expect(nextPlan.releases[0].newVersion).toBe('2.1.0-alpha.1')
})

test('a bigger bump landing later escalates the stable target of the lane', () => {
  const plan = assembleReleasePlan({
    workspaceDir: '/ws',
    projects: [makeProject('cli', '2.1.0-alpha.1')],
    intents: [
      makeIntent('one', { cli: 'minor' }),
      makeIntent('two', { cli: 'major' }),
    ],
    ledger: { 'cli@2.1.0-alpha.0': ['one'], 'cli@2.1.0-alpha.1': [] },
    versioning: { lanes: { cli: 'alpha' } },
  })
  expect(plan.releases[0].newVersion).toBe('3.0.0-alpha.0')
})

test('packages on the main lane release stable versions from the same run', () => {
  const plan = assembleReleasePlan({
    workspaceDir: '/ws',
    projects: [makeProject('cli', '2.0.0'), makeProject('lib', '1.0.0')],
    intents: [makeIntent('one', { cli: 'minor', lib: 'minor' })],
    ledger: NO_LEDGER,
    versioning: { lanes: { cli: 'alpha' } },
  })
  expect(plan.releases.find((release) => release.name === 'cli')!.newVersion).toBe('2.1.0-alpha.0')
  expect(plan.releases.find((release) => release.name === 'lib')!.newVersion).toBe('1.1.0')
})

test('returning to the main lane releases the accumulated stable version even without pending intents', () => {
  const plan = assembleReleasePlan({
    workspaceDir: '/ws',
    projects: [makeProject('cli', '2.1.0-alpha.2')],
    intents: [
      makeIntent('one', { cli: 'minor' }),
      makeIntent('two', { cli: 'patch' }),
    ],
    ledger: { 'cli@2.1.0-alpha.0': ['one'], 'cli@2.1.0-alpha.2': ['two'] },
    versioning: {},
  })
  expect(plan.releases).toHaveLength(1)
  const release = plan.releases[0]
  expect(release.newVersion).toBe('2.1.0')
  expect(release.intents.map((intent) => intent.id).sort()).toStrictEqual(['one', 'two'])
})

test('an intent naming a main-lane and a lane package is consumed half by half', () => {
  const plan = assembleReleasePlan({
    workspaceDir: '/ws',
    projects: [makeProject('cli', '2.0.0'), makeProject('lib', '1.0.1')],
    intents: [makeIntent('one', { cli: 'minor', lib: 'patch' })],
    ledger: { 'lib@1.0.1': ['one'] },
    versioning: { lanes: { cli: 'alpha' } },
  })
  expect(plan.releases.map((release) => release.name)).toStrictEqual(['cli'])
  expect(plan.releases[0].newVersion).toBe('2.1.0-alpha.0')
})

test('snapshot plans release the same set under snapshot versions', () => {
  const plan = assembleReleasePlan({
    workspaceDir: '/ws',
    projects: [
      makeProject('lib', '1.0.0'),
      makeProject('cli', '1.0.0', { lib: 'workspace:*' }),
    ],
    intents: [makeIntent('one', { lib: 'patch' })],
    ledger: NO_LEDGER,
    snapshotSuffix: 'preview-20260712000000',
  })
  expect(plan.releases.map((release) => release.newVersion)).toStrictEqual([
    '0.0.0-preview-20260712000000',
    '0.0.0-preview-20260712000000',
  ])
})

test('filter narrows the plan to the selection plus its fixed companions and invalidated dependents', () => {
  const plan = assembleReleasePlan({
    workspaceDir: '/ws',
    projects: [
      makeProject('lib', '1.0.0'),
      makeProject('cli', '1.0.0', { lib: 'workspace:*' }),
      makeProject('unrelated', '1.0.0'),
    ],
    intents: [
      makeIntent('one', { lib: 'patch' }),
      makeIntent('two', { unrelated: 'major' }),
    ],
    ledger: NO_LEDGER,
    filter: new Set(['lib']),
  })
  expect(plan.releases.map((release) => release.name).sort()).toStrictEqual(['cli', 'lib'])
})

test('a name shared by two projects is ambiguous and must be referenced by directory', () => {
  const twins = [
    { rootDir: '/ws/pnpm11/pnpm', manifest: { name: 'pnpm', version: '11.0.0' } },
    { rootDir: '/ws/pnpm/npm/pnpm', manifest: { name: 'pnpm', version: '12.0.0' } },
  ]
  expect(() => assembleReleasePlan({
    workspaceDir: '/ws',
    projects: twins,
    intents: [makeIntent('one', { pnpm: 'patch' })],
    ledger: NO_LEDGER,
  })).toThrow(/matches multiple workspace projects/)

  const plan = assembleReleasePlan({
    workspaceDir: '/ws',
    projects: twins,
    intents: [makeIntent('one', { './pnpm/npm/pnpm': 'patch' })],
    ledger: NO_LEDGER,
  })
  expect(plan.releases).toHaveLength(1)
  expect(plan.releases[0].dir).toBe('pnpm/npm/pnpm')
  expect(plan.releases[0].newVersion).toBe('12.0.1')
})

test('ledger consumption attributes by directory when names collide', () => {
  const twins = [
    { rootDir: '/ws/pnpm11/pnpm', manifest: { name: 'pnpm', version: '11.0.0' } },
    { rootDir: '/ws/pnpm/npm/pnpm', manifest: { name: 'pnpm', version: '12.0.0' } },
  ]
  const plan = assembleReleasePlan({
    workspaceDir: '/ws',
    projects: twins,
    intents: [makeIntent('one', { './pnpm11/pnpm': 'patch', './pnpm/npm/pnpm': 'patch' })],
    ledger: { 'pnpm@12.0.1': { dir: 'pnpm/npm/pnpm', intents: ['one'] } },
  })
  // The Rust line already consumed the intent; only the TS line still releases.
  expect(plan.releases.map((release) => release.dir)).toStrictEqual(['pnpm11/pnpm'])
})

test('lanes keyed by directory path apply to the right twin', () => {
  const twins = [
    { rootDir: '/ws/pnpm11/pnpm', manifest: { name: 'pnpm', version: '11.0.0' } },
    { rootDir: '/ws/pnpm/npm/pnpm', manifest: { name: 'pnpm', version: '12.0.0' } },
  ]
  const plan = assembleReleasePlan({
    workspaceDir: '/ws',
    projects: twins,
    intents: [makeIntent('one', { './pnpm11/pnpm': 'patch', './pnpm/npm/pnpm': 'minor' })],
    ledger: NO_LEDGER,
    versioning: { lanes: { './pnpm/npm/pnpm': 'alpha' } },
  })
  expect(plan.releases.find((release) => release.dir === 'pnpm11/pnpm')!.newVersion).toBe('11.0.1')
  expect(plan.releases.find((release) => release.dir === 'pnpm/npm/pnpm')!.newVersion).toBe('12.1.0-alpha.0')
})

test('a lane named main is rejected: it is the reserved default lane', () => {
  expect(() => assembleReleasePlan({
    workspaceDir: '/ws',
    projects: [makeProject('cli', '2.0.0')],
    intents: [],
    ledger: NO_LEDGER,
    versioning: { lanes: { cli: 'Main' } },
  })).toThrow(/reserved default lane/)
})

test('materializeWorkspaceRange mirrors pack-time materialization', () => {
  expect(materializeWorkspaceRange('workspace:*', '1.2.3')).toBe('1.2.3')
  expect(materializeWorkspaceRange('workspace:^', '1.2.3')).toBe('^1.2.3')
  expect(materializeWorkspaceRange('workspace:~', '1.2.3')).toBe('~1.2.3')
  expect(materializeWorkspaceRange('workspace:^1.0.0', '1.2.3')).toBe('^1.0.0')
  expect(materializeWorkspaceRange('workspace:lib@^', '1.2.3')).toBe('^1.2.3')
  expect(materializeWorkspaceRange('^1.0.0', '1.2.3')).toBeNull()
})

test('epic members move independently inside the band while the lead major holds', () => {
  const plan = assembleReleasePlan({
    workspaceDir: '/ws',
    projects: [makeProject('pnpm', '11.2.0'), makeProject('lib', '1101.4.2')],
    intents: [makeIntent('one', { pnpm: 'patch', lib: 'minor' })],
    ledger: NO_LEDGER,
    versioning: { epics: [{ lead: 'pnpm', packages: ['lib'] }] },
  })
  expect(plan.releases.find((release) => release.name === 'pnpm')!.newVersion).toBe('11.2.1')
  expect(plan.releases.find((release) => release.name === 'lib')!.newVersion).toBe('1101.5.0')
})

test('a major intent bumps a member to the next major inside the band', () => {
  const plan = assembleReleasePlan({
    workspaceDir: '/ws',
    projects: [makeProject('pnpm', '11.0.0'), makeProject('lib', '1101.4.2')],
    intents: [makeIntent('one', { lib: 'major' })],
    ledger: NO_LEDGER,
    versioning: { epics: [{ lead: 'pnpm', packages: ['lib'] }] },
  })
  expect(plan.releases.map((release) => release.name)).toStrictEqual(['lib'])
  expect(plan.releases[0].newVersion).toBe('1102.0.0')
})

test('when the lead reaches a new stable major every member re-bases to the band floor', () => {
  const plan = assembleReleasePlan({
    workspaceDir: '/ws',
    projects: [
      makeProject('pnpm', '11.9.9'),
      makeProject('lib', '1101.4.2'),
      makeProject('ui', '1105.0.0'),
    ],
    intents: [makeIntent('one', { pnpm: 'major' })],
    ledger: NO_LEDGER,
    versioning: { epics: [{ lead: 'pnpm', packages: ['lib', 'ui'] }] },
  })
  expect(plan.releases.find((release) => release.name === 'pnpm')!.newVersion).toBe('12.0.0')
  const lib = plan.releases.find((release) => release.name === 'lib')!
  expect(lib.newVersion).toBe('1200.0.0')
  expect(lib.causes).toStrictEqual(['epic'])
  expect(plan.releases.find((release) => release.name === 'ui')!.newVersion).toBe('1200.0.0')
})

test('a member on a lane re-bases to a prerelease of the band floor', () => {
  const plan = assembleReleasePlan({
    workspaceDir: '/ws',
    projects: [makeProject('pnpm', '11.0.0'), makeProject('lib', '1101.2.0')],
    intents: [makeIntent('one', { pnpm: 'major' })],
    ledger: NO_LEDGER,
    versioning: { epics: [{ lead: 'pnpm', packages: ['lib'] }], lanes: { lib: 'alpha' } },
  })
  expect(plan.releases.find((release) => release.name === 'lib')!.newVersion).toBe('1200.0.0-alpha.0')
})

test('the re-base waits while the lead is on a prerelease lane', () => {
  const plan = assembleReleasePlan({
    workspaceDir: '/ws',
    projects: [makeProject('pnpm', '11.0.0'), makeProject('lib', '1101.2.0')],
    intents: [makeIntent('one', { pnpm: 'major', lib: 'patch' })],
    ledger: NO_LEDGER,
    versioning: { epics: [{ lead: 'pnpm', packages: ['lib'] }], lanes: { pnpm: 'alpha' } },
  })
  expect(plan.releases.find((release) => release.name === 'pnpm')!.newVersion).toBe('12.0.0-alpha.0')
  // The member versions inside its old band until the lead's stable release.
  expect(plan.releases.find((release) => release.name === 'lib')!.newVersion).toBe('1101.2.1')
})

test('epic membership resolves directory globs and honors negations', () => {
  const projects = [
    { rootDir: '/ws/pnpm', manifest: { name: 'pnpm', version: '11.0.0' } },
    { rootDir: '/ws/pkgs/a', manifest: { name: '@scope/a', version: '1100.0.0' } },
    { rootDir: '/ws/pkgs/b', manifest: { name: '@scope/b', version: '1100.0.0' } },
    { rootDir: '/ws/tools/tool', manifest: { name: '@scope/tool', version: '5.0.0' } },
  ]
  const plan = assembleReleasePlan({
    workspaceDir: '/ws',
    projects,
    intents: [makeIntent('one', { pnpm: 'major' })],
    ledger: NO_LEDGER,
    versioning: { epics: [{ lead: './pnpm', packages: ['./pkgs/**', '!./pkgs/b'] }] },
  })
  expect(plan.releases.map((release) => release.name).sort()).toStrictEqual(['@scope/a', 'pnpm'])
  expect(plan.releases.find((release) => release.name === '@scope/a')!.newVersion).toBe('1200.0.0')
})

test('epic selectors are order-dependent: a later include overrides an earlier negation', () => {
  const projects = [
    { rootDir: '/ws/pnpm', manifest: { name: 'pnpm', version: '11.0.0' } },
    { rootDir: '/ws/pkgs/a', manifest: { name: '@scope/a', version: '1100.0.0' } },
    { rootDir: '/ws/pkgs/b', manifest: { name: '@scope/b', version: '1100.0.0' } },
  ]
  const plan = assembleReleasePlan({
    workspaceDir: '/ws',
    projects,
    intents: [makeIntent('one', { pnpm: 'major' })],
    ledger: NO_LEDGER,
    // '!./pkgs/b' first, then './pkgs/**' — the later include wins, so b is a member.
    versioning: { epics: [{ lead: './pnpm', packages: ['!./pkgs/b', './pkgs/**'] }] },
  })
  expect(plan.releases.map((release) => release.name).sort()).toStrictEqual(['@scope/a', '@scope/b', 'pnpm'])
})

test('a package matched by two epics is a configuration error', () => {
  expect(() => assembleReleasePlan({
    workspaceDir: '/ws',
    projects: [makeProject('pnpm', '11.0.0'), makeProject('other', '2.0.0'), makeProject('lib', '1101.0.0')],
    intents: [],
    ledger: NO_LEDGER,
    versioning: {
      epics: [
        { lead: 'pnpm', packages: ['lib'] },
        { lead: 'other', packages: ['lib'] },
      ],
    },
  })).toThrow(/at most one epic/)
})

test('a fixed group straddling an epic boundary is a configuration error', () => {
  expect(() => assembleReleasePlan({
    workspaceDir: '/ws',
    projects: [makeProject('pnpm', '11.0.0'), makeProject('lib', '1101.0.0'), makeProject('outsider', '3.0.0')],
    intents: [],
    ledger: NO_LEDGER,
    versioning: {
      epics: [{ lead: 'pnpm', packages: ['lib'] }],
      fixed: [['lib', 'outsider']],
    },
  })).toThrow(/straddles the epic/)
})

test('a member major bump that would exceed the band ceiling is rejected', () => {
  expect(() => assembleReleasePlan({
    workspaceDir: '/ws',
    projects: [makeProject('pnpm', '11.0.0'), makeProject('lib', '1199.4.2')],
    intents: [makeIntent('one', { lib: 'major' })],
    ledger: NO_LEDGER,
    versioning: { epics: [{ lead: 'pnpm', packages: ['lib'] }] },
  })).toThrow(/band is exhausted/)
})

test('a member below its epic band is rejected when it releases', () => {
  expect(() => assembleReleasePlan({
    workspaceDir: '/ws',
    projects: [makeProject('pnpm', '11.0.0'), makeProject('lib', '5.0.0')],
    intents: [makeIntent('one', { lib: 'patch' })],
    ledger: NO_LEDGER,
    versioning: { epics: [{ lead: 'pnpm', packages: ['lib'] }] },
  })).toThrow(/outside the band 1100-1199/)
})

test('the top of the band takes a minor without tripping the ceiling guard', () => {
  const plan = assembleReleasePlan({
    workspaceDir: '/ws',
    projects: [makeProject('pnpm', '11.0.0'), makeProject('lib', '1199.4.2')],
    intents: [makeIntent('one', { lib: 'minor' })],
    ledger: NO_LEDGER,
    versioning: { epics: [{ lead: 'pnpm', packages: ['lib'] }] },
  })
  expect(plan.releases.find((release) => release.name === 'lib')!.newVersion).toBe('1199.5.0')
})

test('an epic whose lead is not a releasable project fails the plan', () => {
  expect(() => assembleReleasePlan({
    workspaceDir: '/ws',
    projects: [makeProject('lib', '1101.0.0')],
    intents: [],
    ledger: NO_LEDGER,
    versioning: { epics: [{ lead: 'ghost', packages: ['lib'] }] },
  })).toThrow(/is not a releasable workspace project/)
})

test('checkVersioningInvariants: a workspace within its bands and in lockstep has no violations', () => {
  const violations = checkVersioningInvariants({
    workspaceDir: '/ws',
    projects: [
      makeProject('pnpm', '11.15.1'),
      makeProject('@pnpm/lib', '1102.0.7'),
      makeProject('@pnpm/exe', '11.15.1'),
    ],
    versioning: {
      epics: [{ lead: 'pnpm', packages: ['@pnpm/lib'] }],
      fixed: [['pnpm', '@pnpm/exe']],
    },
  })
  expect(violations).toEqual([])
})

test('checkVersioningInvariants: reports an epic member whose committed major is outside the band', () => {
  const violations = checkVersioningInvariants({
    workspaceDir: '/ws',
    projects: [makeProject('pnpm', '11.15.1'), makeProject('@pnpm/lib', '5.0.0')],
    versioning: { epics: [{ lead: 'pnpm', packages: ['@pnpm/lib'] }] },
  })
  expect(violations).toHaveLength(1)
  expect(violations[0].code).toBe('VERSIONING_EPIC_OUT_OF_BAND')
  expect(violations[0].message).toContain('outside the band 1100-1199')
})

test('checkVersioningInvariants: reports a fixed group that fell out of lockstep', () => {
  const violations = checkVersioningInvariants({
    workspaceDir: '/ws',
    projects: [makeProject('pnpm', '11.15.1'), makeProject('@pnpm/exe', '11.15.0')],
    versioning: { fixed: [['pnpm', '@pnpm/exe']] },
  })
  expect(violations).toHaveLength(1)
  expect(violations[0].code).toBe('VERSIONING_FIXED_GROUP_MISMATCH')
  expect(violations[0].message).toContain('not in lockstep')
})

test('checkVersioningInvariants: reports every violation at once', () => {
  const violations = checkVersioningInvariants({
    workspaceDir: '/ws',
    projects: [
      makeProject('pnpm', '11.15.1'),
      makeProject('@pnpm/lib', '5.0.0'),
      makeProject('@pnpm/exe', '11.15.0'),
    ],
    versioning: {
      epics: [{ lead: 'pnpm', packages: ['@pnpm/lib'] }],
      fixed: [['pnpm', '@pnpm/exe']],
    },
  })
  expect(violations.map((violation) => violation.code).sort()).toEqual([
    'VERSIONING_EPIC_OUT_OF_BAND',
    'VERSIONING_FIXED_GROUP_MISMATCH',
  ])
})

test('checkVersioningInvariants: malformed configuration still throws', () => {
  expect(() => checkVersioningInvariants({
    workspaceDir: '/ws',
    projects: [makeProject('pnpm', '11.15.1')],
    versioning: { epics: [{ lead: 'ghost', packages: ['@pnpm/lib'] }] },
  })).toThrow(/is not a releasable workspace project/)
})
