---
"pacquet": patch
---

The projects that run their own lifecycle scripts (`preinstall`, `install`, `postinstall`, `prepare`, …) now match pnpm in every install-family command. A project runs them when the command installs it in full, and — in a workspace the command only partly covers — whenever the command mutates it at all; the workspace root runs them even when the command was pointed at another project, because it is installed in full alongside it. As a result, `pnpm update <pkg>` and `pnpm add <pkg>` in a workspace no longer skip the workspace root's scripts, `pnpm update` at a workspace root no longer runs the other members' scripts, and `pnpm update --latest` no longer runs the project's own scripts (it rewrites named dependency specs, so it is a partial install like `pnpm update <pkg>`) [#13358](https://github.com/pnpm/pnpm/issues/13358).
