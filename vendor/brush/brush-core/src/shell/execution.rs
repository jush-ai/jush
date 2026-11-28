//! Execution support for shell.

use std::{io::Read as _, path::Path};

use crate::{
    ExecutionControlFlow, ExecutionParameters, ExecutionResult, ProcessGroupPolicy, SourceInfo,
    arithmetic::Evaluatable as _, callstack, error, interp::Execute as _, openfiles,
};

impl<SE: crate::extensions::ShellExtensions> crate::Shell<SE> {
    /// Returns the default execution parameters for this shell.
    pub fn default_exec_params(&self) -> ExecutionParameters {
        let mut params = ExecutionParameters::default();

        params.process_group_policy = if self.options.enable_job_control {
            ProcessGroupPolicy::NewProcessGroup
        } else {
            ProcessGroupPolicy::SameProcessGroup
        };

        params
    }

    pub(super) async fn source_if_exists(
        &mut self,
        path: impl AsRef<Path>,
        params: &ExecutionParameters,
    ) -> Result<bool, error::Error> {
        let path = path.as_ref();
        if path.exists() {
            // TODO(startup): an `exit` in a startup file should end the shell, but
            // `load_config` has no way to tell its caller to stop; until it does, the
            // file's result is dropped. Tracked by the compat case "exit in an rc file
            // ends the shell".
            let _ = self
                .source_script(path, std::iter::empty::<String>(), params)
                .await?;
            Ok(true)
        } else {
            tracing::debug!("skipping non-existent file: {}", path.display());
            Ok(false)
        }
    }

    /// Source the given file as a shell script, returning the execution result.
    ///
    /// # Arguments
    ///
    /// * `path` - The path to the file to source.
    /// * `args` - The arguments to pass to the script as positional parameters.
    /// * `params` - Execution parameters.
    pub async fn source_script<S: Into<String>, P: AsRef<Path>, I: Iterator<Item = S>>(
        &mut self,
        path: P,
        args: I,
        params: &ExecutionParameters,
    ) -> Result<ExecutionResult, error::Error> {
        self.parse_and_execute_script_file(
            path.as_ref(),
            args,
            params,
            callstack::ScriptCallType::Source,
        )
        .await
    }

    /// Source an already parsed program, as [`Self::source_script`] sources a file: in a
    /// script's call frame, with `args` as its positional parameters. A library sourced by
    /// many scripts then needs parsing only once.
    ///
    /// Parsing is the caller's job. A parse error never reaches this function, so it is not
    /// reported as fatal the way [`Self::source_script`] reports one. The program also keeps
    /// the [`Self::parser_options`] it was parsed with (e.g., extended globbing), so a cache
    /// of parsed programs should be keyed on them.
    ///
    /// # Arguments
    ///
    /// * `program` - The parsed program to run.
    /// * `source_info` - Where the program came from, for its call frame and errors.
    /// * `args` - The arguments to pass to the script as positional parameters.
    /// * `params` - Execution parameters.
    pub async fn source_program<S: Into<String>, I: Iterator<Item = S>>(
        &mut self,
        program: &brush_parser::ast::Program,
        source_info: &crate::SourceInfo,
        args: I,
        params: &ExecutionParameters,
    ) -> Result<ExecutionResult, error::Error> {
        self.run_in_script_frame(
            callstack::ScriptCallType::Source,
            source_info,
            args,
            Ok(program),
            params,
        )
        .await
    }

    /// Parse and execute the given file as a shell script, returning the execution result.
    ///
    /// # Arguments
    ///
    /// * `path` - The path to the file to source.
    /// * `args` - The arguments to pass to the script as positional parameters.
    /// * `params` - Execution parameters.
    /// * `call_type` - The type of script call being made.
    async fn parse_and_execute_script_file<
        S: Into<String>,
        P: AsRef<Path>,
        I: Iterator<Item = S>,
    >(
        &mut self,
        path: P,
        args: I,
        params: &ExecutionParameters,
        call_type: callstack::ScriptCallType,
    ) -> Result<ExecutionResult, error::Error> {
        let path = path.as_ref();
        tracing::debug!("sourcing: {}", path.display());
        let opened_file = self.open_script_file(path, params)?;
        let text = Self::read_script_text(opened_file, path)?;

        let source_info = crate::SourceInfo::from(path.to_owned());
        let parse_result = self.parse(text.as_bytes());

        self.run_in_script_frame(call_type, &source_info, args, parse_result, params)
            .await
    }

    /// Opens the file a script is read from.
    fn open_script_file(
        &self,
        path: &Path,
        params: &ExecutionParameters,
    ) -> Result<openfiles::OpenFile, error::Error> {
        let mut options = std::fs::File::options();
        options.read(true);

        let opened_file: openfiles::OpenFile = self
            .open_file(&options, false, path, params)
            .map_err(|e| error::ErrorKind::FailedSourcingFile(path.to_owned(), e))?;

        if opened_file.is_dir() {
            return Err(error::ErrorKind::FailedSourcingFile(
                path.to_owned(),
                std::io::Error::from(std::io::ErrorKind::IsADirectory),
            )
            .into());
        }

        Ok(opened_file)
    }

    /// Reads the text of the script file `path`, opened as `file`: its bytes as the shell's
    /// text (`bytes::decode`), which the parser reads as UTF-8.
    fn read_script_text(
        mut file: openfiles::OpenFile,
        path: &Path,
    ) -> Result<String, error::Error> {
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .map_err(|e| error::ErrorKind::FailedSourcingFile(path.to_owned(), e))?;
        Ok(crate::bytes::decode_cow(&bytes).into_owned())
    }

    /// Runs a script's program in a new call frame for the script, as every way of running
    /// a script file or a parsed program as a script does: enters the frame, runs the
    /// program (or reports why it failed to parse), reports any error, and ends the
    /// script's `return` at the script's boundary.
    ///
    /// # Arguments
    ///
    /// * `call_type` - The type of script call being made.
    /// * `source_info` - Where the script came from, for its call frame and errors.
    /// * `args` - The arguments to pass to the script as positional parameters.
    /// * `parse_result` - The script's parsed program, or the error parsing it failed with.
    /// * `params` - Execution parameters.
    async fn run_in_script_frame<
        S: Into<String>,
        I: Iterator<Item = S>,
        P: std::borrow::Borrow<brush_parser::ast::Program>,
    >(
        &mut self,
        call_type: callstack::ScriptCallType,
        source_info: &crate::SourceInfo,
        args: I,
        parse_result: Result<P, brush_parser::ParseError>,
        params: &ExecutionParameters,
    ) -> Result<ExecutionResult, error::Error> {
        let mut result = self
            .enter_script(call_type, source_info, args.map(Into::into))
            .run_parsed_result(parse_result, source_info, params)
            .await?;

        // A `return` that ended the script has been used up at the script's boundary. All
        // other control flow is preserved.
        if matches!(
            result.next_control_flow,
            ExecutionControlFlow::ReturnFromFunctionOrScript
        ) {
            result.next_control_flow = ExecutionControlFlow::Normal;
        }

        Ok(result)
    }

    /// Executes the given string as a shell program, returning the resulting exit status.
    ///
    /// # Arguments
    ///
    /// * `command` - The command to execute.
    /// * `source_info` - Information about the source of the command text.
    /// * `params` - Execution parameters.
    pub async fn run_string<S: Into<String>>(
        &mut self,
        command: S,
        source_info: &crate::SourceInfo,
        params: &ExecutionParameters,
    ) -> Result<ExecutionResult, error::Error> {
        let parse_result = self.parse_string(command);
        self.run_parsed_result(parse_result, source_info, params)
            .await
    }

    /// Executes the given command, provided to a shell executable on the command
    /// line (i.e., via `-c`).
    ///
    /// It is expected that the shell will not be used for any further execution
    /// after this command; this function will perform any necessary shell exit
    /// handling.
    ///
    /// # Arguments
    ///
    /// * `command` - The command to execute.
    pub async fn run_dash_c_command<S: Into<String>>(
        &mut self,
        command: S,
    ) -> Result<ExecutionResult, error::Error> {
        self.start_command_string_mode();

        // Execute the command string.
        let params = self.default_exec_params();
        let source_info = SourceInfo::from("-c");
        let mut result = self.run_string(command, &source_info, &params).await?;

        self.end_command_string_mode()?;

        // Give the shell a chance to run on-exit tasks. An `exit` there sets the shell's
        // status; a failure there doesn't change the command's outcome.
        if let Ok(Some(status)) = self.run_exit_trap().await {
            result.exit_code = status.into();
        }

        Ok(result)
    }

    /// Executes the given script file, returning the resulting exit status.
    ///
    /// It is expected that the shell will not be used for any further execution
    /// after this command; this function will perform any necessary shell exit
    /// handling.
    ///
    /// # Arguments
    ///
    /// * `script_path` - The path to the script file to execute.
    /// * `args` - The arguments to pass to the script as positional parameters.
    pub async fn run_script<S: Into<String>, P: AsRef<Path>, I: Iterator<Item = S>>(
        &mut self,
        script_path: P,
        args: I,
    ) -> Result<ExecutionResult, error::Error> {
        let params = self.default_exec_params();
        let mut result = self
            .parse_and_execute_script_file(
                script_path.as_ref(),
                args,
                &params,
                callstack::ScriptCallType::Run,
            )
            .await?;

        // Give the shell a chance to run on-exit tasks. An `exit` there sets the shell's
        // status; a failure there doesn't change the script's outcome.
        if let Ok(Some(status)) = self.run_exit_trap().await {
            result.exit_code = status.into();
        }

        Ok(result)
    }

    /// Executes the given text as a shell reads a script or a `-c` string:
    /// each command runs once the text up to its end is read, so one that
    /// precedes a syntax error has run when the error is reported, and an
    /// `exit` before text that does not parse ends the shell without
    /// reading it.
    ///
    /// # Arguments
    ///
    /// * `text` - The text to execute.
    /// * `source_info` - Information about the source of the text.
    /// * `params` - Execution parameters.
    pub async fn run_string_incrementally(
        &mut self,
        text: &str,
        source_info: &crate::SourceInfo,
        params: &ExecutionParameters,
    ) -> Result<ExecutionResult, error::Error> {
        // Text that parses as a whole, which nearly every script does, runs
        // as one program.
        let parse_result = self.parse_string(text);
        if parse_result.is_ok() {
            return self.run_parsed_result(parse_result, source_info, params).await;
        }

        let mut result = ExecutionResult::success();
        let mut chunk = String::new();
        let mut first_line = 1;
        for (index, line) in text.split_inclusive('\n').enumerate() {
            if chunk.is_empty() {
                first_line = index + 1;
            }
            chunk.push_str(line);
            // Blank lines in front keep the line numbers of what follows.
            let padded = format!("{}{chunk}", "\n".repeat(first_line - 1));
            match self.parse_string(padded) {
                Ok(program) => {
                    chunk.clear();
                    result = self
                        .run_parsed_result(Ok(program), source_info, params)
                        .await?;
                    if !result.is_normal_flow() {
                        return Ok(result);
                    }
                }
                // More lines may complete it.
                Err(error) if Self::is_incomplete(&error) => {}
                Err(error) => {
                    return self
                        .run_parsed_result::<brush_parser::ast::Program>(
                            Err(error),
                            source_info,
                            params,
                        )
                        .await;
                }
            }
        }
        if chunk.is_empty() {
            return Ok(result);
        }
        let padded = format!("{}{chunk}", "\n".repeat(first_line - 1));
        let parse_result = self.parse_string(padded);
        self.run_parsed_result(parse_result, source_info, params).await
    }

    /// Whether `error` is the input ending before a command does, which more
    /// input can mend.
    fn is_incomplete(error: &brush_parser::ParseError) -> bool {
        use brush_parser::{ParseError, TokenizerError};
        match error {
            ParseError::ParsingAtEndOfInput => true,
            ParseError::ParsingNear(_) => false,
            ParseError::Tokenizing { inner, .. } => matches!(
                inner,
                TokenizerError::UnterminatedEscapeSequence
                    | TokenizerError::UnterminatedSingleQuote(_)
                    | TokenizerError::UnterminatedAnsiCQuote(_)
                    | TokenizerError::UnterminatedDoubleQuote(_)
                    | TokenizerError::UnterminatedBackquote(_)
                    | TokenizerError::UnterminatedExtendedGlob(_)
                    | TokenizerError::UnterminatedVariable
                    | TokenizerError::UnterminatedCommandSubstitution
                    | TokenizerError::UnterminatedExpansion
                    | TokenizerError::UnterminatedHereDocuments(..)
            ),
            _ => false,
        }
    }

    /// Executes the script file at `path` as `run_script` does, reading it as
    /// `run_string_incrementally` reads text: a script may carry data that is
    /// not a program after its `exit`.
    ///
    /// # Arguments
    ///
    /// * `script_path` - The path to the script file to execute.
    /// * `args` - The arguments to pass to the script as positional parameters.
    pub async fn run_script_incrementally<S: Into<String>, P: AsRef<Path>, I: Iterator<Item = S>>(
        &mut self,
        script_path: P,
        args: I,
    ) -> Result<ExecutionResult, error::Error> {
        let params = self.default_exec_params();
        let path = script_path.as_ref();
        let file = self.open_script_file(path, &params)?;
        let text = Self::read_script_text(file, path)?;
        let source_info = crate::SourceInfo::from(path.to_owned());

        let mut result = self
            .enter_script(
                callstack::ScriptCallType::Run,
                &source_info,
                args.map(Into::into),
            )
            .run_string_incrementally(&text, &source_info, &params)
            .await?;
        if matches!(
            result.next_control_flow,
            ExecutionControlFlow::ReturnFromFunctionOrScript
        ) {
            result.next_control_flow = ExecutionControlFlow::Normal;
        }

        // Give the shell a chance to run on-exit tasks. An `exit` there sets the shell's
        // status; a failure there doesn't change the script's outcome.
        if let Ok(Some(status)) = self.run_exit_trap().await {
            result.exit_code = status.into();
        }

        Ok(result)
    }

    /// Executes a `-c` command string as `run_dash_c_command` does, reading it
    /// as `run_string_incrementally` reads text.
    ///
    /// # Arguments
    ///
    /// * `command` - The command to execute.
    pub async fn run_dash_c_incrementally(
        &mut self,
        command: &str,
    ) -> Result<ExecutionResult, error::Error> {
        self.start_command_string_mode();
        let params = self.default_exec_params();
        let source_info = SourceInfo::from("-c");
        let mut result = self
            .run_string_incrementally(command, &source_info, &params)
            .await?;
        self.end_command_string_mode()?;
        if let Ok(Some(status)) = self.run_exit_trap().await {
            result.exit_code = status.into();
        }
        Ok(result)
    }

    pub(crate) async fn run_parsed_result<P: std::borrow::Borrow<brush_parser::ast::Program>>(
        &mut self,
        parse_result: Result<P, brush_parser::ParseError>,
        source_info: &crate::SourceInfo,
        params: &ExecutionParameters,
    ) -> Result<ExecutionResult, error::Error> {
        // If parsing succeeded, run the program. If there's a parse error, it's fatal (per spec).
        let result = match parse_result {
            Ok(prog) => prog.borrow().execute(self, params).await,
            Err(parse_err) => Err(error::Error::from(error::ErrorKind::ParseError(
                parse_err,
                source_info.clone(),
            ))
            .into_fatal()),
        };

        // Report any errors.
        match result {
            Ok(result) => Ok(result),
            Err(err) => {
                let _ = self.display_error(&mut params.stderr(self), &err);

                let result = err.into_result(self);
                self.set_last_exit_status(result.exit_code.into());

                Ok(result)
            }
        }
    }

    /// Executes the given parsed shell program, returning the resulting exit status.
    ///
    /// # Arguments
    ///
    /// * `program` - The program to execute.
    /// * `params` - Execution parameters.
    pub async fn run_program(
        &mut self,
        program: brush_parser::ast::Program,
        params: &ExecutionParameters,
    ) -> Result<ExecutionResult, error::Error> {
        program.execute(self, params).await
    }

    /// Evaluate the given arithmetic expression, returning the result.
    pub fn eval_arithmetic(
        &mut self,
        expr: &brush_parser::ast::ArithmeticExpr,
    ) -> Result<i64, error::Error> {
        Ok(expr.eval(self)?)
    }
}
