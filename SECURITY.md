# Security Policy

## Supported Versions

| Version | Supported          |
| ------- | ------------------ |
| 11.x  | :white_check_mark: |
| 10.x  | :white_check_mark: till 2027 April 30 |
| <= 9.x   | :x:                |

## Reporting a Vulnerability

Submit your findings here: https://github.com/pnpm/pnpm/security/advisories

**We do not operate a bounty program.**

### pacquet and pnpr

The Rust port (`pacquet/`) and the resolver server (`pnpr/`) are **not
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

The following are examples of reports we consider **out of scope**:

- Tampering with store, lockfile, `node_modules`, or config files that the
  attacker can already write to.
- Bypassing store integrity checks given pre-existing write access to the store.
- Attacks that require the user to run pnpm with a maliciously crafted local
  project or environment that they did not obtain from a trusted source.

If you believe a report falls outside these assumptions — for example, a way to
bypass a trust boundary that pnpm *does* enforce — please include the exact
privilege the attacker starts with and how pnpm escalates it.
