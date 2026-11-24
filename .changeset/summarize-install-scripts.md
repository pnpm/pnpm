---
"pnpm": minor
---

An install lists the lifecycle scripts it ran in the checks footer above `Done in`. One line names the project scripts that ran, one the dependencies that were built, and one the dependency builds that came from the side-effects cache. A dependency that was skipped because its optional build failed is listed as a problem.

In a terminal, a script's output is shown while it runs and erased when it succeeds. A failed script keeps its output. In CI and piped output, a script that succeeds without printing anything no longer prints any lines.
