mod cli_args;
mod fixtures;
mod verify;
mod work_env;
mod workspace_manifest;

#[tokio::main]
async fn main() {
    use pipe_trait::Pipe;

    let cli_args::CliArgs {
        scenario,
        registry_port,
        verdaccio,
        repository,
        fixture_dir,
        hyperfine_options,
        work_env,
        with_pnpm,
        build_only,
        revisions,
    } = clap::Parser::parse();
    let repository = std::fs::canonicalize(repository).expect("get absolute path to repository");
    if !work_env.exists() {
        std::fs::create_dir_all(&work_env).expect("create work env");
    }
    let work_env = std::fs::canonicalize(work_env).expect("get absolute path to work env");
    let registry = format!("http://localhost:{registry_port}/");
    let verdaccio = if build_only {
        None
    } else if verdaccio {
        verify::ensure_program("just").arg("install").pipe(verify::executor("just install"));
        pacquet_registry_mock::MockInstanceOptions {
            client: &Default::default(),
            port: registry_port,
            stdout: work_env.join("verdaccio.stdout.log").pipe(Some).as_deref(),
            stderr: work_env.join("verdaccio.stderr.log").pipe(Some).as_deref(),
            max_retries: 10,
            retry_delay: tokio::time::Duration::from_millis(500),
        }
        .spawn_if_necessary()
        .await
    } else {
        verify::ensure_virtual_registry(&registry).await;
        None
    };
    verify::ensure_git_repo(&repository);
    verify::validate_revision_list(&revisions);
    verify::ensure_program("bash");
    verify::ensure_program("cargo");
    verify::ensure_program("git");
    verify::ensure_program("hyperfine");
    verify::ensure_program("pnpm");
    let env = work_env::WorkEnv {
        root: work_env,
        with_pnpm,
        revisions,
        registry,
        repository,
        scenario,
        hyperfine_options,
        fixture_dir,
    };
    if build_only {
        env.build();
    } else {
        env.run();
    }
    drop(verdaccio); // terminate verdaccio if exists
}
