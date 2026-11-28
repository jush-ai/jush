//! The runner's builtins as brush registers a builtin: each takes its words
//! as they are, its name first, and parses them itself, as a utility does.

use std::marker::PhantomData;

use brush_core::{
    BuiltinError, CommandArg, ErrorKind, ExecutionContext, ExecutionExitCode, ExecutionResult,
    builtins::{
        BoxFuture, Builtin, ContentOptions, ContentType, Invocation, ParseArgs, Registration,
        UsageError,
    },
    extensions::DefaultShellExtensions,
};

/// A builtin of the runner's.
pub(crate) trait RunnerBuiltin: 'static {
    /// Runs one invocation; `args` start with the name it was invoked under.
    fn run(
        context: ExecutionContext<'_>,
        args: Vec<CommandArg>,
    ) -> BoxFuture<'_, Result<ExecutionResult, brush_core::Error>>;

    /// Its help of `content_type`, as invoked under `name`.
    fn content(
        name: &str,
        content_type: ContentType,
        options: &ContentOptions,
    ) -> Result<String, brush_core::Error>;
}

/// The registration of the runner's builtin `B`.
pub(crate) const fn registration<B: RunnerBuiltin>() -> Registration<DefaultShellExtensions> {
    Registration::of::<Words<B>>()
}

/// One invocation of `B`: its words, the name first.
struct Words<B> {
    args: Vec<CommandArg>,
    builtin: PhantomData<fn() -> B>,
}

impl<B: RunnerBuiltin> ParseArgs for Words<B> {
    fn content(
        name: &str,
        content_type: ContentType,
        options: &ContentOptions,
    ) -> Result<String, brush_core::Error> {
        B::content(name, content_type, options)
    }

    fn parse(invocation: Invocation<'_>) -> Result<Self, UsageError> {
        let name = CommandArg::String(invocation.name().to_owned());
        Ok(Self {
            args: std::iter::once(name).chain(invocation.into_args()).collect(),
            builtin: PhantomData,
        })
    }
}

impl<B: RunnerBuiltin> Builtin<DefaultShellExtensions> for Words<B> {
    type Error = Failure;

    async fn execute(self, context: ExecutionContext<'_>) -> Result<ExecutionResult, Self::Error> {
        B::run(context, self.args).await.map_err(Failure)
    }
}

/// Why a builtin of the runner's failed. A message about it names the
/// builtin once: a failure of brush's builtin that it ran, as `wait -n` runs
/// brush's `wait`, names it already.
#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub(crate) struct Failure(brush_core::Error);

impl BuiltinError for Failure {
    fn as_io_error(&self) -> Option<&std::io::Error> {
        self.0.as_io_error()
    }

    fn names_builtin(&self) -> bool {
        !matches!(self.0.kind(), ErrorKind::BuiltinError(..)) && self.0.names_builtin()
    }
}

impl From<&Failure> for ExecutionExitCode {
    fn from(failure: &Failure) -> Self {
        Self::from(&failure.0)
    }
}
