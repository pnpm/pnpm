# Security Policy

## Supported Versions

| Version | Supported          |
| ------- | ------------------ |
| 11.x  | :white_check_mark: till 2027 April 30 |
| 10.x  | :white_check_mark: till 2027 April 30 |
| <= 9.x   | :x:                |

## Reporting a Vulnerability

Submit your findings here: https://github.com/pnpm/pnpm/security/advisories

**We do not operate a bounty program.**

### pacquet and pnpr

The Rust port (`pnpm/`) and the resolver server (`pnpr/`) are **not
production ready** and are under active development. Do not report security
issues in them through the security advisory process — open a
[regular issue](https://github.com/pnpm/pnpm/issues) in this repository instead.

## Threat Model and Scope

pnpm's security boundary is **filesystem permissions**. We assume that the store
directory, the project directory, `node_modules`, the lockfile, and pnpm's
configuration files are only writable by parties the user already trusts. A
report that assumes an attacker who already has write access to any of these
locations is **out of scope** — at that point the trust boundary has already
been crossed and the attacker can achieve code execution regardless of pnpm's
behavior.

In particular:

- **The content-addressable store is not a security boundary against a
  write-capable local adversary.** The integrity hashes recorded for each file
  live inside the store itself (e.g. `<storeDir>/v3/files/index.db`), in the same
  trust domain as the files they describe. Anyone who can modify a file in the
  store can also modify its recorded hash. Integrity verification therefore
  exists to detect **accidental corruption** (interrupted writes, bit-rot,
  partially fetched tarballs), not to defend against tampering by someone who can
  already write to the store. Optimizations such as the `mtime` fast path do not
  weaken this guarantee, because the guarantee was never tampering resistance.

- **The store must not be shared among mutually-untrusting users.** A store
  writable by an untrusted party is equivalent to letting that party write
  arbitrary code into your `node_modules`. If you share a store across users or
  CI jobs, restrict write access to trusted identities via filesystem
  permissions.

- **Running pnpm inside a repository means trusting that repository.** This is
  not limited to `pnpm run`, `pnpm exec`, and `pnpm dlx`, which execute whatever
  the repository specifies. A plain `pnpm install` also runs the project's own
  `preinstall`, `install`, `postinstall`, and `prepare` scripts by default;
  `--ignore-scripts` is what suppresses them. The build approval prompt
  (`onlyBuiltDependencies` / `allowBuilds`) gates **dependencies'** build
  scripts, not the scripts of the project you are installing.

  A report therefore does not describe a privilege escalation merely because an
  untrusted repository can influence what pnpm executes — in that starting
  position, code execution is already available to the attacker by design.

- **The workspace's extent is defined by `pnpm-workspace.yaml` and may reach
  outside the repository checkout.** `packages:` patterns may be
  parent-relative (`../shared`), and pnpm treats the directories they name as
  workspace projects: it reads their manifests, records them as lockfile
  importers, and creates `node_modules` inside them. Those directories are part
  of the project trust domain **by declaration**. A hostile edit to
  `pnpm-workspace.yaml` that points the workspace outside the checkout is the
  same starting position as the previous bullet: whoever can change that file
  can already achieve code execution at install time through the configuration
  it carries, so this is not an escalation. The lockfile does not define the
  workspace — an importer entry for a path that no declared pattern matches
  does not cause pnpm to install into that path.

- **Some restrictions are hardening, not boundaries.** Environment variable
  expansion in repository-controlled configuration is suppressed for request
  destinations (`registry`, `proxy`, `pnprServer`) and for auth values. That
  narrows silent credential exfiltration — the case that survives
  `--ignore-scripts`, and the case where a one-line config diff draws less
  review than a new `postinstall` script would. It is deliberately not a
  general-purpose sandbox, and a setting outside its coverage is not a
  vulnerability on that basis alone. What matters is what the gap grants an
  attacker beyond what the repository could already do without it.

The following are examples of reports we consider **out of scope**:

- Tampering with store, lockfile, `node_modules`, or config files that the
  attacker can already write to.
- Bypassing store integrity checks given pre-existing write access to the store.
- Attacks that require the user to run pnpm with a maliciously crafted local
  project or environment that they did not obtain from a trusted source.
- Repository-controlled configuration that changes what `pnpm run`, `pnpm exec`,
  or `pnpm dlx` executes, since those commands run repository-controlled code by
  definition.
- Behavior that matches npm and Yarn and that we would have to diverge from the
  ecosystem to change. Report it upstream first; if they treat it as a
  vulnerability, we will follow.

If you believe a report falls outside these assumptions — for example, a way to
bypass a trust boundary that pnpm *does* enforce — please include the exact
privilege the attacker starts with and how pnpm escalates it. When the report is
about a hardening measure that does not cover some setting, state what the
attacker gains from that gap beyond what they could already do without it.
