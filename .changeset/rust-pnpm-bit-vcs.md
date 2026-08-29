---
"pacquet": minor
---

Added an experimental `pnpm vcs` command to the Rust pnpm CLI. It initializes an ordinary pnpm workspace as a Bit workspace, automatically maps every workspace project and unclaimed root file to Bit components, persists durable version-free component identities in `pnpm-workspace.yaml`, reconstructs a workspace with `pnpm vcs clone`, validates component toolchain requirements against one locked workspace profile, reports component status, and commits all changes as one snap batch. The integration invokes Bit directly through a versioned JSON protocol and does not invoke Git.
