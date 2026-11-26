//! A program's process's own routes (`crates/core-api/src/process/mod.rs`):
//!
//! - `POST /cores` opens a core in a directory, `DELETE /cores/{id}` closes
//!   it.
//! - `PUT /addresses` declares where the program serves the calls from
//!   outside; `POST /schedules/calls?id=` and `POST /sign-ins` hand on the
//!   calls it received there.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::ids::UserId;

/// `POST /cores`: the directory of the core to open, made when it is absent
/// or empty; a relative one is taken from the process's working directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, garde::Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OpenCore {
    #[garde(length(min = 1))]
    pub dir: String,
}

/// The core opened: its id, under which its routes are at
/// `/cores/{id}/api`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OpenedCore {
    pub id: UserId,
}

/// `PUT /addresses`: where the program serves each kind of call from
/// outside, as URI templates (RFC 6570) whose `{name}`s Jush fills; an
/// address left out is one the program does not serve.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, garde::Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProgramAddresses {
    /// A schedule's URL, with `{id}`, the schedule's id, such as
    /// `https://bot.example.com/hooks/{id}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[garde(skip)]
    pub schedule: Option<String>,
    /// Where a plugin's sign-in sends the browser back to, with `{core}`,
    /// the core's id, and `{plugin}`, the plugin's, as the program needs
    /// them, such as `https://bot.example.com/sign-in/{core}/{plugin}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[garde(skip)]
    pub sign_in_return: Option<String>,
}

/// What became of a call of a schedule's URL that the program handed on
/// (`POST /schedules/calls?id=`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum ScheduleCallAnswer {
    /// The schedule keeps the call, for a run that may start later or not
    /// at all, as a burst, a delivery sent again and a pause decide.
    Taken,
    Refused { reason: ScheduleCallRefusal },
}

/// Why a schedule refused a call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ScheduleCallRefusal {
    /// No open core holds a URL schedule of the id.
    Unknown,
    /// The call carries the secret neither as a bearer token nor in a
    /// GitHub signature.
    Secret,
    /// The schedule's calls in the last minute are over its limit.
    TooMany,
    /// The call's body, or its headers, are over their limit.
    TooLarge,
}

/// `POST /sign-ins`: the return of a plugin's sign-in, which the vendor
/// sent the browser back to at the program's address.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, garde::Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompleteSignIn {
    /// The core that started the sign-in.
    #[garde(skip)]
    pub core: String,
    #[garde(skip)]
    pub plugin: String,
    /// The query of the address, such as `code=…&state=…`.
    #[garde(skip)]
    pub query: String,
}

/// How a plugin's sign-in ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum SignInAnswer {
    /// The plugin holds the vendor's tokens.
    SignedIn { vendor: String },
    Refused { reason: SignInRefusal },
}

/// Why a sign-in did not end signed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum SignInRefusal {
    /// The core has no plugin of the id that signs in, or has it off.
    UnknownPlugin,
    /// No open core of the id started this sign-in, or it started it over
    /// 10 minutes ago, or the sign-in ended already.
    Expired,
    /// The vendor sent the browser back with an error, such as the user's
    /// refusal.
    Declined,
    /// The vendor issued no tokens for the code.
    VendorFailed,
}
