use crate::CancellationToken;
use std::{
    error::Error,
    fmt,
    io::{BufRead, BufReader, Read},
    process::{Command, Stdio},
    sync::mpsc::{self, Receiver, Sender},
    thread::{self, JoinHandle},
    time::Duration,
};

const MAX_CAPTURE_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandStream {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    pub success: bool,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandRunError {
    Cancelled,
    Spawn(String),
    Wait(String),
    Stream(String),
}

impl fmt::Display for CommandRunError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => formatter.write_str("External command was cancelled."),
            Self::Spawn(message) | Self::Wait(message) | Self::Stream(message) => {
                formatter.write_str(message)
            }
        }
    }
}

impl Error for CommandRunError {}

pub fn run_cancellable_command<F>(
    command: &mut Command,
    cancellation: &CancellationToken,
    mut on_line: F,
) -> Result<CommandOutput, CommandRunError>
where
    F: FnMut(CommandStream, &str),
{
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            CommandRunError::Spawn(format!("Could not start external command: {error}"))
        })?;

    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(CommandRunError::Spawn(
                "External command did not expose stdout.".into(),
            ));
        }
    };
    let stderr = match child.stderr.take() {
        Some(stderr) => stderr,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(CommandRunError::Spawn(
                "External command did not expose stderr.".into(),
            ));
        }
    };

    let (sender, receiver) = mpsc::channel();
    let stdout_reader =
        spawn_reader(stdout, CommandStream::Stdout, sender.clone()).map_err(|error| {
            let _ = child.kill();
            let _ = child.wait();
            CommandRunError::Spawn(format!("Could not start stdout reader: {error}"))
        })?;
    let stderr_reader = match spawn_reader(stderr, CommandStream::Stderr, sender) {
        Ok(reader) => reader,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stdout_reader.join();
            return Err(CommandRunError::Spawn(format!(
                "Could not start stderr reader: {error}"
            )));
        }
    };

    let mut stdout_capture = String::new();
    let mut stderr_capture = String::new();

    let status = loop {
        if let Some(error) = drain_messages(
            &receiver,
            &mut stdout_capture,
            &mut stderr_capture,
            &mut on_line,
        ) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            return Err(error);
        }

        if cancellation.is_requested() {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            let _ = drain_messages(
                &receiver,
                &mut stdout_capture,
                &mut stderr_capture,
                &mut on_line,
            );
            return Err(CommandRunError::Cancelled);
        }

        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => thread::sleep(Duration::from_millis(50)),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err(CommandRunError::Wait(format!(
                    "Could not poll external command: {error}"
                )));
            }
        }
    };

    join_reader(stdout_reader, "stdout")?;
    join_reader(stderr_reader, "stderr")?;
    if let Some(error) = drain_messages(
        &receiver,
        &mut stdout_capture,
        &mut stderr_capture,
        &mut on_line,
    ) {
        return Err(error);
    }

    Ok(CommandOutput {
        success: status.success(),
        exit_code: status.code(),
        stdout: stdout_capture,
        stderr: stderr_capture,
    })
}

#[derive(Debug)]
enum StreamMessage {
    Line(CommandStream, String),
    Error(CommandStream, String),
}

fn spawn_reader<R>(
    reader: R,
    stream: CommandStream,
    sender: Sender<StreamMessage>,
) -> std::io::Result<JoinHandle<()>>
where
    R: Read + Send + 'static,
{
    thread::Builder::new()
        .name(match stream {
            CommandStream::Stdout => "storyteller-command-stdout".into(),
            CommandStream::Stderr => "storyteller-command-stderr".into(),
        })
        .spawn(move || read_stream(reader, stream, sender))
}

fn read_stream<R>(reader: R, stream: CommandStream, sender: Sender<StreamMessage>)
where
    R: Read,
{
    let mut reader = BufReader::new(reader);
    loop {
        let mut bytes = Vec::new();
        match reader.read_until(b'\n', &mut bytes) {
            Ok(0) => return,
            Ok(_) => {
                while bytes
                    .last()
                    .is_some_and(|byte| matches!(*byte, b'\r' | b'\n'))
                {
                    bytes.pop();
                }
                let line = String::from_utf8_lossy(&bytes).into_owned();
                if sender.send(StreamMessage::Line(stream, line)).is_err() {
                    return;
                }
            }
            Err(error) => {
                let _ = sender.send(StreamMessage::Error(stream, error.to_string()));
                return;
            }
        }
    }
}

fn drain_messages<F>(
    receiver: &Receiver<StreamMessage>,
    stdout_capture: &mut String,
    stderr_capture: &mut String,
    on_line: &mut F,
) -> Option<CommandRunError>
where
    F: FnMut(CommandStream, &str),
{
    let mut first_error = None;
    for message in receiver.try_iter() {
        match message {
            StreamMessage::Line(stream, line) => {
                match stream {
                    CommandStream::Stdout => append_capture(stdout_capture, &line),
                    CommandStream::Stderr => append_capture(stderr_capture, &line),
                }
                on_line(stream, &line);
            }
            StreamMessage::Error(stream, error) => {
                if first_error.is_none() {
                    first_error = Some(CommandRunError::Stream(format!(
                        "Could not read external command {}: {error}",
                        stream_name(stream)
                    )));
                }
            }
        }
    }
    first_error
}

fn append_capture(target: &mut String, line: &str) {
    if target.len() >= MAX_CAPTURE_BYTES {
        return;
    }
    let needed = line.len().saturating_add(1);
    if target.len().saturating_add(needed) <= MAX_CAPTURE_BYTES {
        target.push_str(line);
        target.push('\n');
    }
}

fn join_reader(reader: JoinHandle<()>, label: &str) -> Result<(), CommandRunError> {
    reader.join().map_err(|_| {
        CommandRunError::Stream(format!("External command {label} reader thread panicked."))
    })
}

fn stream_name(stream: CommandStream) -> &'static str {
    match stream {
        CommandStream::Stdout => "stdout",
        CommandStream::Stderr => "stderr",
    }
}
