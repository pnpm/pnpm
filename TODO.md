# TODO — `preferredManifestFormat`

Working notes for the stacked pair of PRs adding `preferredManifestFormat` to
`pnpm-workspace.yaml`. Everything below is open work on the feature branch
unless marked otherwise.

**Delete this file before either PR is marked ready for review.**

## Branch layout

| Branch | Contents |
| --- | --- |
| `fix/dedupe-coexisting-manifests` | Workspace-discovery fix only. Independently landable and backportable. |
| `feat/preferred-manifest-format` | The setting, stacked on the fix branch. |

GitHub cannot set a cross-fork PR's base to a branch in the fork, so both PRs
target `pnpm/pnpm:main`. The feature PR therefore shows the fix commit in its
diff until the fix lands; rebase and force-push once it does.

Both branches are based on pre-restructure `main` (`52ee08aad4`). Upstream has
since moved the TypeScript CLI to `pnpm11/` and added the Rust `pnpm/` and
`pnpr/` trees, so every path in these diffs has moved. A substantial rebase is
due regardless of anything below.

## 1. The write path does not honor the setting (blocker)

The PR description claimed install and run paths pick the setting up
automatically by spreading `...opts`. They do not. Those paths go through
`@pnpm/cli.utils`, whose wrappers accept `opts` and then drop it:

```ts
// cli/utils/src/readProjectManifest.ts:27, 36, 49
const { fileName, manifest, writeProjectManifest } = await utils.readProjectManifest(projectDir)
const manifest = await utils.readProjectManifestOnly(projectDir)
const { fileName, manifest, writeProjectManifest } = await utils.tryReadProjectManifest(projectDir)
```

`ReadProjectManifestOpts` in that file does not declare
`preferredManifestFormat` either.

Affected callers: `installing/commands/src/installDeps.ts:265`,
`remove.ts:206`, `update/index.ts:200`, `exec/commands/src/run.ts:117,221,224,258`,
`exec.ts`, `link.ts`, `deps/inspection/commands/src/outdated/outdated.ts`,
`global/commands/src/installGlobalPackages.ts`.

Failure: with `preferredManifestFormat: json5` and both files present,
`findPackages` hands install the json5 manifest, then `pnpm add foo` re-reads
the directory through `cli.utils` and writes the new dependency into
`package.json`. Read and write disagree within a single command.

- [ ] Add `preferredManifestFormat` to `ReadProjectManifestOpts`.
- [ ] Forward `opts` in all three `cli/utils` wrappers.
- [ ] `installing/commands/src/createProjectManifestWriter.ts:9` calls the
      reader with no opts and hardcodes `package.json` in its ENOENT fallback.

## 2. The no-manifest fallback filename changed, undocumented

`tryReadProjectManifest` used to return `fileName: 'package.json'` for a
directory with no manifest, with a writer targeting it. It now returns
`FORMAT_FILENAMES[order[0]]`, so `preferredManifestFormat: yaml` makes a fresh
directory get a `package.yaml` created.

Currently unreachable because of #1; it goes live the moment #1 is fixed.

- [ ] Decide deliberately whether creating a new manifest should follow the
      preference, then document and test whichever way it goes.

## 3. Invalid values fail silently

`buildFormatOrder` and `pickManifestPerDirectory` both guard with
`DEFAULT_FORMAT_ORDER.includes(preferred)` and quietly fall back, so
`preferredManifestFormat: jsonc` does nothing with no message. pnpm generally
errors on bad config.

- [ ] Validate in `validateWorkspaceManifest`
      (`workspace/workspace-manifest-reader/src/index.ts`) alongside the
      existing `assertValidWorkspaceManifest*` checks, throwing a `PnpmError`.

## 4. The shadowing warning is inconsistent

`findPackages` warns when the preferred format is missing and other manifests
coexist. `tryReadProjectManifest` — which reads the root manifest and every
single-project path — never does, so the root silently resolves to the wrong
file while workspace packages warn.

- [ ] Either lift the warning into the reader, or document why discovery is
      the only place it fires.

## 5. Test gaps

- [ ] Nothing asserts the `logger.warn` fires, or stays quiet in the
      partial-migration case. That is the subtlest logic added.
- [ ] No test for the `config/reader` bootstrap reorder, i.e. that the root
      manifest actually honors the setting.
- [ ] e2e: `pnpm install` in a fixture with both `package.json` and
      `package.json5` and `preferredManifestFormat: json5`, verifying the
      right manifest is written back. This is exactly what would have caught #1.
- [ ] Full bundle build via `pnpm --filter pnpm run compile`.

## 6. Smaller items

- [ ] `config/reader/src/index.ts:439` — `workspaceDir: workspaceManifestDir!`.
      The assertion is safe, since `workspaceManifest` is only assigned in
      branches that also set the dir, but restructuring as a single
      `{ manifest, dir }` object would drop the `!`.
- [x] Dropped the `delete (globOpts as ...).preferredManifestFormat` line
      during the split. It was unnecessary: excess-property checks do not
      apply to a variable, tinyglobby ignores unknown options, and
      `includeRoot` was already spread through without deletion.
- [ ] Docs: the setting needs an entry on the pnpm-workspace.yaml settings
      page in the docs repo.

## Verified locally

Tests run under Node 23 via fnm: the repo's jest transform uses
`stripTypeScriptTypes` with `mode: 'transform'`, which the system Node 26 no
longer supports. CI uses Node 24.

- `@pnpm/workspace.projects-reader` — 11/11 on the fix branch, 13/13 on the
  feature branch. Compile and lint clean on both.
- `@pnpm/workspace.project-manifest-reader` — 23/23. Compile and lint clean.
- `tsgo --build` clean across `core/types`, `config/reader`,
  `workspace/project-manifest-reader`, `workspace/projects-reader`,
  `workspace/projects-filter`, `deps/status`.
- The duplicate-project bug was reproduced against the pre-fix implementation:
  the `conflicting-manifests` fixture yielded 3 projects, two sharing a
  `rootDir` (one from `package.json`, one from `package.json5`).
- Fixed a `no-await-in-loop` lint error the feature commit introduced in
  `tryReadProjectManifest`. It would have failed CI.

## Verified during review, no action needed

- The `config/reader` reorder is behavior-preserving. Nothing between the two
  blocks reads `workspacePackagePatterns`, and the global branch still does
  not set it.
- `readWorkspaceManifest` does not reject unknown keys, and
  `WorkspaceManifest extends PnpmSettings`, so the setting flows through.
- `Config extends OptionsFromRootManifest`, so adding the key to the `Pick<>`
  in `getOptionsFromRootManifest.ts` does put it on `Config`.
