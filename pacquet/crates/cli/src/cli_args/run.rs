use clap::Args;
use miette::Context;
use pacquet_executor::execute_shell;
use pacquet_package_manifest::PackageManifest;
use std::path::PathBuf;

#[derive(Debug, Args)]
pub struct RunArgs {
    /// A pre-defined package script.
    pub command: String,

    /// Any additional arguments passed after the script name
    pub args: Vec<String>,

    /// You can use the --if-present flag to avoid exiting with a non-zero exit code when the
    /// script is undefined. This lets you run potentially undefined scripts without breaking the
    /// execution chain.
    #[clap(long)]
    pub if_present: bool,
}

impl RunArgs {
    /// Execute the subcommand.
    pub fn run(self, manifest_path: PathBuf) -> miette::Result<()> {
        let RunArgs { command, args, if_present } = self;

        let manifest = PackageManifest::from_path(manifest_path)
            .wrap_err("getting the package.json in current directory")?;

        if let Some(script) = manifest.script(&command, if_present)? {
            let mut command = script.to_string();
            // append an empty space between script and additional args
            command.push(' ');
            // then append the additional args
            command.push_str(&args.join(" "));
            execute_shell(command.trim())?;
        }

        Ok(())
    }
}
