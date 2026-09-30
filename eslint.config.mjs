import eslintConfig, { sizeAndShapeRules } from "@pnpm/eslint-config";
import * as regexpPlugin from "eslint-plugin-regexp";

// Packages whose code still exceeds the size and shape limits. Remove a
// package from this list once it is refactored to pass them.
const PENDING_SIZE_AND_SHAPE_REFACTOR = [
    ".meta-updater/**",
    "pnpm11/__utils__/scripts/**",
    "pnpm11/__utils__/test-fixtures/**",
    "pnpm11/bins/cmd-shim/**",
    "pnpm11/deps/graph-builder/**",
    "pnpm11/deps/security/signatures/**",
    "pnpm11/engine/runtime/commands/**",
    "pnpm11/exec/lifecycle/**",
    "pnpm11/exec/prepare-package/**",
    "pnpm11/fetching/directory-fetcher/**",
    "pnpm11/fetching/git-fetcher/**",
    "pnpm11/fetching/pick-fetcher/**",
    "pnpm11/fetching/tarball-fetcher/**",
    "pnpm11/fs/indexed-pkg-importer/**",
    "pnpm11/global/packages/**",
    "pnpm11/hooks/pnpmfile/**",
    "pnpm11/hooks/read-package-hook/**",
    "pnpm11/installing/context/**",
    "pnpm11/installing/linking/direct-dep-linker/**",
    "pnpm11/installing/linking/modules-cleaner/**",
    "pnpm11/installing/linking/real-hoist/**",
    "pnpm11/installing/modules-yaml/**",
    "pnpm11/lockfile/detect-dep-types/**",
    "pnpm11/lockfile/peer-edges/**",
    "pnpm11/lockfile/preferred-versions/**",
    "pnpm11/lockfile/pruner/**",
    "pnpm11/lockfile/settings-checker/**",
    "pnpm11/lockfile/utils/**",
    "pnpm11/lockfile/walker/**",
    "pnpm11/modules-mounter/daemon/**",
    "pnpm11/network/agent/**",
    "pnpm11/network/auth-header/**",
    "pnpm11/object/property-path/**",
    "pnpm11/os/env/path-extender-posix/**",
    "pnpm11/patching/apply-patch/**",
    "pnpm11/patching/config/**",
    "pnpm11/pkg-manifest/commands/**",
    "pnpm11/pkg-manifest/utils/**",
    "pnpm11/releasing/exportable-manifest/**",
    "pnpm11/resolving/default-resolver/**",
    "pnpm11/resolving/git-resolver/**",
    "pnpm11/resolving/local-resolver/**",
    "pnpm11/resolving/registry/pkg-metadata-filter/**",
    "pnpm11/store/commands/**",
    "pnpm11/store/connection-manager/**",
    "pnpm11/store/file-mode/**",
    "pnpm11/store/index/**",
    "pnpm11/testing/registry-mock/**",
    "pnpm11/text/comments-parser/**",
    "pnpm11/text/tree-renderer/**",
    "pnpm11/workspace/commands/**",
    "pnpm11/workspace/injected-deps-syncer/**",
    "pnpm11/workspace/project-manifest-reader/**",
    "pnpm11/workspace/projects-graph/**",
    "pnpm11/workspace/projects-reader/**",
    "pnpm11/workspace/task-scheduler/**",
    "pnpm11/workspace/workspace-manifest-reader/**",
    "pnpm11/yaml/document-sync/**"
]

export default [
    {
        ignores: ["**/fixtures", "**/__fixtures__", "**/node_modules", "**/lib", ".claude/**", "bench-work-env/**", "**/example"],
    },
    ...eslintConfig,
    regexpPlugin.configs['flat/recommended'],
    {
        files: ["pnpm11/pnpm/src/**/*.ts"],
        rules: {
            "import-x/no-extraneous-dependencies": "off",
        },
    },
    {
        files: PENDING_SIZE_AND_SHAPE_REFACTOR,
        rules: Object.fromEntries(
            Object.keys(sizeAndShapeRules)
                .filter((rule) => rule !== "perfectionist/overly-complex-condition" && rule !== "perfectionist/overly-long-method-chain")
                .map((rule) => [rule, "off"])
        ),
    },
]
