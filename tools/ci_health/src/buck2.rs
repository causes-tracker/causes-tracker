//! Parses buck2's end-of-build summary lines out of a CI job log.
//!
//! Every buck2 invocation that runs actions prints, on stderr:
//!
//! ```text
//! [2026-07-15T10:30:00.625+00:00] Build ID: ac446c1c-…
//! [2026-07-15T10:30:04.155+00:00] Cache hits: 100%
//! [2026-07-15T10:30:04.155+00:00] Commands: 2 (cached: 2, remote: 0, local: 0)
//! [2026-07-15T10:30:04.155+00:00] Network: Up: 270KiB  Down: 90MiB  (GRPC-SESSION-ID)
//! ```
//!
//! The `Network:` line is absent in local-only mode (no remote cache
//! configured), and the `Cache hits:` percentage is redundant with the
//! `Commands:` counts, so only `Build ID:`/`Commands:`/`Network:` are read.

use crate::metrics::{Buck2BuildId, Buck2Invocation};
use anyhow::{Context, Result, bail};

/// Extract one `Buck2Invocation` per action-running buck2 invocation in `log`.
/// A `Build ID:` with no following `Commands:` line (e.g. `buck2 clean`) is
/// not an action-running invocation and is dropped.
/// Malformed `Commands:`/`Network:` lines fail loudly.
pub fn extract_invocations(log: &str) -> Result<Vec<Buck2Invocation>> {
    let mut out = Vec::new();
    let mut current: Option<Pending> = None;
    for line in log.lines() {
        if let Some(rest) = after(line, "] Build ID: ") {
            if let Some(done) = current.take().and_then(Pending::finish) {
                out.push(done);
            }
            current = Some(Pending::new(token(rest)));
        } else if let Some(rest) = after(line, "] Commands: ") {
            let cur = current
                .as_mut()
                .with_context(|| format!("Commands line before any Build ID: {line}"))?;
            if cur.commands.is_some() {
                bail!("second Commands line for build {}: {line}", cur.build_id.0);
            }
            cur.commands = Some(parse_commands(rest).with_context(|| format!("parse {line}"))?);
        } else if let Some(rest) = after(line, "] Network: ") {
            let cur = current
                .as_mut()
                .with_context(|| format!("Network line before any Build ID: {line}"))?;
            if let Some(net) = parse_network(rest).with_context(|| format!("parse {line}"))? {
                cur.network = Some(net);
            }
        }
    }
    if let Some(done) = current.and_then(Pending::finish) {
        out.push(done);
    }
    Ok(out)
}

struct Pending {
    build_id: Buck2BuildId,
    commands: Option<Commands>,
    network: Option<Network>,
}

struct Commands {
    total: u64,
    cached: u64,
    remote: u64,
    local: u64,
}

struct Network {
    up: u64,
    down: u64,
}

impl Pending {
    fn new(build_id: &str) -> Self {
        Self {
            build_id: Buck2BuildId(build_id.to_owned()),
            commands: None,
            network: None,
        }
    }

    fn finish(self) -> Option<Buck2Invocation> {
        let commands = self.commands?;
        let network = self.network.unwrap_or(Network { up: 0, down: 0 });
        Some(Buck2Invocation {
            build_id: self.build_id,
            commands_total: commands.total,
            commands_cached: commands.cached,
            commands_remote: commands.remote,
            commands_local: commands.local,
            bytes_uploaded: network.up,
            bytes_downloaded: network.down,
        })
    }
}

/// The remainder of `line` after `needle`, when present.
fn after<'a>(line: &'a str, needle: &str) -> Option<&'a str> {
    line.find(needle).map(|i| &line[i + needle.len()..])
}

/// The leading whitespace-delimited token of `rest`.
fn token(rest: &str) -> &str {
    rest.split_whitespace().next().unwrap_or("")
}

/// Parse `2 (cached: 2, remote: 0, local: 0)`.
fn parse_commands(rest: &str) -> Result<Commands> {
    let total = token(rest).parse().context("total")?;
    let field = |key: &str| -> Result<u64> {
        let val = after(rest, key).with_context(|| format!("missing {key}"))?;
        val.split(|c: char| !c.is_ascii_digit())
            .next()
            .unwrap_or("")
            .parse()
            .with_context(|| key.to_owned())
    };
    Ok(Commands {
        total,
        cached: field("cached: ")?,
        remote: field("remote: ")?,
        local: field("local: ")?,
    })
}

/// Parse buck2's `Network:` remainder, accepting both the
/// `Up: 270KiB  Down: 90MiB  (SESSION)` and `up 5.1KiB  down 4.2MiB  session
/// SESSION` shapes. A line carrying neither count (`session SESSION` alone,
/// printed when no bytes moved) yields `None`; one count without the other
/// is malformed and errors.
fn parse_network(rest: &str) -> Result<Option<Network>> {
    let side = |upper: &str, lower: &str| after(rest, upper).or_else(|| after(rest, lower));
    match (side("Up: ", "up "), side("Down: ", "down ")) {
        (Some(up), Some(down)) => Ok(Some(Network {
            up: parse_size(token(up))?,
            down: parse_size(token(down))?,
        })),
        (None, None) => Ok(None),
        (Some(_), None) => bail!("missing down"),
        (None, Some(_)) => bail!("missing up"),
    }
}

/// Parse buck2's humanized byte sizes: `0B`, `270KiB`, `95MiB`, `1.2GiB`.
fn parse_size(s: &str) -> Result<u64> {
    let split = s
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .with_context(|| format!("no unit suffix in size {s:?}"))?;
    let (num, unit) = s.split_at(split);
    let value: f64 = num.parse().with_context(|| format!("size {s:?}"))?;
    let multiplier: f64 = match unit {
        "B" => 1.0,
        "KiB" => 1024.0,
        "MiB" => 1024.0 * 1024.0,
        "GiB" => 1024.0 * 1024.0 * 1024.0,
        "TiB" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
        _ => bail!("unknown size unit {unit:?} in {s:?}"),
    };
    Ok((value * multiplier).round() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verbatim shape of the CI `buck2` job log (run 29408325080): GH
    /// timestamp prefix, buck2 timestamp bracket, two invocations
    /// (populate build + round-trip rebuild), and noise lines that must
    /// not match — including the workflow's echoed script, which contains
    /// the text `Cache hits: 100%`.
    const CI_LOG: &str = "\
2026-07-15T10:29:58.7962448Z ##[group]Run buck2 build //...
2026-07-15T10:30:00.6284552Z [2026-07-15T10:30:00.625+00:00] Build ID: ac446c1c-5913-44d6-bd0e-0c1a47d36f26
2026-07-15T10:30:04.1555320Z [2026-07-15T10:30:04.155+00:00] Cache hits: 100%
2026-07-15T10:30:04.1556498Z [2026-07-15T10:30:04.155+00:00] Commands: 2 (cached: 2, remote: 0, local: 0)
2026-07-15T10:30:04.1557879Z [2026-07-15T10:30:04.155+00:00] Network: Up: 270KiB  Down: 90MiB  (GRPC-SESSION-ID)
2026-07-15T10:30:04.1579315Z BUILD SUCCEEDED
2026-07-15T10:30:04.1730050Z ##[group]Run buck2 clean
2026-07-15T10:30:04.1732178Z \u{1b}[36;1mgrep -q 'Cache hits: 100%' rebuild.log\u{1b}[0m
2026-07-15T10:30:04.7498178Z [2026-07-15T10:30:04.746+00:00] Build ID: 40e60d62-f0c7-4990-9649-9320026c1626
2026-07-15T10:30:08.0737989Z [2026-07-15T10:30:08.073+00:00] Cache hits: 100%
2026-07-15T10:30:08.0739025Z [2026-07-15T10:30:08.073+00:00] Commands: 2 (cached: 2, remote: 0, local: 0)
2026-07-15T10:30:08.0740052Z [2026-07-15T10:30:08.073+00:00] Network: Up: 271KiB  Down: 95MiB  (GRPC-SESSION-ID)
2026-07-15T10:30:08.0754127Z BUILD SUCCEEDED
";

    #[test]
    fn extracts_both_invocations_from_ci_log() {
        let got = extract_invocations(CI_LOG).unwrap();
        assert_eq!(
            got,
            vec![
                Buck2Invocation {
                    build_id: Buck2BuildId("ac446c1c-5913-44d6-bd0e-0c1a47d36f26".into()),
                    commands_total: 2,
                    commands_cached: 2,
                    commands_remote: 0,
                    commands_local: 0,
                    bytes_uploaded: 270 * 1024,
                    bytes_downloaded: 90 * 1024 * 1024,
                },
                Buck2Invocation {
                    build_id: Buck2BuildId("40e60d62-f0c7-4990-9649-9320026c1626".into()),
                    commands_total: 2,
                    commands_cached: 2,
                    commands_remote: 0,
                    commands_local: 0,
                    bytes_uploaded: 271 * 1024,
                    bytes_downloaded: 95 * 1024 * 1024,
                },
            ]
        );
    }

    /// Local-only mode (no remote cache configured) prints no `Network:`
    /// line; bytes default to zero.
    /// Captured from a dev-workspace build on 2026-07-15.
    #[test]
    fn local_mode_has_no_network_line() {
        let log = "\
[2026-07-15T10:41:33.194+00:00] Build ID: c28c6d92-0198-4288-b836-b4e023a2fd9e
[2026-07-15T10:41:36.545+00:00] Cache hits: 0%
[2026-07-15T10:41:36.545+00:00] Commands: 2 (cached: 0, remote: 0, local: 2)
BUILD SUCCEEDED
";
        let got = extract_invocations(log).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].commands_local, 2);
        assert_eq!(got[0].bytes_uploaded, 0);
        assert_eq!(got[0].bytes_downloaded, 0);
        assert_eq!(got[0].commands_cached, 0);
    }

    /// The newer buck2 `Network:` line shape: lowercase `up`/`down` with a
    /// `session` label.
    #[test]
    fn parses_newer_network_line_shape() {
        let log = "\
[2026-08-08T11:58:50.676+00:00] Build ID: aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee
[2026-08-08T11:58:50.676+00:00] Commands: 7 (cached: 4, remote: 0, local: 3)
[2026-08-08T11:58:50.676+00:00] Network: up 5.1KiB  down 4.2MiB  session GRPC-SESSION-ID
BUILD SUCCEEDED
";
        let got = extract_invocations(log).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].bytes_uploaded, (5.1_f64 * 1024.0).round() as u64);
        assert_eq!(
            got[0].bytes_downloaded,
            (4.2_f64 * 1024.0 * 1024.0).round() as u64
        );
    }

    /// buck2 prints a `Network:` line carrying only a session label, no
    /// byte counts, when the invocation moved no bytes; it records zero
    /// traffic rather than erroring.
    #[test]
    fn network_line_without_byte_counts_records_zero() {
        let log = "\
[2026-08-08T18:28:48.554+00:00] Build ID: aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee
[2026-08-08T18:28:48.554+00:00] Commands: 2 (cached: 2, remote: 0, local: 0)
[2026-08-08T18:28:48.554+00:00] Network: session GRPC-SESSION-ID
BUILD SUCCEEDED
";
        let got = extract_invocations(log).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].bytes_uploaded, 0);
        assert_eq!(got[0].bytes_downloaded, 0);
    }

    /// A `Network:` line carrying one count but not the other is malformed,
    /// distinct from the countless line above, and errors.
    #[test]
    fn network_line_with_one_count_errors() {
        let log = "\
[2026-08-08T18:28:48.554+00:00] Build ID: aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee
[2026-08-08T18:28:48.554+00:00] Commands: 2 (cached: 2, remote: 0, local: 0)
[2026-08-08T18:28:48.554+00:00] Network: up 5.1KiB  session GRPC-SESSION-ID
BUILD SUCCEEDED
";
        assert!(extract_invocations(log).is_err());
    }

    /// `buck2 clean` / `buck2 kill` print a `Build ID:` but run no
    /// actions; they must not produce an invocation.
    #[test]
    fn build_id_without_commands_is_dropped() {
        let log = "\
[2026-07-15T10:30:04.5.000+00:00] Build ID: 11111111-2222-3333-4444-555555555555
/home/runner/work/causes-tracker/causes-tracker/buck-out/v2/CACHEDIR.TAG
[2026-07-15T10:30:04.746+00:00] Build ID: 40e60d62-f0c7-4990-9649-9320026c1626
[2026-07-15T10:30:08.073+00:00] Commands: 3 (cached: 1, remote: 0, local: 2)
";
        let got = extract_invocations(log).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(
            got[0].build_id,
            Buck2BuildId("40e60d62-f0c7-4990-9649-9320026c1626".into())
        );
        assert_eq!(got[0].commands_total, 3);
    }

    #[test]
    fn malformed_commands_line_errors() {
        let log = "\
[2026-07-15T10:30:04.746+00:00] Build ID: 40e60d62-f0c7-4990-9649-9320026c1626
[2026-07-15T10:30:08.073+00:00] Commands: 2 (cached: two, remote: 0, local: 0)
";
        assert!(extract_invocations(log).is_err());
    }

    #[test]
    fn empty_log_yields_no_invocations() {
        assert_eq!(extract_invocations("").unwrap(), vec![]);
    }

    #[test]
    fn parses_humanized_sizes() {
        assert_eq!(parse_size("0B").unwrap(), 0);
        assert_eq!(parse_size("270KiB").unwrap(), 276480);
        assert_eq!(parse_size("95MiB").unwrap(), 99614720);
        assert_eq!(parse_size("1.2GiB").unwrap(), 1288490189);
        assert_eq!(parse_size("3TiB").unwrap(), 3 * 1024u64.pow(4));
        assert!(parse_size("95XB").is_err());
        assert!(parse_size("").is_err());
    }
}
