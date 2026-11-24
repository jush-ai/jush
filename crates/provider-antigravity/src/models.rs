//! The Antigravity catalog (`crates/provider-registry/src/catalog.rs`): the
//! models a new session of the account offers (`probe.rs`), each once with
//! the thinking levels `agy` gives it, and its context window and prices from
//! models.dev.
//!
//! `agy` lists each model at each of its levels, such as `gemini-4-neon-high`
//! "Gemini 4 Neon (High)", and names every level in its `thought_level`
//! option. A model is the id without its level, `gemini-4-neon`, and a run
//! asks `agy` for the id with the level the conversation chose.
//!
//! A model's facts are those of the listing models.dev's search gives it: its
//! maker's, such as Google's for Gemini, where the maker lists it. A model no
//! listing knows, such as one of `agy`'s orchestrators, which no vendor sells,
//! is left out: it has no context window and no price to show.
//!
//! Its context window is no larger than the limit at which `agy` compacts its
//! conversation by itself ([`AGY_LIMITS`]), so the agent compacts before `agy`
//! would, and the conversation has one history, the agent's.

use jush_provider_common::CatalogError;
use jush_provider_common::models_dev::{ModelsDevMatch, ModelsDevSnapshot};
use jush_shared_types::{FileExtension, ProviderModel, ProviderModelList, ThinkingForm};

use crate::acp::NewSession;

/// The session setting that names the thinking levels.
const THOUGHT_LEVEL: &str = "thought_level";

/// The tokens at which `agy` 1.5.16 compacts a conversation of a model whose
/// id starts so, before sending a request that would reach them; any other
/// model's are [`AGY_DEFAULT_LIMIT`].
///
/// `agy` compiles them into its default configuration, which Google's backend
/// may override; nothing `agy` offers names them, and Jush never calls that
/// backend with the account's tokens. Jush runs Google's newest `agy`, so a
/// later release may move one, and a run logs every compaction `agy` makes by
/// itself (`run.rs`).
const AGY_LIMITS: [(&str, u32); 5] = [
    ("gemini-3.8-flash", 256_000),
    ("gemini-4-", 512_000),
    ("gemini-pro-orchestrator", 512_000),
    ("claude-", 840_000),
    ("gpt-oss-", 80_000),
];

/// The tokens at which `agy` compacts a conversation of a model its
/// configuration gives no limit: the default Google chose (agy's
/// `GetCheckpointConfig`).
const AGY_DEFAULT_LIMIT: u32 = 256_000;

/// One model `agy` offers, at the levels it offers it.
pub(crate) struct Offered {
    /// Such as `gemini-4-neon`, the catalog's id of it.
    pub(crate) id: String,
    /// Such as `Gemini 4 Neon`.
    pub(crate) name: String,
    levels: Vec<String>,
}

/// The catalog of the models `session` offers, with `snapshot`'s facts.
pub(crate) fn catalog(
    session: &NewSession,
    snapshot: &ModelsDevSnapshot,
) -> Result<ProviderModelList, CatalogError> {
    let Some(offer) = &session.models else {
        return Err(CatalogError::Invalid(
            "Antigravity's new session lists no models".into(),
        ));
    };
    let levels = levels(session);
    let models: Vec<ProviderModel> = offered(session)
        .into_iter()
        .filter_map(|model| {
            let listing = listing(snapshot, &model.id)?;
            Some(catalog_model(model, listing, &levels))
        })
        .collect();
    let default_model_id = offer
        .current_model_id
        .as_deref()
        .map(|current| split(current, &levels).0)
        .filter(|current| models.iter().any(|model| model.id == *current))
        .map(str::to_owned);
    Ok(ProviderModelList {
        models,
        default_model_id,
        warnings: snapshot.warnings.clone(),
        source_fetched_at: snapshot.fetched_at,
        stale: snapshot.stale,
    })
}

/// The models `session` offers, each once.
pub(crate) fn offered(session: &NewSession) -> Vec<Offered> {
    let levels = levels(session);
    let mut offered: Vec<Offered> = Vec::new();
    for model in session.models.iter().flat_map(|offer| &offer.available_models) {
        let (id, level) = split(&model.model_id, &levels);
        match offered.iter_mut().find(|known| known.id == id) {
            Some(known) => known.levels.extend(level.map(str::to_owned)),
            None => offered.push(Offered {
                id: id.to_owned(),
                name: name(&model.name, level.is_some()),
                levels: level.map(str::to_owned).into_iter().collect(),
            }),
        }
    }
    offered
}

/// The thinking levels `session` names, in `agy`'s order.
fn levels(session: &NewSession) -> Vec<&str> {
    session
        .config_options
        .iter()
        .find(|option| option.id == THOUGHT_LEVEL)
        .map(|option| option.options.iter().map(|value| value.value.as_str()).collect())
        .unwrap_or_default()
}

/// `agy`'s model `id` without its level, and the level, one of `levels`.
fn split<'a>(id: &'a str, levels: &[&'a str]) -> (&'a str, Option<&'a str>) {
    levels
        .iter()
        .find_map(|level| {
            let model = id.strip_suffix(level)?.strip_suffix('-')?;
            Some((model, Some(*level)))
        })
        .unwrap_or((id, None))
}

/// The name of a model `agy` names `named` at one of its levels, when
/// `leveled`: without the level, `Gemini 4 Neon (High)` named
/// `Gemini 4 Neon`.
fn name(named: &str, leveled: bool) -> String {
    let unleveled = leveled
        .then(|| named.rsplit_once(" (").filter(|(_, level)| level.ends_with(')')))
        .flatten();
    unleveled.map_or(named, |(name, _)| name).to_owned()
}

/// The listing models.dev's search gives the model `id`: the one of that id
/// without a vendor's prefix.
fn listing(snapshot: &ModelsDevSnapshot, id: &str) -> Option<ModelsDevMatch> {
    snapshot.search(id, None).into_iter().find(|found| {
        let bare = found.id.rsplit('/').next().unwrap_or(&found.id);
        bare.eq_ignore_ascii_case(id)
    })
}

/// The catalog's model `offered`, with `listing`'s facts; `levels` are all
/// of `agy`'s, in its order.
fn catalog_model(offered: Offered, listing: ModelsDevMatch, levels: &[&str]) -> ProviderModel {
    let efforts: Vec<String> = levels
        .iter()
        .filter(|level| offered.levels.iter().any(|offered| offered == *level))
        .map(|level| (*level).to_owned())
        .collect();
    let context_window = within_agy_limit(&offered.id, listing.model.context_window);
    ProviderModel {
        id: offered.id,
        display_name: offered.name,
        supports_tools: Some(true),
        // Its model reads any other medium as a file (`media.rs`), but its
        // `read_file` reads no FLAC or AIFF (measured).
        accepted_extensions: listing.model.accepted_extensions.map(|extensions| {
            extensions
                .into_iter()
                .filter(|extension| !matches!(extension, FileExtension::Flac | FileExtension::Aiff))
                .collect()
        }),
        supported_thinking_efforts: (!efforts.is_empty()).then_some(efforts),
        // `agy` only levels thinking; it cannot turn it off.
        can_disable_thinking: Some(false),
        service_tiers: Vec::new(),
        default_service_tier_id: None,
        // Only an API's request reads either.
        thinking_form: ThinkingForm::default(),
        interleaved_thinking: false,
        context_window,
        ..listing.model
    }
}

/// `window`, the context window models.dev lists for `agy`'s model `id`, no
/// larger than the limit at which `agy` compacts by itself.
fn within_agy_limit(id: &str, window: Option<u32>) -> Option<u32> {
    let limit = agy_limit(id);
    Some(window.map_or(limit, |window| window.min(limit)))
}

/// The tokens at which `agy` compacts a conversation of its model `id`.
pub(crate) fn agy_limit(id: &str) -> u32 {
    AGY_LIMITS
        .iter()
        .find(|(prefix, _)| id.starts_with(prefix))
        .map_or(AGY_DEFAULT_LIMIT, |(_, limit)| *limit)
}
