# Website documentation

`pnpm/docs/`, `pnpm11/docs/`, and `pnpr/docs/` are the sources for the v12, v11,
and registry documentation published on [pnpm.io](https://pnpm.io). Change documentation in
the same pull request as the behavior it describes. The website, blog, v10 and
archived documentation, translations, and deployment remain in
[pnpm/pnpm.io](https://github.com/pnpm/pnpm.io).

## Editing

The Markdown lives directly under `pnpm/docs/` for v12 and `pnpm11/docs/` for
v11, with registry documentation under `pnpr/docs/`. Each directory includes
its `sidebars.json` and documentation assets under `static/`. Update both CLI
trees when a fix affects both versions.

Use `/img/...` URLs for assets stored under a line's `static/img/` directory.
The publisher places them under `/docs-assets/<line>/img/` and rewrites the
published Markdown, so different releases cannot overwrite each other's images.
Images shared with the blog or website remain available at their original URLs.

With a sibling checkout of pnpm/pnpm.io containing the sync tooling:

```sh
pnpm --dir ../pnpm.io install --frozen-lockfile
node ../pnpm.io/scripts/sync-docs.mjs . --preview
pnpm --dir ../pnpm.io build
pnpm --dir ../pnpm.io check-urls
```

Adjust the paths to your checkouts. `--preview` replaces the website's generated
documentation copies with the local v11, v12, and pnpr sources. Run it again after
editing sources. It does not publish or advance release state.
The Documentation workflow performs the same build and URL checks for PRs.
It pins the website revision for reproducibility; update that revision when
adopting website tooling changes. Release publication uses the website's current
`main` branch.

## Publication

When a maintainer publishes a draft GitHub release, Sync documentation
dispatches itself from `main` for that tag, so every sync runs the workflow,
scripts, and trusted keys on `main`. The sync verifies the
signed tag and npm publication, then imports only that version's documentation
from the tagged commit. Stable v11 and v12 releases update their
respective CLI documentation. Pnpr releases, including alpha releases, update
only the registry documentation. Other CLI releases do not trigger a sync.

The sync builds and checks the site without the publication credential. A
separate job copies only documentation files into a clean checkout, then commits
the generated copies to
pnpm/pnpm.io. Its Deploy workflow publishes the website. `docs-sync.json` in
that repository records the release version and both source commits. Older
releases cannot replace newer copies, and rerunning a release cannot undo a
later documentation correction.

A sync failure does not undo package publication. Rerun Sync documentation
with `release_tag` to retry, without publishing packages again. Release retries
use the currently trusted keys in `.github/release-keys/`; removing a key also
disables retries for tags signed with it. Historical keys are not automatically
re-trusted. Use the manual publication from `main` described below if needed.
Concurrent website changes may cause the final push to fail; rerun the sync to
build against the new website commit. The workflow never force-pushes.

To publish directly from `main` without a release, open **Actions → Sync
documentation → Run workflow**, select `main`, and leave both inputs empty.
This builds, checks, and publishes every product's current documentation,
including docs for unreleased changes. It requires the same `DOCS_SYNC_TOKEN`
setup as automatic publication. The generated website commit records the source
SHA. This manual override leaves the last-release tracking in `docs-sync.json`
unchanged; subsequent newer releases resume updating their respective docs.

### Release pages

Each stable v11 and v12 sync also writes the release's page on the pnpm.io blog,
`blog/releases/<version>.md`. The page is the composed changelog section that
the release PR curated (see the
[release-notes skill](.agents/skills/release-notes/SKILL.md)), read at the
release commit. Its lead paragraph becomes the blog excerpt. The sync never
overwrites an existing page, so fix a published page in pnpm/pnpm.io directly.
Pnpr releases get no page.

### Corrections

For a correction that should ship before the next package release, branch from
the published release tag, change only the corresponding `pnpm/docs/` or
`pnpm11/docs/` directory, or `pnpr/docs/` for registry documentation. Make the
commit available in pnpm/pnpm. Dispatch the sync with that `release_tag` and the
full `docs_commit` SHA. Also carry the correction into the development branch.
The workflow rejects corrections containing code changes or changes to another
version's documentation. This keeps unreleased features off the public site.

## Markdown by renderer

Each file kind is rendered by a different tool. Use only what its renderer keeps.

| Files | Renderer | Admonitions | Links |
|---|---|---|---|
| `pnpm/docs/`, `pnpm11/docs/`, `pnpr/docs/` | Docusaurus on pnpm.io | `:::note`, `:::tip`, `:::warning`, and the other Docusaurus forms | relative `.md` paths between pages; site-rooted `/img/...` and `/blog/...` paths, which the publisher rewrites |
| `.changeset/*.md` | GitHub release notes and the release blog | none | absolute URLs |
| `AGENTS.md`, `CONTRIBUTING.md`, and the other in-repo guides | GitHub, editors, agent tooling | GitHub alerts (`> [!NOTE]`); `WARNING` and `CAUTION` only for a hazard | relative paths |
| READMEs of published npm packages | npm | none | absolute URLs |
| Rust doc comments | rustdoc | none | intra-doc links |
| Doc comments on clap commands and arguments | plain text in `--help` | none | none |

GitHub alerts render only on GitHub. Docusaurus admonitions render only on pnpm.io.

## Reference pages

These tests fail when the v12 reference drifts from the code. Each keeps a
list of today's gaps, and fixing a gap means deleting its entry:

- `pnpm/crates/config/src/known_settings/tests/docs_reference.rs`: every
  setting pnpm reads has a section under `settings/` and an entry in the
  `settings.md` index, and no section names a setting pnpm does not read.
- `pnpm/crates/cli/src/cli_args/tests/docs_command_pages.rs`: every flag a
  command accepts has a heading on its page under `cli/`, and no page
  documents a flag the command rejects.
- `pnpm/crates/cli/src/tests/docs_error_codes.rs`: every `ERR_PNPM_*` code the
  docs name exists in the source, and `errors.md` lists its codes in
  alphabetical order.

The templates below are the shape those tests parse. A new page follows
them. An existing page moves toward them when it is edited.

### Command page (`cli/<command>.md`)

The page of `pnpm <command>` is `cli/<command>.md`. A subcommand's page is
`cli/<command>-<subcommand>.md`, or a `### <subcommand>` section of the
command's page.

````markdown
---
id: <command>
title: pnpm <command>
---

<One sentence: what the command does to what. A second sentence for the one
default or term the reader needs.>

Aliases: `<alias>`

## Synopsis

```sh
pnpm <command> [<arg>...] [--<flag>]
```

## Description

<What the command reads, what it writes, and what it does not do. One bullet
per argument shape when there are several. One sentence linking the page that
owns the concept.>

## Options

### --long-name, -s

<What the flag does, as one sentence. Its default, when it has one. Each
interaction as its own sentence: "Cannot be combined with `--other`." The
setting it mirrors: "Same as the [`settingName`](../settings/<group>.md#settingname) setting.">

## Exit status

- `0`: <success condition>.
- non-zero: <failure condition>, reported as `ERR_PNPM_<CODE>`.

## Examples

To <reader's goal>:

```sh
pnpm <command> <args>
```

## See also

[`pnpm <related>`](./<related>.md), [<guide>](../<guide>.md)
````

Rules the tests rely on:

- A flag's heading starts with the flag: `### --save-dev, -D`. A value goes
  after the flag: `### --registry &lt;url\>`. `### --[no-]bail` documents
  both spellings, and so does a heading for either one of them.
- A flag shared by several commands lives in a partial under `settings/`,
  such as `_cpuFlag.mdx`, and each page imports it. The partial's headings
  count as the page's own.
- A command that accepts another command's flags says so on one line that
  says it accepts, takes, or applies the options or arguments of the other
  command, and links its page: "`pnpm ci` accepts the options of
  [`pnpm install`](./install.md)." The page then documents those flags, so
  the sentence is for a command that takes the whole option set.
- A command documented on another command's page, such as `pnpm rb` on
  `cli/rebuild.md`, is listed in the test's `PAGE_NAMES`.
- A flag that spells a setting, such as `--shamefully-hoist`, is accepted by
  every command. Document it on the pages of the commands it changes.
- Global flags are documented once, on `pnpm-cli.md`, `cli/recursive.md`,
  `filtering.md`, or the section of the setting they mirror, not on each
  command page.
- The one-line description matches the first line of `pnpm <command> --help`.
- A version marker ("Added in: v12.N.0") sits at the end of the entry it
  qualifies. It is removed once the release is older than the previous minor
  line.

### Setting entry (`settings/<group>.md`)

````markdown
### settingName

* Default: **<value>**
* Type: **<type or the enumerated values>**
* Environment variable: `PNPM_CONFIG_SETTING_NAME`
* CLI: `--setting-name`
* Set in: `pnpm-workspace.yaml`, global `config.yaml`

<One sentence: what the setting controls.>

* **a**: <what pnpm does>.
* **b**: <what pnpm does>. Choose this when <condition>.

<The cost of a non-default value, as its own sentence, with the symptom.>

```yaml title="pnpm-workspace.yaml"
settingName: b
```

Related: [`otherSetting`](#othersetting).
````

Rules the tests rely on:

- The heading is the setting name as written in `pnpm-workspace.yaml`. A
  nested setting keeps its dots: `### update.changeset`.
- Every section is linked from the `## Settings` index in `settings.md`, in
  its group.
- An alias is named only in the section of the setting it aliases.

### Error entry (`errors.md`)

Each entry is `## ERR_PNPM_<CODE>`, in alphabetical order. It states what
happened, then the fix, naming who acts. A page that states a rule names the
code the rule raises, so a reader who searches for the code finds the rule.

## Initial rollout

1. Land the pnpm/pnpm.io tooling, builder patch and generated copies first.
   Its v11, v12, and pnpr edit links will point at the corresponding product docs in
   pnpm/pnpm.
2. Land the product docs and the Documentation and Sync documentation
   workflows in pnpm/pnpm. The sync starts with releases whose tags include the
   documentation sources; older tags cannot provide them.
3. Create a `docs-sync` environment in pnpm/pnpm with `DOCS_SYNC_TOKEN`, a token
   with permission to push content to pnpm/pnpm.io's `main` branch. Configure
   branch rules to allow that identity. Use a GitHub App token or fine-grained
   personal access token; the source repository's `GITHUB_TOKEN` cannot perform
   this cross-repository push and trigger the website deployment. Limit the
   environment's deployment branches to `main`, so a workflow on another branch
   or a tag cannot read the token.
4. Keep Crowdin uploads in pnpm/pnpm.io. Its existing paths and translation
   identifiers remain unchanged. Run `pnpm crowdin-upload` there after a sync
   when updating translation sources, as before.

Version names and publication paths are managed by pnpm/pnpm.io. Its
`scripts/docs-sources.mjs` maps `11.x` to `pnpm11/docs`, `12.x` to `pnpm/docs`,
and `pnpr` to `pnpr/docs`.
The sources here have no additional version directories. A future v10 migration
can be made separately on `release/10` if needed.
