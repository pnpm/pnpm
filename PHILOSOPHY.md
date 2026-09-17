# pnpm project philosophy

## Core requirements

Security, performance, efficient disk usage, and predictability are core
requirements. Design for them together. Look for a design that preserves all
four before accepting a tradeoff.

For example, lockfile verification can retain a secure default while caching
verification work for fast installs. An explicit option to trust the lockfile
lets users choose speed when their environment provides other security checks.
When a tradeoff remains, provide explicit options that let users choose for
their environment. Keep secure behavior as the default and make each option's
behavior predictable.

## Features and abstractions

First assess whether existing pnpm capabilities, alone or in combination,
solve the user's problem. Reuse them when they do. Extend the owning capability
when it leaves a gap and the extension fits the overall architecture.

A feature expected to be widely used can justify added complexity. Evaluate
how that complexity changes the whole system: a broader abstraction can make
older features simpler by expressing them as cases of the new model. Favor
that consolidation over accumulating independent special cases. Prioritize
code reuse and deduplication, while preserving meaningful differences in
behavior.

Compatibility can delay consolidation of stable features until the next major
version. Temporary duplication is acceptable until then, with the intended
shared abstraction and consolidation point identified. Experimental features
can be consolidated immediately.

## Compatibility and releases

Breaking changes to stable behavior require a major version bump. Experimental
areas can change before the next major release.

npm compatibility is useful evidence, not a requirement. pnpm can choose its
own behavior for a clear reason. Agreement among several alternative package
managers is a signal to consider following their approach, but cannot override
pnpm's core requirements.

## Concrete benefits

Evaluate ease-of-use claims through concrete benefits, such as fewer steps for
a common task or clearer recovery from an error. Ease of use is subjective and
does not replace evidence of value or override the core requirements.
