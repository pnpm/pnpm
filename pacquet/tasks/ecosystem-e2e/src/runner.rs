use crate::{
    cli_args::{Binary, Layout},
    stacks::{PROJECT_DIR, Serve, Stack},
};
use std::{
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Write},
    net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

/// One grid cell: a stack installed by a binary under a layout.
#[derive(Debug)]
pub struct Cell<'stack> {
    pub stack: &'stack Stack,
    pub binary: Binary,
    pub layout: Layout,
}

#[derive(Debug)]
pub struct Outcome {
    pub passed: bool,
    pub duration_secs: f64,
    pub stage: &'static str,
    pub message: String,
    pub log_path: PathBuf,
}

impl Cell<'_> {
    pub fn id(&self) -> String {
        format!("{}--{}--{}", self.stack.name, self.binary.label(), self.layout.label())
    }
}

/// Scaffold a stack into `<template_root>/<stack>` once, without installing
/// dependencies. The generated tree is later copied into each cell.
///
/// With `reuse`, an already-scaffolded template (a `<stack>/app` directory
/// holding a `package.json`) is returned untouched — re-running a generator
/// into a non-empty directory typically fails. Without it, the caller is
/// expected to have wiped the work dir, so scaffolding starts clean.
pub fn scaffold_template(
    pnpm: &str,
    stack: &Stack,
    template_root: &Path,
    log_path: &Path,
    reuse: bool,
) -> Result<PathBuf, String> {
    let template_dir = template_root.join(stack.name);
    let project_dir = template_dir.join(PROJECT_DIR);
    if reuse && project_dir.join("package.json").is_file() {
        return Ok(project_dir);
    }
    fs::create_dir_all(&template_dir)
        .map_err(|error| format!("create {template_dir:?}: {error}"))?;
    for command in stack.scaffold {
        let mut process = sandboxed_command(pnpm);
        process.current_dir(&template_dir).arg("dlx").arg(command.spec).args(command.args);
        run(
            &format!("pnpm dlx {} {}", command.spec, command.args.join(" ")),
            &mut process,
            log_path,
        )?;
    }
    if !project_dir.is_dir() {
        return Err(format!("scaffold did not produce {project_dir:?}"));
    }
    Ok(project_dir)
}

/// Install and build a single cell. Returns whichever stage failed first.
pub fn run_cell(
    cell: &Cell,
    template_project: &Path,
    cells_root: &Path,
    pnpm: &str,
    pacquet: &str,
    serve: bool,
) -> Outcome {
    let started = Instant::now();
    let cell_dir = cells_root.join(cell.id());
    let project_dir = cell_dir.join(PROJECT_DIR);
    let log_path = cell_dir.join("cell.log");
    // Start each run with a fresh log so reruns (notably under `--keep`) don't
    // interleave with stale output.
    let _ = fs::remove_file(&log_path);

    let outcome = |stage: &'static str, result: Result<(), String>| -> Option<Outcome> {
        result.err().map(|message| Outcome {
            passed: false,
            duration_secs: started.elapsed().as_secs_f64(),
            stage,
            message,
            log_path: log_path.clone(),
        })
    };

    if let Some(failed) =
        outcome("prepare", prepare_cell(template_project, &cell_dir, &project_dir, cell))
    {
        return failed;
    }

    let install_binary = match cell.binary {
        Binary::Pnpm => pnpm,
        Binary::Pacquet => pacquet,
    };
    let mut install = sandboxed_command(install_binary);
    install.current_dir(&project_dir).arg("install");
    if let Some(failed) = outcome("install", run("install", &mut install, &log_path)) {
        return failed;
    }

    if let Some(failed) =
        outcome("build", run_build_script(&project_dir, cell.stack.build_script, &log_path))
    {
        return failed;
    }

    let serve_spec = serve.then_some(cell.stack.serve.as_ref()).flatten();
    if let Some(spec) = serve_spec
        && let Some(failed) = outcome("serve", run_serve(&project_dir, spec, &log_path))
    {
        return failed;
    }

    Outcome {
        passed: true,
        duration_secs: started.elapsed().as_secs_f64(),
        stage: "done",
        message: String::new(),
        log_path,
    }
}

fn prepare_cell(
    template_project: &Path,
    cell_dir: &Path,
    project_dir: &Path,
    cell: &Cell,
) -> Result<(), String> {
    fs::create_dir_all(cell_dir).map_err(|error| format!("create {cell_dir:?}: {error}"))?;
    if project_dir.exists() {
        fs::remove_dir_all(project_dir)
            .map_err(|error| format!("clean {project_dir:?}: {error}"))?;
    }
    copy_tree(template_project, project_dir)?;
    write_workspace_yaml(cell_dir, project_dir, cell.layout)
}

/// Pin the store and cache inside the cell so pnpm and pacquet never share a
/// store, and so every cell starts cold. The explicit
/// `enableGlobalVirtualStore` matters under CI: CI defaults it to `false`,
/// but an explicit value in `pnpm-workspace.yaml` is respected.
///
/// `dangerouslyAllowAllBuilds` lets dependency build scripts run unattended
/// (esbuild, etc.) so the build stage exercises a real, fully-built
/// `node_modules` instead of stopping at an approval prompt. That is the
/// point of an ecosystem run, not a security stance — these are throwaway
/// projects scaffolded from pinned generators.
fn write_workspace_yaml(cell_dir: &Path, project_dir: &Path, layout: Layout) -> Result<(), String> {
    let store_dir = cell_dir.join("store");
    let cache_dir = cell_dir.join("cache");
    let contents = format!(
        "enableGlobalVirtualStore: {}\ndangerouslyAllowAllBuilds: true\nstoreDir: {}\ncacheDir: {}\n",
        layout.enable_global_virtual_store(),
        store_dir.display(),
        cache_dir.display(),
    );
    let path = project_dir.join("pnpm-workspace.yaml");
    fs::write(&path, contents).map_err(|error| format!("write {path:?}: {error}"))
}

/// Run a `package.json` script with `node_modules/.bin` on `PATH`, without
/// going through any package manager. The install is the thing under test;
/// the build is just a layout-agnostic check that the produced
/// `node_modules` is usable, so it must not let pnpm or pacquet re-install
/// and mask the result.
fn run_build_script(project_dir: &Path, script_name: &str, log_path: &Path) -> Result<(), String> {
    let manifest_path = project_dir.join("package.json");
    let manifest_text = fs::read_to_string(&manifest_path)
        .map_err(|error| format!("read {manifest_path:?}: {error}"))?;
    let manifest: serde_json::Value = serde_json::from_str(&manifest_text)
        .map_err(|error| format!("parse {manifest_path:?}: {error}"))?;
    let script = manifest
        .get("scripts")
        .and_then(|scripts| scripts.get(script_name))
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| format!("no `{script_name}` script in {manifest_path:?}"))?;

    // Absolute `/bin/sh`, not `sh`: the child's PATH is prepended with the
    // project's `node_modules/.bin`, and Rust resolves a bare program name
    // against that child PATH — a dependency shipping a `.bin/sh` would
    // otherwise run in place of the system shell. The script the shell runs
    // still resolves framework bins from `.bin` via that PATH.
    let mut process = sandboxed_command("/bin/sh");
    process.current_dir(project_dir).arg("-c").arg(script).env("PATH", bin_path(project_dir)?);
    run(&format!("run {script_name}: {script}"), &mut process, log_path)
}

/// Environment variables an install/build/serve subprocess legitimately
/// needs. Everything else is scrubbed, so a lifecycle script from an
/// untrusted package can't read ambient CI secrets (`GITHUB_TOKEN`, npm auth
/// tokens, etc.) out of the environment. Build/serve callers override `PATH`
/// with a `node_modules/.bin`-prefixed one; the rest inherit it from here.
const ENV_PASSTHROUGH: &[&str] = &[
    "PATH",
    "HOME",
    "TMPDIR",
    "TEMP",
    "TMP",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "TERM",
    "USER",
    "LOGNAME",
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "NO_PROXY",
    "http_proxy",
    "https_proxy",
    "no_proxy",
    "NODE_EXTRA_CA_CERTS",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
];

/// A [`Command`] whose environment is scrubbed down to [`ENV_PASSTHROUGH`].
/// Used for every launch that runs third-party code — scaffolding,
/// installs, and the build/serve scripts — so unattended lifecycle scripts
/// can't read ambient secrets out of the parent environment.
fn sandboxed_command(program: &str) -> Command {
    let mut command = Command::new(program);
    command.env_clear();
    for key in ENV_PASSTHROUGH {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    command
}

/// `PATH` with the project's `node_modules/.bin` prepended, so locally
/// installed binaries (next, vite, etc.) resolve ahead of anything global.
fn bin_path(project_dir: &Path) -> Result<OsString, String> {
    let bin_dir = project_dir.join("node_modules").join(".bin");
    match std::env::var_os("PATH") {
        Some(existing) => {
            let mut entries = vec![bin_dir];
            entries.extend(std::env::split_paths(&existing));
            std::env::join_paths(entries).map_err(|error| format!("compose PATH: {error}"))
        }
        None => Ok(bin_dir.into_os_string()),
    }
}

/// Start the built app, poll it over HTTP until it serves a non-error
/// response, then tear it down. Proves the produced `node_modules` works at
/// runtime, not just at bundle time. The server is always killed before
/// returning, whatever the outcome.
fn run_serve(project_dir: &Path, serve: &Serve, log_path: &Path) -> Result<(), String> {
    let port = pick_free_port()?;
    let args: Vec<String> =
        serve.command.iter().map(|token| token.replace("{port}", &port.to_string())).collect();
    let (program, rest) = args.split_first().ok_or("serve command is empty")?;
    // Run the argv directly off the project's `.bin` instead of joining it
    // into a `sh -c` string, so tokens keep their boundaries and don't need
    // shell quoting. The spawned PID is the server itself, so it's killable.
    let program_path = project_dir.join("node_modules").join(".bin").join(program);

    let mut log = OpenOptions::new()
        .append(true)
        .open(log_path)
        .map_err(|error| format!("open log {log_path:?}: {error}"))?;
    let _ = writeln!(
        log,
        "\n$ serve: {} (probe http://127.0.0.1:{port}{})",
        args.join(" "),
        serve.ready_path,
    );

    let mut child = sandboxed_command(&program_path.to_string_lossy())
        .current_dir(project_dir)
        .args(rest)
        .env("PATH", bin_path(project_dir)?)
        // Servers that take their port via env rather than a flag (nitro,
        // react-router-serve, etc.) honor these; for the rest the explicit
        // `{port}` flag wins and PORT is harmless.
        .env("PORT", port.to_string())
        .env("HOST", "127.0.0.1")
        .stdin(Stdio::null())
        .stdout(clone_handle(&log, log_path)?)
        .stderr(clone_handle(&log, log_path)?)
        .spawn()
        .map_err(|error| format!("spawn server `{}`: {error}", args.join(" ")))?;

    let result = wait_until_serving(&mut child, port, serve);
    let _ = child.kill();
    let _ = child.wait();
    result
}

fn wait_until_serving(
    child: &mut std::process::Child,
    port: u16,
    serve: &Serve,
) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(serve.timeout_secs);
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            return Err(format!("server exited before serving ({status})"));
        }
        // A 2xx/3xx means the server served the route through the installed
        // layout. A connection error or an HTTP error status is retried until
        // the deadline — many dev servers briefly refuse connections or answer
        // 4xx/5xx while warming up.
        let observation = match probe(port, serve.ready_path) {
            Ok(code) if (200..400).contains(&code) => return Ok(()),
            Ok(code) => format!("HTTP {code} at {}", serve.ready_path),
            Err(error) => error,
        };
        if Instant::now() >= deadline {
            return Err(format!(
                "not serving within {}s (last: {observation})",
                serve.timeout_secs,
            ));
        }
        thread::sleep(Duration::from_millis(500));
    }
}

/// Issue one `GET` and return the HTTP status code, or an error if the
/// server isn't accepting connections / didn't answer with a status line.
/// Only the status line is read; the body is left unread and the connection
/// dropped.
fn probe(port: u16, path: &str) -> Result<u16, String> {
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(2))
        .map_err(|error| format!("connect: {error}"))?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let request = format!("GET {path} HTTP/1.0\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).map_err(|error| format!("write request: {error}"))?;
    let mut status_line = String::new();
    BufReader::new(stream)
        .read_line(&mut status_line)
        .map_err(|error| format!("read response: {error}"))?;
    status_line
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .ok_or_else(|| format!("unparsable status line: {status_line:?}"))
}

fn pick_free_port() -> Result<u16, String> {
    let listener =
        TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).map_err(|error| format!("bind: {error}"))?;
    listener.local_addr().map(|addr| addr.port()).map_err(|error| format!("local_addr: {error}"))
}

fn run(label: &str, command: &mut Command, log_path: &Path) -> Result<(), String> {
    let mut log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .map_err(|error| format!("open log {log_path:?}: {error}"))?;
    let _ = writeln!(log, "\n$ {label}");
    let stdout = clone_handle(&log, log_path)?;
    let stderr = clone_handle(&log, log_path)?;
    let status = command
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .status()
        .map_err(|error| format!("spawn `{label}`: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("`{label}` exited with {status} (see {})", log_path.display()))
    }
}

fn clone_handle(log: &File, log_path: &Path) -> Result<File, String> {
    log.try_clone().map_err(|error| format!("clone log handle {log_path:?}: {error}"))
}

fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    let status = Command::new("cp")
        .arg("-R")
        .arg(from)
        .arg(to)
        .status()
        .map_err(|error| format!("spawn cp: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("cp -R {from:?} {to:?} exited with {status}"))
    }
}
