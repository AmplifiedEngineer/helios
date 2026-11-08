// SPDX-License-Identifier: Apache-2.0

//! Runs an external program with a timeout and a cap on its output.

use std::io::{self, Read};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

/// Why a program run failed. No variant holds the program's output, which can
/// carry secrets.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ProcError {
    #[error("program not found")]
    NotFound,
    #[error("program did not finish before the timeout")]
    Timeout,
    #[error("program output is over the size cap")]
    OutputTooLarge,
    #[error("program exited with {status}")]
    Failed { status: ExitStatus },
    #[error("could not run the program: {kind}")]
    Io { kind: io::ErrorKind },
}

/// How often the wait loop checks for exit, the timeout and the output cap.
const POLL_INTERVAL: Duration = Duration::from_millis(10);
const READ_CHUNK: usize = 8192;
/// How long to wait for a pipe to close after the child is gone; a grandchild can hold it open.
const DRAIN_GRACE: Duration = Duration::from_millis(500);

/// Output read from one pipe, or why reading stopped.
enum Drained {
    Done(Vec<u8>),
    OverCap,
    Failed(io::ErrorKind),
}

/// Reads `pipe` to the end, giving up once more than `max_bytes` arrive.
fn drain(mut pipe: impl Read, max_bytes: usize, over_cap: &AtomicBool) -> Drained {
    let mut data = Vec::new();
    let mut chunk = [0u8; READ_CHUNK];
    loop {
        match pipe.read(&mut chunk) {
            Ok(0) => return Drained::Done(data),
            Ok(n) => {
                if data.len() + n > max_bytes {
                    over_cap.store(true, Ordering::Relaxed);
                    return Drained::OverCap;
                }
                data.extend_from_slice(&chunk[..n]);
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Drained::Failed(e.kind()),
        }
    }
}

fn spawn_drain(
    pipe: Option<impl Read + Send + 'static>,
    max_bytes: usize,
    over_cap: Arc<AtomicBool>,
) -> Receiver<Drained> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let drained = match pipe {
            Some(pipe) => drain(pipe, max_bytes, &over_cap),
            None => Drained::Done(Vec::new()),
        };
        // The receiver is gone once `run` has given up on this pipe.
        let _ = sender.send(drained);
    });
    receiver
}

fn kill_and_reap(child: &mut Child) {
    // Both can fail only when the child already exited.
    let _ = child.kill();
    let _ = child.wait();
}

/// Waits up to `DRAIN_GRACE` for a drain thread; a stuck one is left detached.
fn receive(receiver: &Receiver<Drained>) -> Result<Drained, ProcError> {
    match receiver.recv_timeout(DRAIN_GRACE) {
        Ok(drained) => Ok(drained),
        Err(RecvTimeoutError::Timeout) => Err(ProcError::Timeout),
        Err(RecvTimeoutError::Disconnected) => Ok(Drained::Failed(io::ErrorKind::Other)),
    }
}

/// Runs `program` directly, without a shell.
///
/// Each of stdout and stderr is capped at `max_bytes`. The child is killed on
/// timeout or when either stream goes over the cap. Output text never appears
/// in an error.
pub fn run(
    program: &Path,
    args: &[&str],
    timeout: Duration,
    max_bytes: usize,
) -> Result<Output, ProcError> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| match e.kind() {
            io::ErrorKind::NotFound => ProcError::NotFound,
            kind => ProcError::Io { kind },
        })?;

    let over_cap = Arc::new(AtomicBool::new(false));
    let stdout = spawn_drain(child.stdout.take(), max_bytes, Arc::clone(&over_cap));
    let stderr = spawn_drain(child.stderr.take(), max_bytes, Arc::clone(&over_cap));

    let deadline = Instant::now() + timeout;
    let outcome = loop {
        if over_cap.load(Ordering::Relaxed) {
            kill_and_reap(&mut child);
            break Err(ProcError::OutputTooLarge);
        }
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if Instant::now() >= deadline => {
                kill_and_reap(&mut child);
                break Err(ProcError::Timeout);
            }
            Ok(None) => thread::sleep(POLL_INTERVAL),
            Err(e) => {
                kill_and_reap(&mut child);
                break Err(ProcError::Io { kind: e.kind() });
            }
        }
    };

    let status = outcome?;
    let (stdout, stderr) = (receive(&stdout)?, receive(&stderr)?);
    match (stdout, stderr) {
        (Drained::OverCap, _) | (_, Drained::OverCap) => Err(ProcError::OutputTooLarge),
        (Drained::Failed(kind), _) | (_, Drained::Failed(kind)) => Err(ProcError::Io { kind }),
        (Drained::Done(stdout), Drained::Done(stderr)) if status.success() => Ok(Output {
            status,
            stdout,
            stderr,
        }),
        _ => Err(ProcError::Failed { status }),
    }
}
