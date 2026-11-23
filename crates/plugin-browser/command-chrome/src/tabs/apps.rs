//! A tab's requests to open a link in an app: each link to an app a frame of
//! its page goes to, which waits for the user, as a desktop browser's dialog
//! asks before it starts the app (`driver::apps`).
//!
//! # What Chrome does with a request
//!
//! Chrome drops the navigation itself, then decides, as its
//! `external_protocol_handler.cc` does, and says what it decided in the
//! console of the page that asked, except when it asks:
//!
//! ```text
//! the page asks -> refused     "Not allowed to launch …"
//!                  (asked again without a click of the user's, or a
//!                   scheme Chrome never hands on)
//!               -> no app      "Failed to launch …"
//!               -> started     "Launched external handler for …"
//!                  (`mailto:`, which Chrome hands on without asking)
//!               -> its dialog  nothing
//! ```
//!
//! A Chrome without a window never shows its dialog, and while the dialog is
//! open the tab takes no input: no click or key of the user's or the agent's
//! reaches the page, and no command can close it. Chrome closes it only when
//! the tab's top-level page commits a document of another site. So a request
//! Chrome said nothing of holds the tab: Jush reads what Chrome said rather
//! than guess what it would decide, which differs by system and by what the
//! page did before.
//!
//! # What Jush does
//!
//! A request Chrome refused is dropped, as Chrome shows nothing for it.
//! Every other one is asked, one at a time in the order the page asked, as
//! Chrome queues its own dialogs: a newer request waits behind the one on
//! screen, which never changes under the user's hand. An answer to a request
//! while one holds the tab releases it (`release`). A top-level page of
//! another site ends every request, as it closes Chrome's dialogs; another
//! page of the same site ends those that hold nothing.

use std::{collections::VecDeque, net::IpAddr, time::Duration};

use chromiumoxide::Page;
use chromiumoxide::cdp::browser_protocol::{
    log::{EnableParams as LogEnableParams, EventEntryAdded},
    network::{EventRequestWillBeSent, ResourceType},
    page::{
        EventFrameNavigated, GetFrameTreeParams, GetNavigationHistoryParams, NavigateParams,
        NavigateToHistoryEntryParams,
    },
};
use futures_util::StreamExt;
use tokio::{
    sync::{mpsc, watch},
    time::Instant,
};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use crate::driver::{
    apps,
    operation::{BrowserError, CONTROL_TIMEOUT, Result},
};
use crate::tabs::protocol::AppRequest;
use crate::tabs::tab::BrowserTab;
use crate::traced::SpawnTraced as _;

/// How long a request waits before it is asked. Chrome refuses a request as
/// it receives it, a few milliseconds after the page asked, and a refused
/// request is never asked; the wait keeps one from flashing up meanwhile.
const REFUSAL_WAIT: Duration = Duration::from_millis(250);

/// A request, from the page's asking to the user's answer.
#[derive(Clone)]
struct Entry {
    id: u64,
    url: String,
    scheme: String,
    /// The app the device opens the link with, once the system answered.
    app: Option<Option<String>>,
    /// Whether Chrome had its time to refuse the request (`REFUSAL_WAIT`).
    unrefused: bool,
    /// Whether Chrome said nothing of the request: its dialog holds the tab,
    /// or will once Chrome found the app's name.
    silent: bool,
}

impl Entry {
    fn ready(&self) -> bool {
        self.unrefused && self.app.is_some()
    }
}

/// A tab's requests to open a link in an app, in the order the page asked.
#[derive(Clone, Default)]
pub struct Requests {
    entries: VecDeque<Entry>,
    /// Whether an answer releases the tab from Chrome's dialog now.
    releasing: bool,
}

impl Requests {
    /// The request the user is asked now: the oldest, once it is ready.
    pub fn asked(&self) -> Option<AppRequest> {
        let entry = self.entries.front().filter(|entry| entry.ready())?;
        Some(AppRequest {
            url: entry.url.clone(),
            app: entry.app.clone().flatten(),
        })
    }

    fn holds_input(&self) -> bool {
        self.releasing || self.entries.iter().any(|entry| entry.unrefused && entry.silent)
    }
}

/// A tab's requests to open a link in an app.
pub struct AppRequests {
    requests: watch::Sender<Requests>,
}

impl AppRequests {
    /// The request the user is asked now, if one is.
    pub fn pending(&self) -> Option<AppRequest> {
        self.requests.borrow().asked()
    }

    pub fn watch(&self) -> watch::Receiver<Requests> {
        self.requests.subscribe()
    }

    /// Whether Chrome's own dialog holds the tab's input until the user
    /// answers, or an answer releases the tab now.
    pub fn holds_input(&self) -> bool {
        self.requests.borrow().holds_input()
    }

    /// Ends the request for `url`, which the user answered: `None` unless it
    /// is the one asked, otherwise whether the tab must be released
    /// (`release`), which holds its input until `released`.
    pub fn answer(&self, url: &str) -> Option<bool> {
        let mut answered = None;
        self.requests.send_if_modified(|requests| {
            if requests.asked().is_none_or(|asked| asked.url != url) {
                return false;
            }
            let holds = requests.entries.iter().any(|entry| entry.silent);
            requests.entries.pop_front();
            requests.releasing = holds;
            answered = Some(holds);
            true
        });
        answered
    }

    /// Ends the hold of an answer, once the tab is released or could not be.
    pub fn released(&self) {
        self.requests
            .send_if_modified(|requests| std::mem::take(&mut requests.releasing));
    }
}

/// What Chrome said of a request in the console of the page that asked.
#[derive(Clone, Copy)]
enum Said {
    Refused,
    Decided,
}

/// Applies what Chrome said of a request of `scheme` to `entries`, and says
/// whether a request took it.
fn apply(entries: &mut VecDeque<Entry>, said: Said, scheme: &str) -> bool {
    let of = |entry: &Entry| entry.silent && entry.scheme == scheme;
    match said {
        // Chrome refuses as it receives a request, so the refusal is the
        // newest request's.
        Said::Refused => entries.iter().rposition(of).map(|at| entries.remove(at)).is_some(),
        // Chrome decides in the order it received them.
        Said::Decided => entries
            .iter_mut()
            .find(|entry| of(entry))
            .map(|entry| entry.silent = false)
            .is_some(),
    }
}

/// What Chrome's console `text` says of a request, and of which scheme.
/// Chrome names the link's scheme, or the whole link in older versions.
fn chrome_said(text: &str) -> Option<(Said, String)> {
    let (said, rest) = [
        (Said::Refused, "Not allowed to launch '"),
        (Said::Decided, "Failed to launch '"),
        (Said::Decided, "Launched external handler for '"),
    ]
    .into_iter()
    .find_map(|(said, prefix)| text.strip_prefix(prefix).map(|rest| (said, rest)))?;
    let (scheme, _) = rest.split_once(':')?;
    Some((said, scheme.to_ascii_lowercase()))
}

fn scheme_of(url: &str) -> String {
    url.split_once(':')
        .map_or_else(String::new, |(scheme, _)| scheme.to_ascii_lowercase())
}

/// Whether Chrome keeps a tab's dialogs when its top-level page goes from
/// `from` to `to`, as its `WebContentsModalDialogManager` does: the two
/// share their host, or their domain under its public suffix.
fn same_site(from: &str, to: &str) -> bool {
    let host = |url: &str| url::Url::parse(url).ok()?.host_str().map(str::to_owned);
    let (Some(from), Some(to)) = (host(from), host(to)) else {
        return false;
    };
    from == to || registrable(&from).is_some_and(|domain| registrable(&to) == Some(domain))
}

/// The domain `host` belongs to under its public suffix; none for an
/// address.
fn registrable(host: &str) -> Option<&str> {
    if host.trim_matches(['[', ']']).parse::<IpAddr>().is_ok() {
        return None;
    }
    psl::domain_str(host)
}

/// Follows the requests of `page` to open a link in an app until `ended`.
///
/// A request is a document request of the page's own frames for a link to an
/// app, whether a frame went to it or a server redirected one there; a link
/// that opens another tab is that tab's.
pub(crate) async fn observe(page: &Page, ended: CancellationToken, tasks: &TaskTracker) -> Result<AppRequests> {
    let mut sent = page.event_listener::<EventRequestWillBeSent>().await?;
    let mut said = page.event_listener::<EventEntryAdded>().await?;
    let mut navigated = page.event_listener::<EventFrameNavigated>().await?;
    page.execute(LogEnableParams::default()).await?;
    let mut top = page.execute(GetFrameTreeParams {}).await?.result.frame_tree.frame.url;
    let requests = watch::channel(Requests::default()).0;
    let follows = requests.clone();
    let lookups = tasks.clone();
    tasks.spawn_traced(async move {
        let (found, mut answers) = mpsc::unbounded_channel::<(u64, Option<String>)>();
        let mut waits = VecDeque::<(Instant, u64)>::new();
        // Chrome's request and its console message reach Jush by two ways,
        // the browser's and the page's, so a message can come first: it
        // waits for its request as long as a request waits for a refusal.
        let mut early = VecDeque::<(Instant, Said, String)>::new();
        let mut next_id = 0;
        loop {
            let wait = waits.front().map(|(until, _)| *until);
            // A commit goes before the requests that arrived with it: a
            // request of the page it ends is seldom that late, and one of
            // the page it brings is often that early.
            tokio::select! {
                biased;
                _ = ended.cancelled() => break,
                event = navigated.next() => match event {
                    Some(Ok(event)) if event.frame.parent_id.is_none() => {
                        let to = event.frame.url.clone();
                        let kept = same_site(&top, &to);
                        top = to;
                        follows.send_if_modified(|requests| {
                            let before = requests.entries.len();
                            requests.entries.retain(|entry| kept && entry.silent);
                            requests.entries.len() != before
                        });
                    }
                    Some(_) => {}
                    None => break,
                },
                event = sent.next() => match event {
                    Some(Ok(event))
                        if event.r#type == Some(ResourceType::Document) && apps::is_app_link(&event.request.url) =>
                    {
                        next_id += 1;
                        let id = next_id;
                        let url = event.request.url.clone();
                        waits.push_back((Instant::now() + REFUSAL_WAIT, id));
                        let scheme = scheme_of(&url);
                        early.retain(|(until, _, _)| *until > Instant::now());
                        let said = early
                            .iter()
                            .position(|(_, _, of)| *of == scheme)
                            .and_then(|at| early.remove(at))
                            .map(|(_, said, _)| said);
                        follows.send_modify(|requests| {
                            requests.entries.push_back(Entry {
                                id,
                                scheme: scheme.clone(),
                                url: url.clone(),
                                app: None,
                                unrefused: false,
                                silent: true,
                            });
                            if let Some(said) = said {
                                apply(&mut requests.entries, said, &scheme);
                            }
                        });
                        let (found, ended) = (found.clone(), ended.clone());
                        lookups.spawn_traced(async move {
                            let lookup = tokio::task::spawn_blocking(move || apps::app_for(&url));
                            tokio::select! {
                                _ = ended.cancelled() => {}
                                // A lookup that could not run names no app.
                                app = lookup => { let _ended = found.send((id, app.ok().flatten())); }
                            }
                        });
                    }
                    Some(_) => {}
                    None => break,
                },
                event = said.next() => match event {
                    Some(Ok(event)) => {
                        let Some((said, scheme)) = chrome_said(&event.entry.text) else { continue };
                        if !follows.send_if_modified(|requests| apply(&mut requests.entries, said, &scheme)) {
                            early.push_back((Instant::now() + REFUSAL_WAIT, said, scheme));
                        }
                    }
                    Some(Err(_)) => {}
                    None => break,
                },
                Some((id, app)) = answers.recv() => {
                    tracing::info!(app = app.as_deref(), "a page asked to open a link in an app");
                    follows.send_if_modified(|requests| {
                        requests.entries.iter_mut().find(|entry| entry.id == id).map(|entry| entry.app = Some(app)).is_some()
                    });
                }
                () = tokio::time::sleep_until(wait.unwrap_or_else(Instant::now)), if wait.is_some() => {
                    let Some((_, id)) = waits.pop_front() else { continue };
                    follows.send_if_modified(|requests| {
                        requests.entries.iter_mut().find(|entry| entry.id == id).map(|entry| entry.unrefused = true).is_some()
                    });
                }
            }
        }
    });
    Ok(AppRequests { requests })
}

/// Releases `tab` from Chrome's dialog as the Back button would bring the
/// page back: the top-level page goes to `about:blank`, another site, which
/// closes the dialog, then back in the tab's history. The page loads again
/// with what Chrome keeps of it on Back, the fields' text included, and
/// `about:blank` stays as the tab's next page until the tab goes elsewhere.
/// A page that asks before it is left is left without asking the user: it
/// comes back at once.
pub(crate) async fn release(tab: &BrowserTab) -> Result<()> {
    let page = tab.page();
    let history = page.execute(GetNavigationHistoryParams {}).await?.result;
    let entry_id = |history: &chromiumoxide::cdp::browser_protocol::page::GetNavigationHistoryReturns, offset: i64| {
        usize::try_from(history.current_index + offset)
            .ok()
            .and_then(|index| history.entries.get(index))
            .map(|entry| (entry.id, entry.url.clone()))
    };
    let (back, _) = entry_id(&history, 0).ok_or(BrowserError::TabNotFound)?;
    let mut navigated = page.event_listener::<EventFrameNavigated>().await?;
    let mut dialogs = tab.state().dialog.watch();
    page.execute(NavigateParams::new("about:blank")).await?;
    tokio::time::timeout(CONTROL_TIMEOUT, async {
        loop {
            tokio::select! {
                event = navigated.next() => match event {
                    Some(Ok(event)) if event.frame.parent_id.is_none() && event.frame.url == "about:blank" => {
                        return Ok(());
                    }
                    Some(_) => {}
                    None => return Err(BrowserError::TabNotFound),
                },
                Ok(()) = dialogs.changed() => {
                    let opened = dialogs.borrow_and_update().clone();
                    if let Some(dialog) = opened
                        && dialog.r#type.as_ref() == "beforeunload"
                    {
                        tab.state().dialog.answer(&dialog, true, None).await?;
                    }
                }
            }
        }
    })
    .await
    .map_err(|_| BrowserError::Timeout)??;
    // Back only from the release's own page, never from one the agent or
    // the user opened meanwhile.
    let history = page.execute(GetNavigationHistoryParams {}).await?.result;
    let from_release = entry_id(&history, 0).is_some_and(|(_, url)| url == "about:blank")
        && entry_id(&history, -1).is_some_and(|(id, _)| id == back);
    if from_release {
        page.execute(NavigateToHistoryEntryParams::new(back)).await?;
    }
    Ok(())
}
