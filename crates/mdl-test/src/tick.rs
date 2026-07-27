//! Deterministic tick-stepping and settlement helpers.
//!
//! Promoted out of `crates/mdl-test/tests/stage9_0_schedule_tick_evidence.rs`,
//! which carried the only implementation of `wait_for_gametime_settled`/
//! `step_and_settle` until a second test file needed them — 9.0's own todo
//! explicitly deferred this promotion to whichever of 9B/9C hit that point
//! first (`notes/compiler/stage-9/9-0-contracts-and-evidence.md`); 9B's
//! `stage9b_recurring_scheduling_server.rs` is that second file.

use std::time::{Duration, Instant};

use crate::{HarnessError, Result, TestServer};

/// Sends `time query gametime` and parses the native gametime clock from the
/// server's log-line acknowledgment.
///
/// # Errors
///
/// Returns an error if the command fails to send, the expected log line
/// never appears, or the observed line cannot be parsed as an integer.
pub fn query_gametime(server: &mut TestServer) -> Result<i64> {
    server.command("time query gametime")?;
    let line = server.wait_for_command_log("The game time is")?;
    line.split("is ")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .ok_or_else(|| HarnessError::InvalidConfig(format!("could not parse gametime line: {line:?}")))?
        .parse::<i64>()
        .map_err(|error| {
            HarnessError::InvalidConfig(format!("could not parse gametime line {line:?}: {error}"))
        })
}

/// Polls the native gametime clock until two consecutive reads agree.
///
/// `tick step <N>` acknowledges immediately with `"Stepping N tick(s)"`, but
/// the actual N-tick processing is not necessarily complete by the time that
/// log line appears (discovered empirically during 9.0's own measurements:
/// reading a counter right after the acknowledgment for `N=5` observed only
/// `+1`, i.e. the read raced the step). This is a completion signal that does
/// not assume any particular step duration.
///
/// # Errors
///
/// Returns [`HarnessError::Timeout`] if the gametime clock never settles
/// within 5 seconds, or any error `query_gametime` itself can return.
pub fn wait_for_gametime_settled(server: &mut TestServer) -> Result<i64> {
    let mut previous = query_gametime(server)?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        std::thread::sleep(Duration::from_millis(50));
        let current = query_gametime(server)?;
        if current == previous {
            return Ok(current);
        }
        previous = current;
        if Instant::now() >= deadline {
            return Err(HarnessError::Timeout {
                expected: "gametime clock to settle".to_owned(),
                timeout: Duration::from_secs(5),
                log: format!("last observed gametime {current}"),
            });
        }
    }
}

/// Issues `tick step <n>` and blocks until both the step acknowledgment and
/// the settled gametime clock confirm the step fully completed.
///
/// # Errors
///
/// Returns any error `TestServer::command`, `wait_for_command_log`, or
/// [`wait_for_gametime_settled`] can return.
pub fn step_and_settle(server: &mut TestServer, n: u32) -> Result<()> {
    server.command(&format!("tick step {n}"))?;
    server.wait_for_command_log(&format!("Stepping {n} tick"))?;
    wait_for_gametime_settled(server)?;
    Ok(())
}
