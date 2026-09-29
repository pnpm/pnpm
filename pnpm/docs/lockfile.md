---
id: lockfile
title: Reading `pnpm-lock.yaml`
---

`pnpm-lock.yaml` is a YAML file, but it is not always a **single** YAML
document. Depending on what the project uses, pnpm writes either one document
or two:

| Document | Contents |
| --- | --- |
| The **env lockfile** (first, when present) | [Config dependencies] and the pnpm version resolved for the project, under `configDependencies` and `packageManagerDependencies`, with the `packages` and `snapshots` entries they need |
| The **project lockfile** (last, always present) | The project's own dependency graph: `importers` with their `dependencies`, and the matching `packages` and `snapshots` |

Both documents declare the same `lockfileVersion`. That field describes the
schema of the entries inside a document, not how many documents the file
contains, so it does not change when the env document appears.

This page is for tools that read `pnpm-lock.yaml` — vulnerability scanners,
SBOM generators, dependency graph builders, and dependency update bots. If you
only use pnpm, you don't need to care about any of this.

A two-document lockfile looks like this:

```yaml title="pnpm-lock.yaml"
---
lockfileVersion: '9.0'

importers:

  .:
    configDependencies: {}
    packageManagerDependencies:
      pnpm:
        specifier: 12.3.4
        version: 12.3.4

packages:
  # pnpm and its platform binaries
snapshots:
  # ...
---
lockfileVersion: '9.0'

settings:
  autoInstallPeers: true

importers:

  .:
    dependencies:
      react:
        specifier: ^19.0.0
        version: 19.0.0

packages:
  # every package in the project's dependency graph
snapshots:
  # ...
```

## Which document you need

That depends on what your tool is for:

- **The project's dependency graph** — dependency update bots, graph builders,
  anything that asks "what does this project depend on": the **last** document.
- **Everything the lockfile installs** — vulnerability scanners and SBOM
  generators: **every** document. Config dependencies are real npm packages,
  installed into `node_modules/.pnpm-config`, and they appear only in the env
  document.

Either way, load the file with a multi-document API. A single-document one
(`load()` in js-yaml, `yaml.safe_load` in Python, `yaml.Unmarshal` into one
value in Go) either raises an error or silently gives you the first document —
which, in a two-document lockfile, is the env document.

### The project's dependency graph

Take the last document:

```js
import { readFile } from 'node:fs/promises'
import { loadAll } from 'js-yaml'

const lockfile = loadAll(await readFile('pnpm-lock.yaml', 'utf8')).at(-1)
```

This is correct for both shapes, and for every `pnpm-lock.yaml` pnpm has ever
written.

If you would rather detect the layout than always take the last document, a
file whose first line is `---` has an env document. pnpm writes that leading
marker only when it writes two documents.

### Everything the lockfile installs

Read every document and union what you find in each:

```js
const documents = loadAll(await readFile('pnpm-lock.yaml', 'utf8'))
const installed = documents.flatMap((doc) => Object.keys(doc.packages ?? {}))
```

Take each document as an inventory of its own rather than merging the documents
into a single object first. Both use the `.` importer key, and each carries its
own `packages` and `snapshots` maps, so merging them overwrites one side's
importer and loses whichever graph it held.

## Resolutions that carry a revision

Since v12.0.0, a `packages:` entry may carry a `revision` field
beside its integrity:

```yaml
packages:
  lodash@4.17.21:
    resolution:
      integrity: sha512-<replacement-digest>
      revision: 1
```

It says that the bytes are a registry **replacement artifact**, fetched from the
registry's integrity-addressed route rather than the canonical
`name@version` URL. An entry with no `revision` field is revision 0 — the
original — which is what every entry pnpm has ever written means, so a lockfile
that has adopted no replacements is unchanged. See [Registry
revisions](./registry-revisions.md).

A tool that reads integrity values should treat `revision` as ordinary metadata:
`integrity` is still a standard Subresource Integrity value pinning the exact
bytes.

## Why the env document comes first

pnpm has to know which pnpm version to switch to, and which plugins to load,
before it can do anything else — including before it has any reason to parse a
dependency graph that may be several megabytes. Keeping that information in a
small leading document means every command reads a few kilobytes rather than
the whole file.

## When a lockfile has two documents

The env document is written when either of these applies:

- The project has [config dependencies]. Their integrity checksums are project
  content and are always recorded.
- pnpm records the pnpm version it resolved for the project, under
  `packageManagerDependencies`. This happens when the project declares
  [`devEngines.packageManager`], or pins pnpm 12 or newer through the legacy
  `packageManager` field. Setting [`pmOnFail`] to `ignore` turns it off — pnpm
  then doesn't enforce the pinned version, so it has nothing to record.

Two-document lockfiles have existed since config dependencies were introduced.
They became common in pnpm 12, which records the resolved package manager
version by default.

Since v11.23.0, a frozen install no longer rewrites this block. When the pinned
pnpm version is missing from the lockfile or no longer matches it,
`pnpm install --frozen-lockfile` fails with
`ERR_PNPM_FROZEN_LOCKFILE_WITH_OUTDATED_LOCKFILE`, so a manifest whose pin was
bumped without regenerating the lockfile no longer passes CI. Run
`pnpm install` without `--frozen-lockfile` and commit the result.

## Scanning for vulnerabilities

:::caution

A tool that reads only the first document of a two-document lockfile does not
fail loudly. The env document is a structurally valid lockfile — it just
describes an importer with no `dependencies`. Such a tool reports that the
project has no dependencies, and therefore no vulnerabilities, and a CI gate
built on it passes.

:::

If you scan pnpm projects:

- **Read every document**, as described above. A tool that reads only the
  project document misses the config dependencies; a tool that reads only the
  env document misses everything else.
- **Verify your scanner against a two-document lockfile before you trust its
  output.** "It reported something" is not a sufficient check here: the env
  document does contain packages, so a broken reader emits a plausible-looking
  result listing pnpm's own binaries.
- [`pnpm audit`] reads the lockfile correctly in both shapes and needs no
  configuration.

[Config dependencies]: config-dependencies.md
[`devEngines.packageManager`]: package_json.md#devenginespackagemanager
[`pmOnFail`]: settings/cli.md#pmonfail
[`pnpm audit`]: cli/audit.md
