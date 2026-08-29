---
"pacquet": minor
---

Added an experimental `pnpm vcs` command to the Rust pnpm CLI. It initializes an ordinary pnpm workspace as a Bit workspace, automatically maps every workspace project and unclaimed root file to Bit components, reports component status, and commits all changes as one snap batch. The integration invokes Bit directly through a versioned JSON protocol and does not invoke Git.
