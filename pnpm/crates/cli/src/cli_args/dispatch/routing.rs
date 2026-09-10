use super::{
    CliCommand, CommandFuture, ExecutionTimeLog, LogEvent, LogLevel, OtpNonInteractiveError,
    RunCtx, dispatch_install, dispatch_query, dispatch_script,
};

/// Route the command and await it, so a routing failure and a command
/// failure reach the caller the same way.
pub(super) async fn run_routed_command(
    command: CliCommand,
    ctx: &RunCtx<'_>,
) -> miette::Result<()> {
    route(command, ctx)?.await
}

/// Route a parsed [`CliCommand`] to its handler. The per-command logic lives
/// in the `dispatch_install` / `dispatch_query` / `dispatch_script` modules,
/// grouped by what the command does (mutate the install graph, read-only
/// query, or run a `package.json` script); this match is only the wiring.
///
/// `completion` / `completion-server` are handled before configuration in
/// [`CliArgs::run_completion_if_requested`](super::CliArgs::run_completion_if_requested), so they are unreachable here.
pub(super) fn route<'a>(
    command: CliCommand,
    ctx: &RunCtx<'a>,
) -> miette::Result<CommandFuture<'a>> {
    match command {
        CliCommand::Add(args) => dispatch_install::add(ctx, args),
        CliCommand::Install(args) => dispatch_install::install(ctx, args),
        CliCommand::InstallTest(args) => dispatch_install::install_test(ctx, args),
        CliCommand::Ci(args) => dispatch_install::ci(ctx, args),
        CliCommand::Pipeline(args) => dispatch_install::pipeline(ctx, args),
        CliCommand::Update(args) => dispatch_install::update(ctx, args),
        CliCommand::Rebuild(args) => dispatch_install::rebuild(ctx, args),
        CliCommand::Remove(args) => dispatch_install::remove(ctx, args),
        CliCommand::Patch(args) => dispatch_install::patch(ctx, args),
        CliCommand::PatchCommit(args) => dispatch_install::patch_commit(ctx, args),
        CliCommand::PatchRemove(args) => dispatch_install::patch_remove(ctx, args),
        CliCommand::Dlx(args) => dispatch_install::dlx(ctx, args),
        CliCommand::Create(args) => dispatch_install::create(ctx, args),
        CliCommand::Runtime(args) => dispatch_install::runtime(ctx, args),
        CliCommand::Env(args) => dispatch_install::env(ctx, args),
        CliCommand::ApproveBuilds(args) => dispatch_install::approve_builds(ctx, args),
        CliCommand::Link(args) => dispatch_install::link(ctx, args),
        CliCommand::Import(args) => dispatch_install::import(ctx, args),
        CliCommand::Dedupe(args) => dispatch_install::dedupe(ctx, args),
        CliCommand::Deploy(args) => dispatch_install::deploy(ctx, args),
        CliCommand::Prune(args) => dispatch_install::prune(ctx, args),
        CliCommand::Fetch(args) => dispatch_install::fetch(ctx, args),
        CliCommand::Unlink(args) => dispatch_install::unlink(ctx, args),
        command => route_registry(command, ctx),
    }
}

fn route_registry<'a>(command: CliCommand, ctx: &RunCtx<'a>) -> miette::Result<CommandFuture<'a>> {
    match command {
        CliCommand::Access(args) => dispatch_query::access(ctx, args),
        CliCommand::Outdated(args) => dispatch_query::outdated(ctx, args),
        CliCommand::Audit(args) => dispatch_query::audit(ctx, args),
        CliCommand::Bugs(args) => dispatch_query::bugs(ctx, args),
        CliCommand::View(args) => dispatch_query::view(ctx, args),
        CliCommand::Whoami => dispatch_query::whoami(ctx),
        CliCommand::Star(args) => dispatch_query::star(ctx, args),
        CliCommand::Unstar(args) => dispatch_query::unstar(ctx, args),
        CliCommand::Stars(args) => dispatch_query::stars(ctx, args),
        CliCommand::DistTag(args) => dispatch_query::dist_tag(ctx, args),
        CliCommand::Team(args) => dispatch_query::team(ctx, args),
        CliCommand::Owner(args) => dispatch_query::owner(ctx, args),
        CliCommand::Deprecate(args) => dispatch_query::deprecate(ctx, args),
        CliCommand::Undeprecate(args) => dispatch_query::undeprecate(ctx, args),
        CliCommand::Unpublish(args) => dispatch_query::unpublish(ctx, args),
        CliCommand::Ping(args) => dispatch_query::ping(ctx, args),
        CliCommand::Search(args) => dispatch_query::search(ctx, args),
        CliCommand::Publish(args) => dispatch_query::publish(ctx, args),
        CliCommand::Token(_) => dispatch_query::not_implemented("token"),
        CliCommand::Docs(args) => dispatch_query::docs(ctx, args),
        CliCommand::Repo(args) => dispatch_query::repo(ctx, args),
        CliCommand::Login(args) => dispatch_query::login(ctx, args),
        CliCommand::Logout(args) => dispatch_query::logout(ctx, args),
        command => route_project(command, ctx),
    }
}

fn route_project<'a>(command: CliCommand, ctx: &RunCtx<'a>) -> miette::Result<CommandFuture<'a>> {
    match command {
        CliCommand::Init(args) => dispatch_script::init(ctx, &args),
        CliCommand::SetScript(args) => dispatch_script::set_script(ctx, args),
        CliCommand::Test(args) => dispatch_script::test(ctx, args),
        CliCommand::Run(args) => dispatch_script::run(ctx, args),
        CliCommand::External(command) => dispatch_script::fallback(ctx, command),
        CliCommand::Exec(args) => dispatch_script::exec(ctx, args),
        CliCommand::Start(args) => dispatch_script::start(ctx, args),
        CliCommand::Stop(args) => dispatch_script::stop(ctx, args),
        CliCommand::Restart(args) => dispatch_script::restart(ctx, args),
        CliCommand::Pkg(args) => dispatch_script::pkg(ctx, args),
        CliCommand::Edit(_) => dispatch_query::not_implemented("edit"),
        CliCommand::Profile(_) => dispatch_query::not_implemented("profile"),
        CliCommand::Xmas(_) => dispatch_query::not_implemented("xmas"),
        command => route_maintenance(command, ctx),
    }
}

fn route_maintenance<'a>(
    command: CliCommand,
    ctx: &RunCtx<'a>,
) -> miette::Result<CommandFuture<'a>> {
    match command {
        CliCommand::Recursive => dispatch_query::recursive(ctx),
        CliCommand::Change(args) => dispatch_query::change(ctx, args),
        CliCommand::Version(args) => dispatch_query::version(ctx, args),
        CliCommand::Lane(args) => dispatch_query::lane(ctx, args),
        CliCommand::List(args) => dispatch_query::list(ctx, args),
        CliCommand::Ll(args) => dispatch_query::ll(ctx, args),
        CliCommand::Licenses(args) => dispatch_query::licenses(ctx, args),
        CliCommand::Why(args) => dispatch_query::why(ctx, args),
        CliCommand::Sbom(args) => dispatch_query::sbom(ctx, args),
        CliCommand::Doctor(args) => dispatch_query::doctor(ctx, args),
        CliCommand::Pack(args) => dispatch_query::pack(ctx, args),
        CliCommand::Stage(args) => dispatch_query::stage(ctx, args),
        CliCommand::Peers(args) => dispatch_query::peers(ctx, args),
        CliCommand::FindHash(args) => dispatch_query::find_hash(ctx, args),
        CliCommand::Shim(args) => dispatch_query::shim(ctx, args),
        CliCommand::Bin(args) => dispatch_query::bin(ctx, args),
        CliCommand::Clean(args) => dispatch_query::clean(ctx, args, "clean"),
        CliCommand::Purge(args) => dispatch_query::clean(ctx, args, "purge"),
        CliCommand::Root(args) => dispatch_query::root(ctx, args),
        CliCommand::Prefix(args) => dispatch_query::prefix(ctx, args),
        CliCommand::Config(args) => dispatch_query::config(ctx, args),
        CliCommand::Get(args) => dispatch_query::config_get(ctx, args),
        CliCommand::Set(args) => dispatch_query::config_set(ctx, args),
        CliCommand::PackApp(args) => dispatch_query::pack_app(ctx, args),
        CliCommand::Store(command) => dispatch_query::store(ctx, command),
        CliCommand::Cache(command) => dispatch_query::cache(ctx, command),
        CliCommand::CatFile(args) => dispatch_query::cat_file(ctx, args),
        CliCommand::CatIndex(args) => dispatch_query::cat_index(ctx, args),
        CliCommand::IgnoredBuilds(args) => dispatch_query::ignored_builds(ctx, args),
        CliCommand::SelfUpdate(args) => dispatch_query::self_update(ctx, args),
        CliCommand::Setup(args) => dispatch_query::setup(ctx, args),
        CliCommand::With(args) => dispatch_query::with(ctx, args),
        CliCommand::Completion(_) | CliCommand::CompletionServer(_) => {
            unreachable!("completion returns before configuration")
        }
        _ => unreachable!("installation and registry commands are routed before project commands"),
    }
}

pub(super) fn prints_json_errors(command: &CliCommand) -> bool {
    match command {
        CliCommand::Publish(args) => args.flags.json,
        CliCommand::View(args) => args.json,
        CliCommand::Pack(args) => args.json,
        _ => false,
    }
}

pub(super) fn print_json_error(error: &miette::Report) {
    let code = error.code().map_or_else(|| "pnpm".to_string(), |code| code.to_string());
    let message = json_error_message(error);
    let mut error_body = serde_json::json!({
        "code": code,
        "message": message,
    });
    if let Some(otp_error) = otp_non_interactive_error(error) {
        if let Some(auth_url) = &otp_error.auth_url {
            error_body["authUrl"] = serde_json::Value::String(auth_url.clone());
        }
        if let Some(done_url) = &otp_error.done_url {
            error_body["doneUrl"] = serde_json::Value::String(done_url.clone());
        }
    }
    let output = serde_json::json!({
        "error": error_body,
    });
    // pnpm's `errorHandler` prints the envelope with `JSON.stringify(_, null, 2)`;
    // match its two-space indentation byte-for-byte.
    let output = serde_json::to_string_pretty(&output).expect("a JSON error envelope serializes");
    println!("{output}");
}

pub(super) fn json_error_message(error: &miette::Report) -> String {
    let mut messages = error.chain().map(ToString::to_string);
    match (messages.next(), messages.next()) {
        (Some(context), Some(source)) if context == super::super::pack::PACK_ERROR_CONTEXT => {
            source
        }
        (Some(message), _) => message,
        (None, _) => error.to_string(),
    }
}

fn otp_non_interactive_error(error: &miette::Report) -> Option<&OtpNonInteractiveError> {
    error
        .downcast_ref::<OtpNonInteractiveError>()
        .or_else(|| error.chain().find_map(|cause| cause.downcast_ref::<OtpNonInteractiveError>()))
}

pub(super) fn now_millis() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis())
}

/// Install fast paths emit the same completion event as the full command.
pub(super) fn emit_execution_time(emit: fn(&LogEvent), started_at: u128) {
    emit(&LogEvent::ExecutionTime(ExecutionTimeLog {
        level: LogLevel::Debug,
        started_at,
        ended_at: now_millis(),
    }));
}
