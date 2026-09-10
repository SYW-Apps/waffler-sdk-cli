//! `waffler` — the argument layer, and nothing else.
//!
//! EVERY COMMAND IS A PARSE, A PORTAL CALL, AND A RENDER. No decision is made here that the library
//! cannot make on its own, because the docker image build drives this crate as a library and anything
//! implemented in a command handler is invisible to it. When a check belongs "in the CLI", it belongs
//! one layer down.

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use console::style;

use waffler_cli::project::project_portal;
use waffler_cli::publishing::publish_portal;
use waffler_cli::session::session_portal;

#[derive(Parser)]
#[command(
    name = "waffler",
    about = "Waffler Package Developer SDK",
    long_about = "Create, build, pack and publish Waffler packages.\n\nWhich registry a command talks \
                  to is resolved from --registry, then WAFFLER_REGISTRY, then the default set by \
                  `waffler use`, then the built-in — and every command says which one it used.",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Create a new package project that packs, publishes and installs
    Scaffold(ScaffoldArgs),
    /// Build the project's declared artifacts, in the profile a publishable bundle requires
    Build(PathArgs),
    /// Check the manifest without building — every problem at once, not the first
    Validate(PathArgs),
    /// Build if needed and write a bundle. No network, no credential
    Pack(PackArgs),
    /// Build a bundle if needed and upload it to the resolved registry
    Publish(PublishArgs),
    /// Withdraw a published version from the registry
    Unpublish(UnpublishArgs),
    /// Sign in to a registry, against whatever issuer that registry advertises
    Login(RegistryArgs),
    /// Sign out of one registry, leaving every other session intact
    Logout(RegistryArgs),
    /// Report the identity held for a registry, naming which
    Whoami(RegistryArgs),
    /// Make a registry the default for later commands
    Use(UseArgs),
    /// Show which registry is resolved, and what it says about itself
    Registry(RegistryArgs),
}

#[derive(clap::Args)]
struct PathArgs {
    /// Project directory
    #[arg(default_value = ".")]
    path: PathBuf,
}

#[derive(clap::Args)]
struct ScaffoldArgs {
    /// The new package's fully-qualified id, e.g. syw.example.hello
    fqid: String,
    /// Where to create it (defaults to the fqid)
    #[arg(long, short)]
    path: Option<PathBuf>,
    #[arg(long, default_value = "0.1.0")]
    version: String,
    #[arg(long, default_value = "A Waffler package.")]
    description: String,
    /// Path to a Waffler checkout, for the SDK path dependencies.
    ///
    /// REQUIRED IN PRACTICE BECAUSE THE SDK IS NOT PUBLISHED. A generated crate has to point its
    /// dependencies somewhere real, and a scaffold that emitted a line pointing nowhere would produce
    /// a project that cannot build — the exact failure a scaffold exists to prevent.
    #[arg(long, default_value = "../..")]
    sdk_path: String,
    /// Write into a directory that is not empty
    #[arg(long)]
    overwrite: bool,
}

#[derive(clap::Args)]
struct PackArgs {
    #[arg(default_value = ".")]
    path: PathBuf,
    /// Where to write the bundle (defaults to <fqid>.zip in the project directory)
    #[arg(long, short)]
    output: Option<PathBuf>,
    /// Reuse already-built artifacts
    #[arg(long)]
    no_build: bool,
}

#[derive(clap::Args)]
struct PublishArgs {
    #[arg(default_value = ".")]
    path: PathBuf,
    /// Publish an already-built bundle instead of packing the project
    #[arg(long, short)]
    bundle: Option<PathBuf>,
    #[arg(long, env = "WAFFLER_REGISTRY")]
    registry: Option<String>,
    #[arg(long)]
    no_build: bool,
}

#[derive(clap::Args)]
struct UnpublishArgs {
    /// The package to withdraw from
    fqid: String,
    /// The version to withdraw
    version: String,
    #[arg(long, env = "WAFFLER_REGISTRY")]
    registry: Option<String>,
}

#[derive(clap::Args)]
struct RegistryArgs {
    #[arg(long, env = "WAFFLER_REGISTRY")]
    registry: Option<String>,
}

#[derive(clap::Args)]
struct UseArgs {
    /// The registry to make default
    url: String,
}

#[tokio::main]
async fn main() {
    if let Err(e) = run(Cli::parse()).await {
        // `{:#}` so anyhow's context chain is printed. A tool that shows only the outermost message
        // reports "publish failed" for an error whose cause was three layers down and specific.
        eprintln!("{} {:#}", style("error:").red().bold(), e);
        std::process::exit(1);
    }
}

async fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Commands::Scaffold(a) => {
            let directory = a.path.unwrap_or_else(|| PathBuf::from(&a.fqid));
            let files = project_portal::scaffold(
                &directory,
                &a.fqid,
                &a.version,
                &a.description,
                &a.sdk_path,
                a.overwrite,
            )?;
            println!("{} created {} in {}", style("✓").green().bold(), plural(files.len(), "file"), directory.display());
            for file in &files {
                println!("    {}", style(&file.relative_path).dim());
            }
            println!("\nNext: {} && {}", style(format!("cd {}", directory.display())).cyan(), style("waffler pack").cyan());
            Ok(())
        }

        Commands::Build(a) => {
            let report = project_portal::build(&a.path)?;
            print!("{}", report.output);
            if !report.succeeded {
                anyhow::bail!("the build failed");
            }
            println!(
                "{} built {} ({} profile)",
                style("✓").green().bold(),
                report.crate_manifest.as_deref().unwrap_or("the project"),
                report.profile
            );
            Ok(())
        }

        Commands::Validate(a) => {
            let violations = project_portal::validate(&a.path)?;
            if violations.is_empty() {
                println!("{} the manifest is sound", style("✓").green().bold());
                return Ok(());
            }
            // EVERY PROBLEM AT ONCE. Fixing a manifest one refused field per run is how a five-minute
            // correction becomes an afternoon.
            eprintln!("{} {}:", style("✗").red().bold(), plural(violations.len(), "problem"));
            for v in &violations {
                eprintln!("    {} {}", style(&v.field).yellow(), v.problem);
            }
            anyhow::bail!("the manifest has {}", plural(violations.len(), "problem"));
        }

        Commands::Pack(a) => {
            let (bundle, report) = publish_portal::pack(&a.path, a.output.as_deref(), a.no_build)?;
            // WHETHER IT BUILT OR REUSED IS ALWAYS SAID. "It packed the wrong binary" and "it packed a
            // binary it did not build" are the same incident a day apart, and only one is discoverable
            // after the fact.
            if report.ran {
                println!("  built {} ({})", report.crate_manifest.as_deref().unwrap_or("the project"), report.profile);
            } else {
                println!("  {} reused existing artifacts; nothing was built", style("note:").yellow());
            }
            println!(
                "{} {} {}@{} ({} bytes, {})",
                style("✓").green().bold(),
                style(bundle.path.display()).white().bold(),
                bundle.fqid,
                bundle.version,
                bundle.size_bytes,
                bundle.framing
            );
            Ok(())
        }

        Commands::Publish(a) => {
            let client = waffler_cli::http_client();
            let session = session_portal::hydrate()?;
            let outcome = publish_portal::publish(
                &client,
                &session,
                &a.path,
                a.bundle.as_deref(),
                a.registry.as_deref(),
                a.no_build,
            )
            .await?;
            println!(
                "{} published {}@{} to {}",
                style("✓").green().bold(),
                style(&outcome.fqid).cyan().bold(),
                outcome.version,
                style(&outcome.registry).dim()
            );
            println!("  the bundle is at {}", style(outcome.bundle_path.display()).dim());
            Ok(())
        }

        Commands::Unpublish(a) => {
            let client = waffler_cli::http_client();
            let session = session_portal::hydrate()?;
            let registry = publish_portal::unpublish(&client, &session, &a.fqid, &a.version, a.registry.as_deref()).await?;
            println!(
                "{} withdrew {}@{} from {}",
                style("✓").green().bold(),
                a.fqid,
                a.version,
                style(&registry.base_url).dim()
            );
            // STATED, so nobody relies on a deletion that did not happen. Another version may
            // reference the same bytes, which is why the registry keeps them.
            println!("  the catalog entry is gone; the stored bytes are not — another version may reference them");
            Ok(())
        }

        Commands::Login(a) => {
            let client = waffler_cli::http_client();
            let session = session_portal::hydrate()?;
            let credential = session_portal::login(&client, &session, a.registry.as_deref()).await?;
            println!(
                "{} signed in to {} as {}",
                style("✓").green().bold(),
                style(&credential.registry).dim(),
                style(credential.username.as_deref().unwrap_or(&credential.subject)).cyan().bold()
            );
            Ok(())
        }

        Commands::Logout(a) => {
            let session = session_portal::hydrate()?;
            let (registry, existed) = session_portal::logout(&session, a.registry.as_deref())?;
            if existed {
                println!("{} signed out of {}", style("✓").green().bold(), registry.base_url);
            } else {
                // Not an error — signing out twice is not a mistake — but said plainly, because
                // "signed out" for a session that was not there reads as a claim about state.
                println!("{} there was no session for {}", style("·").dim(), registry.base_url);
            }
            Ok(())
        }

        Commands::Whoami(a) => {
            let session = session_portal::hydrate()?;
            let (registry, credential) = session_portal::whoami(&session, a.registry.as_deref())?;
            match credential {
                // PER REGISTRY, AND THE REGISTRY IS NAMED. Asking who I am without naming one is a
                // question with as many answers as there are sessions.
                Some(c) => println!(
                    "{} on {} ({})",
                    style(c.username.as_deref().unwrap_or(&c.subject)).cyan().bold(),
                    style(&registry.base_url).dim(),
                    if c.is_fresh(chrono::Duration::zero()) { "session valid" } else { "session expired" }
                ),
                None => println!("not signed in to {}", registry.base_url),
            }
            Ok(())
        }

        Commands::Use(a) => {
            let session = session_portal::hydrate()?;
            let (registry, signed_in) = session_portal::use_registry(&session, &a.url)?;
            println!("{} default registry is now {}", style("✓").green().bold(), style(&registry.base_url).cyan());
            if !signed_in {
                // A `use` to an unauthenticated registry otherwise looks like a working session until
                // the first publish.
                println!("  {} you are not signed in to it — run `waffler login`", style("note:").yellow());
            }
            Ok(())
        }

        Commands::Registry(a) => {
            let client = waffler_cli::http_client();
            let session = session_portal::hydrate()?;
            let registry = session_portal::resolve(&session, a.registry.as_deref())?;
            println!("{} ({})", style(&registry.base_url).cyan().bold(), registry.source.because());
            let profile = session_portal::profile(&client, &registry).await?;
            println!("  publishing:     {}", yes_no(profile.publishing_available));
            println!("  authentication: {}", yes_no(profile.authentication_available));
            match profile.discovery_url.as_deref() {
                Some(url) => {
                    println!("  sign in via:    {url}");
                    println!("  audience:       {}", profile.audience.as_deref().unwrap_or("(not advertised)"));
                    // A CLIENT ID IS OPTIONAL WHERE THE OTHER TWO ARE NOT, so an absent one is
                    // reported as the fallback that will be used rather than as a gap.
                    println!(
                        "  client id:      {}",
                        profile.client_id.as_deref().unwrap_or("(not advertised — falling back to waffler-cli)")
                    );
                }
                // NAMED AS MISSING rather than omitted. A registry that authenticates and does not say
                // where is one `waffler login` cannot serve, and the operator needs to know that is
                // the registry's gap rather than the tool's.
                None if profile.authentication_available => println!(
                    "  sign in via:    {} — this registry does not advertise an OpenID discovery document, so `waffler login` cannot run against it",
                    style("not advertised").yellow()
                ),
                None => {}
            }
            println!("  max upload:     {} bytes", profile.max_package_size_bytes);
            for identity in &profile.signing_identities {
                println!(
                    "  signs with:     {} ({})",
                    identity.get("label").and_then(|v| v.as_str()).unwrap_or("?"),
                    identity.get("algorithm").and_then(|v| v.as_str()).unwrap_or("?")
                );
            }
            Ok(())
        }
    }
}

fn plural(n: usize, noun: &str) -> String {
    format!("{n} {noun}{}", if n == 1 { "" } else { "s" })
}

fn yes_no(v: bool) -> &'static str {
    if v {
        "yes"
    } else {
        "no"
    }
}
