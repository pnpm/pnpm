/// A real-world JavaScript stack to install and build.
///
/// `scaffold` generates the project on disk *without* installing
/// dependencies — it runs once per stack (the generated files are identical
/// across binaries and layouts) and the result is copied into each grid
/// cell, where the binary-under-test performs the actual install. Every
/// scaffold command is run through `pnpm dlx`, so the first token is the
/// package spec to fetch and the rest are its arguments.
#[derive(Debug, Clone, Copy)]
pub struct Stack {
    pub name: &'static str,
    pub description: &'static str,
    pub scaffold: &'static [ScaffoldCommand],
    /// `package.json` script that builds the app once dependencies exist.
    pub build_script: &'static str,
    /// How to boot and probe the built app. A passing build only proves the
    /// layout resolves at bundle time; serving proves the layout works at
    /// runtime — request-time `require`, SSR, native addons. `None` skips
    /// the serve stage for stacks without a server.
    pub serve: Option<Serve>,
}

/// Boots the built app and confirms it serves a non-error HTTP response.
#[derive(Debug, Clone, Copy)]
pub struct Serve {
    /// Command tokens run with `node_modules/.bin` on `PATH` to start the
    /// production server. The literal token `{port}` is replaced with a
    /// free port picked at run time.
    pub command: &'static [&'static str],
    /// HTTP path polled until the server answers (e.g. `/`).
    pub ready_path: &'static str,
    /// How long to wait for the server to start answering before failing.
    pub timeout_secs: u64,
}

/// One `pnpm dlx <spec> <args...>` invocation. `dir` is the project
/// directory name the generator should create; it is passed verbatim as one
/// of `args` (the harness substitutes nothing — keep the literal `app`).
#[derive(Debug, Clone, Copy)]
pub struct ScaffoldCommand {
    pub spec: &'static str,
    pub args: &'static [&'static str],
}

/// Directory name every generator scaffolds into, inside a cell.
pub const PROJECT_DIR: &str = "app";

/// Stacks are pinned to a major version on purpose: an unpinned generator
/// tracking `@latest` turns an upstream framework change into a red cell
/// that looks like a pnpm/pacquet regression. Bump these deliberately.
pub const STACKS: &[Stack] = &[
    Stack {
        name: "next",
        description: "Next.js app-router project (next build)",
        scaffold: &[ScaffoldCommand {
            spec: "create-next-app@15",
            args: &[
                "app",
                "--ts",
                "--app",
                "--no-eslint",
                "--no-tailwind",
                "--no-src-dir",
                "--no-turbopack",
                "--no-import-alias",
                "--use-pnpm",
                "--skip-install",
            ],
        }],
        build_script: "build",
        serve: Some(Serve {
            command: &["next", "start", "--port", "{port}", "--hostname", "127.0.0.1"],
            ready_path: "/",
            timeout_secs: 60,
        }),
    },
    Stack {
        name: "vite-react",
        description: "Vite + React + TypeScript project (vite build)",
        scaffold: &[ScaffoldCommand {
            spec: "create-vite@6",
            args: &["app", "--template", "react-ts"],
        }],
        build_script: "build",
        serve: Some(Serve {
            command: &[
                "vite",
                "preview",
                "--port",
                "{port}",
                "--strictPort",
                "--host",
                "127.0.0.1",
            ],
            ready_path: "/",
            timeout_secs: 30,
        }),
    },
    Stack {
        name: "angular",
        description: "Angular CLI project (ng build + ng serve dev server)",
        scaffold: &[ScaffoldCommand {
            spec: "@angular/cli@19",
            args: &[
                "new",
                "app",
                "--defaults",
                "--skip-install",
                "--skip-git",
                "--package-manager",
                "pnpm",
            ],
        }],
        build_script: "build",
        serve: Some(Serve {
            command: &["ng", "serve", "--port", "{port}", "--host", "127.0.0.1"],
            ready_path: "/",
            timeout_secs: 120,
        }),
    },
    Stack {
        name: "astro",
        description: "Astro minimal project (astro build + astro preview)",
        scaffold: &[ScaffoldCommand {
            spec: "create-astro@5",
            args: &[
                "app",
                "--template",
                "minimal",
                "--no-install",
                "--no-git",
                "--skip-houston",
                "--typescript",
                "strict",
            ],
        }],
        build_script: "build",
        serve: Some(Serve {
            command: &["astro", "preview", "--port", "{port}", "--host", "127.0.0.1"],
            ready_path: "/",
            timeout_secs: 30,
        }),
    },
    Stack {
        name: "sveltekit",
        description: "SvelteKit minimal project (vite build + vite preview)",
        scaffold: &[ScaffoldCommand {
            spec: "sv@0.16",
            args: &[
                "create",
                "app",
                "--template",
                "minimal",
                "--types",
                "ts",
                "--no-add-ons",
                "--no-install",
            ],
        }],
        build_script: "build",
        serve: Some(Serve {
            command: &[
                "vite",
                "preview",
                "--port",
                "{port}",
                "--strictPort",
                "--host",
                "127.0.0.1",
            ],
            ready_path: "/",
            timeout_secs: 30,
        }),
    },
    Stack {
        name: "nuxt",
        description: "Nuxt project (nuxt build + nuxi preview, port via env)",
        scaffold: &[ScaffoldCommand {
            spec: "nuxi@3",
            args: &[
                "init",
                "app",
                "--template",
                "minimal",
                "--packageManager",
                "pnpm",
                "--no-install",
                "--no-gitInit",
            ],
        }],
        build_script: "build",
        serve: Some(Serve { command: &["nuxi", "preview"], ready_path: "/", timeout_secs: 60 }),
    },
    Stack {
        name: "react-router",
        description: "React Router 7 framework project (build + react-router-serve, port via env)",
        scaffold: &[ScaffoldCommand {
            spec: "create-react-router@7",
            args: &["app", "--no-install", "--no-git-init", "--yes"],
        }],
        build_script: "build",
        serve: Some(Serve {
            command: &["react-router-serve", "./build/server/index.js"],
            ready_path: "/",
            timeout_secs: 30,
        }),
    },
];

pub fn select(names: &[String]) -> Result<Vec<&'static Stack>, &str> {
    if names.is_empty() {
        return Ok(STACKS.iter().collect());
    }
    names
        .iter()
        .map(|name| STACKS.iter().find(|stack| stack.name == name).ok_or(name.as_str()))
        .collect()
}
