//! A process provider's command-line tool
//! (`crates/backend-user-shard/src/conversation/subscription_cli.rs`).
//!
//! The routes:
//!
//! - Read the vendor's newest version, without waking the Subscriptions
//!   machine.
//! - Install again; the install's state reaches the pages with the entry's.
//!
//! A sign-in installs the tool it runs itself.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use jush_provider_registry::entry::ProviderEntry;
use jush_provider_subscription_cli::Cli;
use jush_web_api_protocol::auth::UserDto;
use jush_web_api_protocol::providers::{NewestVersion, ProviderCli};
use jush_web_api_protocol::query::Refresh;

use super::error::ApiError;
use super::gate::AuthUser;
use super::providers::scoped;
use super::query::QueryParams;
use jush_backend_user_shard::conversation::subscription_cli::start_install;
use jush_backend_user_shard::services::Services;
use jush_backend_user_shard::shard::Shards;

/// The entry the path names, with the CLI its provider runs as a process; an
/// entry whose provider runs none has no tool to read or install.
async fn process_entry(
    services: &Services,
    user: &UserDto,
    id: &str,
) -> Result<(ProviderEntry, &'static Cli), ApiError> {
    let entry = scoped(services, user, id).await?;
    match services.provider_registry.cli(&entry) {
        Some(cli) => Ok((entry, cli)),
        None => Err(ApiError::provider_not_found()),
    }
}

/// The entry's tool: `refresh=true` reads the vendor's newest version at
/// once.
pub(super) async fn read(
    State(services): State<Arc<Services>>,
    AuthUser(user): AuthUser,
    Path(id): Path<String>,
    QueryParams(Refresh { refresh }): QueryParams<Refresh>,
) -> Result<Json<ProviderCli>, ApiError> {
    let (_, cli) = process_entry(&services, &user, &id).await?;
    let newest = match services.cli_releases.of(cli) {
        Some(releases) => match releases.latest(refresh.0).await {
            Ok(release) => NewestVersion::Read {
                version: release.version,
            },
            Err(error) => NewestVersion::Unreadable {
                message: error.to_string(),
            },
        },
        None => NewestVersion::Unreadable {
            message: format!("This backend reads none of {}'s releases", cli.name),
        },
    };
    Ok(Json(ProviderCli { newest }))
}

/// Starts the install on the Subscriptions machine again.
pub(super) async fn install(
    State(services): State<Arc<Services>>,
    State(shards): State<Shards>,
    AuthUser(user): AuthUser,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let (entry, cli) = process_entry(&services, &user, &id).await?;
    start_install(&services, &shards, &user.id, &entry.id, cli).await;
    Ok(StatusCode::ACCEPTED)
}
