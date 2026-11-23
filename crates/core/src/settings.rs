//! Per-user preferences: the check a locale passes before it is saved, the
//! browser's report and the one a program sets for commands alike, and how
//! a patch merges into what is saved.


use jush_command_protocol::CommandLocale;
use jush_web_api_protocol::settings::{
    BrowserLocale,
    ContextLimitChange, ContextLimits, Preferences, PreferencesPatch,
};
use icu_locale::{Locale, LocaleCanonicalizer};
use icu_time::zone::iana::IanaParserExtended;

/// A patch whose locale, when it has one, names a time zone the backend
/// knows, in its IANA spelling, and holds each language once as its
/// canonical BCP 47 tag, in the order the user's browser reported them.
#[derive(Debug)]
pub struct CheckedPatch(PreferencesPatch);

/// Why a reported locale is refused. Each names the field, as a refused body
/// does.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LocaleError {
    #[error("{field}.timeZone: {zone:?} is not a time zone the backend knows")]
    UnknownTimeZone { field: &'static str, zone: String },
    #[error("{field}.languages[{index}]: {tag:?} is not a BCP 47 language tag")]
    MalformedTag {
        field: &'static str,
        index: usize,
        tag: String,
    },
}

pub fn check(mut patch: PreferencesPatch) -> Result<CheckedPatch, LocaleError> {
    if let Some(locale) = patch.locale.take() {
        let (time_zone, languages) = canonical("locale", &locale.time_zone, &locale.languages)?;
        patch.locale = Some(BrowserLocale {
            time_zone,
            languages,
            language: locale.language,
        });
    }
    patch.command_locale = match patch.command_locale.take() {
        Some(Some(locale)) => {
            let (time_zone, languages) = canonical("commandLocale", &locale.time_zone, &locale.languages)?;
            Some(Some(CommandLocale { time_zone, languages }))
        }
        unchanged => unchanged,
    };
    Ok(CheckedPatch(patch))
}

/// A locale's time zone checked against ICU4X's IANA names, which ignore
/// case, and its tags canonicalized as a web browser's
/// `Intl.getCanonicalLocales` does; a refusal names `field`.
///
/// For example, `zh-cn` becomes `zh-CN`, `iw` becomes `he`, and a tag that
/// repeats an earlier one goes.
fn canonical(field: &'static str, time_zone: &str, tags: &[String]) -> Result<(String, Vec<String>), LocaleError> {
    let zone = IanaParserExtended::new().parse(time_zone);
    if zone.time_zone.is_unknown() {
        return Err(LocaleError::UnknownTimeZone {
            field,
            zone: time_zone.to_owned(),
        });
    }
    let canonicalizer = LocaleCanonicalizer::new_extended();
    let mut languages: Vec<String> = Vec::with_capacity(tags.len());
    for (index, tag) in tags.iter().enumerate() {
        let mut parsed = Locale::try_from_str(tag).map_err(|_| LocaleError::MalformedTag {
            field,
            index,
            tag: tag.clone(),
        })?;
        canonicalizer.canonicalize(&mut parsed);
        let canonical = parsed.to_string();
        if !languages.contains(&canonical) {
            languages.push(canonical);
        }
    }
    Ok((zone.normalized.to_owned(), languages))
}

/// `preferences` with `patch` applied:
///
/// - A field the patch holds replaces the saved one. The lists of hidden
///   entries and models replace it whole.
/// - A `null` shortcut removes that override; a set's change leaves the other set as it is.
/// - A `null` small model goes back to Automatic, and a `null` command
///   locale to the browser's report.
/// - A context limit changes that one model's.
/// - Everything the patch leaves out stays.
pub fn merge(mut preferences: Preferences, CheckedPatch(patch): CheckedPatch) -> Preferences {
    if let Some(appearance) = patch.appearance {
        let saved = &mut preferences.appearance;
        saved.theme = appearance.theme.or(saved.theme);
        saved.tone = appearance.tone.or(saved.tone);
        saved.accent = appearance.accent.or(saved.accent);
        saved.font_size = appearance.font_size.or(saved.font_size);
    }
    if let Some(sets) = patch.shortcuts {
        for (change, saved) in [
            (sets.mac, &mut preferences.shortcuts.mac),
            (sets.pc, &mut preferences.shortcuts.pc),
        ] {
            let Some(change) = change else { continue };
            for (change, override_) in [
                (change.new, &mut saved.new),
                (change.sidebar, &mut saved.sidebar),
                (change.settings, &mut saved.settings),
                (change.search, &mut saved.search),
                (change.search_files, &mut saved.search_files),
                (change.find, &mut saved.find),
                (change.workspace1, &mut saved.workspace1),
                (change.workspace2, &mut saved.workspace2),
                (change.workspace3, &mut saved.workspace3),
                (change.workspace4, &mut saved.workspace4),
                (change.workspace5, &mut saved.workspace5),
                (change.workspace6, &mut saved.workspace6),
                (change.workspace7, &mut saved.workspace7),
                (change.workspace8, &mut saved.workspace8),
                (change.workspace9, &mut saved.workspace9),
            ] {
                if let Some(keys) = change {
                    *override_ = keys;
                }
            }
        }
    }
    preferences.last_model = patch.last_model.or(preferences.last_model);
    preferences.last_workflow = patch.last_workflow.or(preferences.last_workflow);
    preferences.last_project_device = patch.last_project_device.or(preferences.last_project_device);
    preferences.language = patch.language.or(preferences.language);
    preferences.locale = patch.locale.or(preferences.locale);
    if let Some(command_locale) = patch.command_locale {
        preferences.command_locale = command_locale;
    }
    preferences.color_scheme = patch.color_scheme.or(preferences.color_scheme);
    preferences.send_while_running = patch.send_while_running.or(preferences.send_while_running);
    preferences.resume_at_retry = patch.resume_at_retry.or(preferences.resume_at_retry);
    preferences.resume_after_restart = patch.resume_after_restart.or(preferences.resume_after_restart);
    preferences.compact_idle = patch.compact_idle.or(preferences.compact_idle);
    preferences.idle_compaction_lifetime = patch
        .idle_compaction_lifetime
        .or(preferences.idle_compaction_lifetime);
    preferences.idle_compaction_context = patch
        .idle_compaction_context
        .or(preferences.idle_compaction_context);
    preferences.show_unexpected_errors = patch.show_unexpected_errors.or(preferences.show_unexpected_errors);
    preferences.multi_account_warned = patch.multi_account_warned.or(preferences.multi_account_warned);
    if let Some(hidden) = patch.hidden_providers {
        preferences.hidden_providers = hidden;
    }
    if let Some(hidden) = patch.hidden_models {
        preferences.hidden_models = hidden;
    }
    if let Some(small_model) = patch.small_model {
        preferences.small_model = small_model;
    }
    preferences.small_model_same_provider = patch
        .small_model_same_provider
        .or(preferences.small_model_same_provider);
    preferences.suggested_replies = patch.suggested_replies.or(preferences.suggested_replies);
    preferences.output_speed = patch.output_speed.or(preferences.output_speed);
    preferences.taskbar_pins = patch.taskbar_pins.or(preferences.taskbar_pins);
    preferences.file_tree = patch.file_tree.or(preferences.file_tree);
    if let Some(change) = patch.context_limit {
        set_context_limit(&mut preferences.context_limits, change);
    }
    preferences
}

/// Stores the limit `change` names for its model, or removes the model's
/// limit, and with it its entry's map once that is empty, so a model the
/// user returned to its full window has no stored limit.
fn set_context_limit(limits: &mut ContextLimits, change: ContextLimitChange) {
    let ContextLimitChange {
        provider_id,
        model_id,
        tokens,
    } = change;
    match tokens {
        Some(tokens) => {
            limits
                .entry(provider_id)
                .or_default()
                .insert(model_id, tokens);
        }
        None => {
            let Some(models) = limits.get_mut(&provider_id) else {
                return;
            };
            models.remove(&model_id);
            if models.is_empty() {
                limits.remove(&provider_id);
            }
        }
    }
}
