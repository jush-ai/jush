//! Command timing, and formatting dates and times.

use std::sync::Arc;

use crate::{
    error,
    execution_host::{ExecutionHost, ProcessorTime, Timing},
};

/// What a stopwatch measures processor time with.
enum Measure {
    /// The host's measure of the pipeline.
    Host(Box<dyn Timing>),
    /// The shell process's own processor time and its reaped children's, as
    /// they stood when the stopwatch started.
    Process(ProcessorTime),
}

pub(crate) struct Stopwatch {
    started: std::time::SystemTime,
    measure: Measure,
}

impl Stopwatch {
    pub fn stop(&self) -> Result<StopwatchTiming, error::Error> {
        let wall = std::time::SystemTime::now().duration_since(self.started)?;
        let (time, uncounted) = match &self.measure {
            Measure::Host(timing) => (timing.processor_time(), timing.uncounted()),
            Measure::Process(start) => {
                let end = process_time()?;
                (
                    ProcessorTime {
                        user: end.user.saturating_sub(start.user),
                        system: end.system.saturating_sub(start.system),
                    },
                    None,
                )
            }
        };
        Ok(StopwatchTiming {
            wall,
            user: time.user,
            system: time.system,
            uncounted,
        })
    }
}
pub(crate) struct StopwatchTiming {
    pub wall: std::time::Duration,
    pub user: std::time::Duration,
    pub system: std::time::Duration,
    /// The line that says what `user` and `system` leave out, if anything.
    pub uncounted: Option<&'static str>,
}

/// Starts timing a pipeline, with the measure of `host` when it has one.
pub(crate) fn start_timing(
    host: Option<&Arc<dyn ExecutionHost>>,
) -> Result<Stopwatch, error::Error> {
    let measure = match host.and_then(|host| host.time()) {
        Some(timing) => Measure::Host(timing),
        None => Measure::Process(process_time()?),
    };
    Ok(Stopwatch {
        started: std::time::SystemTime::now(),
        measure,
    })
}

/// The shell process's own processor time and that of the children it reaped.
fn process_time() -> Result<ProcessorTime, error::Error> {
    let (self_user, self_system) = crate::sys::resource::get_self_user_and_system_time()?;
    let (children_user, children_system) =
        crate::sys::resource::get_children_user_and_system_time()?;
    Ok(ProcessorTime {
        user: self_user + children_user,
        system: self_system + children_system,
    })
}

/// Format the given duration in a non-POSIX-y way.
///
/// # Arguments
///
/// * `duration` - The duration to format.
pub fn format_duration_non_posixly(duration: &std::time::Duration) -> String {
    let minutes = duration.as_secs() / 60;
    let seconds = duration.as_secs() % 60;
    let millis = duration.subsec_millis();
    format!("{minutes}m{seconds}.{millis:03}s")
}

/// Format the given duration in a POSIX-y way.
///
/// # Arguments
///
/// * `duration` - The duration to format.
pub fn format_duration_posixly(duration: &std::time::Duration) -> String {
    let seconds = duration.as_secs();
    let ten_millis = duration.subsec_millis() / 10;
    format!("{seconds}.{ten_millis:02}")
}

/// Converts an instant to local time: the one place that decides what "local" means. That's
/// currently the system time zone, from brush's own `TZ` environment variable or else the
/// platform's configuration; it doesn't (yet) follow `TZ` set in the shell. A `TZ` that
/// can't be resolved means UTC; that includes the `posix/` and `right/` prefixed names and
/// POSIX strings that name a DST zone without giving its rules (e.g. `CET-1CEST`).
pub(crate) fn to_local(timestamp: jiff::Timestamp) -> jiff::Zoned {
    timestamp.to_zoned(jiff::tz::TimeZone::system())
}

/// Formats a date/time with a `strftime`-style format string, as the C library does in
/// the POSIX locale: `%c`, `%x`, `%X`, and `%r` take their POSIX forms, and a directive
/// that can't be rendered is left as written while the rest of the format is still
/// rendered.
pub(crate) fn strftime(datetime: &jiff::Zoned, format: &str) -> String {
    let config = jiff::fmt::strtime::Config::new()
        .custom(jiff::fmt::strtime::PosixCustom::new())
        .lenient(true);

    // In lenient mode, formatting only fails if writing to the string does, which it can't.
    jiff::fmt::strtime::BrokenDownTime::from(datetime)
        .to_string_with_config(&config, format)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_strftime() {
        let dt: jiff::Zoned = "2024-12-05T13:04:05.789Z[UTC]".parse().unwrap();
        let cases = [
            ("%Y-%m-%d %H:%M:%S", "2024-12-05 13:04:05"),
            ("%c", "Thu Dec  5 13:04:05 2024"),
            ("%x", "12/05/24"),
            ("%X", "13:04:05"),
            ("%r", "01:04:05 PM"),
            ("%Z", "UTC"),
            // Unknown or incomplete directives are left as written, without
            // dropping the rest of the format.
            ("%F %E %v", "2024-12-05 %E %v"),
            ("%F %", "2024-12-05 %"),
        ];
        for (format, expected) in cases {
            assert_eq!(strftime(&dt, format), expected, "format: {format:?}");
        }
    }

    #[test]
    fn test_format_time() {
        assert_eq!(
            format_duration_non_posixly(&Duration::from_millis(0)),
            "0m0.000s"
        );
        assert_eq!(
            format_duration_non_posixly(&Duration::from_millis(1)),
            "0m0.001s"
        );
        assert_eq!(
            format_duration_non_posixly(&Duration::from_millis(123)),
            "0m0.123s"
        );
        assert_eq!(
            format_duration_non_posixly(&Duration::from_millis(1234)),
            "0m1.234s"
        );
        assert_eq!(
            format_duration_non_posixly(&Duration::from_millis(12345)),
            "0m12.345s"
        );
        assert_eq!(
            format_duration_non_posixly(&Duration::from_millis(123_456)),
            "2m3.456s"
        );
        assert_eq!(
            format_duration_non_posixly(&Duration::from_millis(1_234_567)),
            "20m34.567s"
        );

        assert_eq!(
            format_duration_non_posixly(&Duration::from_micros(1)),
            "0m0.000s"
        );
        assert_eq!(
            format_duration_non_posixly(&Duration::from_micros(999)),
            "0m0.000s"
        );
        assert_eq!(
            format_duration_non_posixly(&Duration::from_micros(1001)),
            "0m0.001s"
        );
    }
}
