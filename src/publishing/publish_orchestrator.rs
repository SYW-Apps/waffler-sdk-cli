//! `sdk_cli::publish-orchestrator` — order, and nothing else.
//!
//! Spec: `sdk_cli::publish-orchestrator` / `ipublish-orchestrator` / `publish_orchestrator_impl`.
//!
//! ## THE SEQUENCE, AND THE ORDER IS LOAD-BEARING
//!
//!   1. resolve the registry and REPORT it — before anything irreversible;
//!   2. ask the registry what it accepts and whether it can publish at all;
//!   3. write the bundle from its parts (or read the identity of a pre-built one);
//!   4. check what follows the archive, and refuse an already-signed artifact;
//!   5. check the size against what the registry said;
//!   6. ask what the registry already holds for this fqid;
//!   7. decide whether this publish shape is legal;
//!   8. obtain a bearer — refreshed against THIS moment, after the build;
//!   9. upload;
//!  10. report what the registry answered.
//!
//! ASKING BEFORE SIGNING IS WHAT MAKES THE DOWNGRADE REFUSAL POSSIBLE. Discovering after signing that
//! an unsigned publish was illegal would mean either uploading something that will be refused or
//! silently changing shape.
//!
//! FAILURE LEAVES NOTHING HALF-DONE LOCALLY, AND SAYS WHERE IT STOPPED. A publish that fails at upload
//! has still produced a valid bundle on disk, and saying so is worth more than cleaning it up: the
//! developer can inspect it, and the next attempt does not rebuild.

use std::path::{Path, PathBuf};

use anyhow::{bail, Result};

use super::types::{PublishOutcome, WrittenBundle};
use super::{bundle_writer, project_client_adapter, publication_adapter, session_client_adapter, signature_specialist};
use crate::project::types::BuildReport;
use crate::session::types::{PersistedSession, TargetRegistry};

/// Produce a bundle and stop.
///
/// NO REGISTRY IS RESOLVED AND NO CREDENTIAL IS ASKED FOR on this path. Producing something
/// inspectable without publishing it is how a developer checks their own work and how the container
/// image build consumes this tool — that build has no network and no session, and a pack that reached
/// for either would fail there for a reason that has nothing to do with packing.
pub fn pack(directory: &Path, output_path: Option<&Path>, skip_build: bool) -> Result<(WrittenBundle, BuildReport)> {
    let (plan, report) = project_client_adapter::plan_from_directory(directory, skip_build)?;
    let output = match output_path {
        Some(p) => p.to_path_buf(),
        None => crate::project::project_adapter::default_bundle_path(directory, &plan.fqid),
    };
    let written = bundle_writer::write_bundle(&plan, &output)?;
    Ok((written, report))
}

/// Pack if needed, then upload.
pub async fn publish(
    client: &reqwest::Client,
    session: &PersistedSession,
    directory: &Path,
    bundle_path: Option<&Path>,
    registry_flag: Option<&str>,
    skip_build: bool,
) -> Result<PublishOutcome> {
    let registry = session_client_adapter::target(session, registry_flag)?;
    // REPORTED NOW, BEFORE ANYTHING IRREVERSIBLE. The tool knows the answer at the moment it is
    // cheapest to say, and a publish that quietly went to the wrong registry is the failure an
    // operator discovers last and trusts least.
    println!("Publishing to {} ({})", registry.base_url, registry.source.because());

    let profile = session_client_adapter::describe(client, &registry).await?;
    if !profile.publishing_available {
        // SAID BEFORE A BUILD rather than after, because it is knowable from one request.
        bail!(
            "{} serves downloads and signs nothing, so it cannot accept a publish.",
            registry.base_url
        );
    }

    let bundle = match bundle_path {
        // A PRE-BUILT BUNDLE IS CARRIED THROUGH AS OPAQUE BYTES with its identity read from ITSELF.
        // Re-reading a project manifest to describe an archive that already exists is how the two come
        // to disagree — and the archive is the thing being uploaded.
        Some(path) => bundle_writer::read_identity(path)?,
        None => {
            let (plan, _report) = project_client_adapter::plan_from_directory(directory, skip_build)?;
            let output = crate::project::project_adapter::default_bundle_path(directory, &plan.fqid);
            bundle_writer::write_bundle(&plan, &output)?
        }
    };

    // REFUSED BEFORE UPLOADING. The registry refuses one too — it cannot safely decide which trailing
    // bytes are signature and which are content — and finding that out locally costs nothing while
    // finding it out after the upload costs the upload.
    signature_specialist::refuse_if_signed(&bundle.path)?;

    if profile.max_package_size_bytes > 0 && bundle.size_bytes > profile.max_package_size_bytes {
        // The bundle on disk is KEPT: the developer can inspect it, and the next attempt does not
        // rebuild.
        bail!(
            "{} is {} bytes and {} accepts at most {}.\n  The bundle is left at {} — nothing was \
             uploaded.",
            bundle.path.display(),
            bundle.size_bytes,
            registry.base_url,
            profile.max_package_size_bytes,
            bundle.path.display()
        );
    }

    let held = publication_adapter::fetch_published_package(client, &registry.base_url, &bundle.fqid).await?;
    if let Some(held) = held.as_ref() {
        if held.has_publisher_signature() {
            // REFUSED, WITH NO FLAG TO WAIVE IT.
            //
            // A package that has ever carried a publisher signature may never publish one without:
            // accepting it would let anyone who obtains publish rights strip the binding to whoever
            // built the software, and every node afterwards would verify a registry signature and find
            // nothing missing.
            //
            // This tool cannot yet PRODUCE a publisher signature — see `signature_specialist` for why
            // the producer half is deliberately absent — so reaching this branch means the package was
            // signed by something else and this tool must not be the way that gets undone.
            bail!(
                "{} already has publisher-signed versions on {}, and an unsigned publish would strip \
                 that binding.\n  This tool does not produce publisher signatures yet, so it cannot \
                 publish to this package without downgrading it. Publish with the tool that signed the \
                 existing versions.",
                bundle.fqid,
                registry.base_url
            );
        }
    }

    // THE BEARER IS FETCHED HERE, after the build and before the upload. A token checked before a
    // release build and used after it can expire in between, and the wasted upload is the whole cost
    // of getting that order wrong.
    let bearer = session_client_adapter::bearer(client, session, &registry).await?;

    publication_adapter::upload_bundle(client, &registry.base_url, &bundle.fqid, &bundle.path, &bearer).await
}

/// Withdraw a published version.
///
/// IT EXISTS BECAUSE THE CYCLE NEEDS IT. Publish, install, uninstall, withdraw, republish is the loop a
/// developer actually runs while getting a package right, and a tool that can only add versions makes
/// every mistake permanent — which pushes people to bump the version to escape a bad publish, and a
/// version number that means "the last one was wrong" is a version number that means nothing.
pub async fn unpublish(
    client: &reqwest::Client,
    session: &PersistedSession,
    fqid: &str,
    version: &str,
    registry_flag: Option<&str>,
) -> Result<TargetRegistry> {
    let registry = session_client_adapter::target(session, registry_flag)?;
    // Reported first, like a publish. Withdrawing from the wrong registry is worse than publishing to
    // one, because the damage is to something that already worked.
    println!(
        "Withdrawing {fqid}@{version} from {} ({})",
        registry.base_url,
        registry.source.because()
    );

    let bearer = session_client_adapter::bearer(client, session, &registry).await?;
    publication_adapter::withdraw_version(client, &registry.base_url, fqid, version, &bearer).await?;
    Ok(registry)
}

/// Where a bundle lands by default, for a caller that wants to report it before packing.
pub fn default_output(directory: &Path, fqid: &str) -> PathBuf {
    crate::project::project_adapter::default_bundle_path(directory, fqid)
}
