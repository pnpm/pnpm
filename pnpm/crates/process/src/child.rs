use crate::{
    ExitStatus, host,
    pipe::{ReadPipe, WritePipe},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    io::{self, Read},
    ops::{Deref, DerefMut},
    sync::Arc,
    thread,
};
use tokio::io::AsyncReadExt;

pub struct Child {
    pub stdin: Option<WritePipe>,
    pub stdout: Option<ReadPipe>,
    pub stderr: Option<ReadPipe>,
    process: Arc<ProcessHandle>,
    status: Option<ExitStatus>,
}

pub(crate) struct ProcessHandle {
    pub(crate) handle: u32,
    pid: u32,
}

impl Drop for ProcessHandle {
    fn drop(&mut self) {
        if let Err(error) =
            host::request(&json!({"operation":"process.release", "handle":self.handle}))
        {
            eprintln!("Failed to release child process: {error}");
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Output {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl Child {
    pub(crate) fn from_host(value: Value, piped_stdin: bool) -> io::Result<Self> {
        #[derive(Deserialize)]
        struct Spawned {
            handle: u32,
            pid: u32,
            stdout: Option<u32>,
            stderr: Option<u32>,
        }
        let result: Spawned = serde_json::from_value(value).map_err(io::Error::other)?;
        let process = Arc::new(ProcessHandle { handle: result.handle, pid: result.pid });
        Ok(Self {
            stdin: piped_stdin.then(|| WritePipe::new(Arc::clone(&process))),
            stdout: result.stdout.map(ReadPipe::new),
            stderr: result.stderr.map(ReadPipe::new),
            process,
            status: None,
        })
    }

    #[must_use]
    pub fn id(&self) -> u32 {
        self.process.pid
    }

    pub fn kill(&mut self) -> io::Result<()> {
        host::request(
            &json!({"operation":"process.kill", "handle":self.process.handle, "signal":"SIGKILL"}),
        )?;
        Ok(())
    }

    pub fn wait(&mut self) -> io::Result<ExitStatus> {
        drop(self.stdin.take());
        if let Some(status) = self.status {
            return Ok(status);
        }
        let result =
            host::request(&json!({"operation":"process.wait", "handle":self.process.handle}))?;
        let status = ExitStatus::from_host(result)?;
        self.status = Some(status);
        Ok(status)
    }

    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        if let Some(status) = self.status {
            return Ok(Some(status));
        }
        let result =
            host::request(&json!({"operation":"process.tryWait", "handle":self.process.handle}))?;
        if result.is_null() {
            return Ok(None);
        }
        let status = ExitStatus::from_host(result)?;
        self.status = Some(status);
        Ok(Some(status))
    }

    pub fn wait_with_output(mut self) -> io::Result<Output> {
        drop(self.stdin.take());
        let stdout = self.stdout.take();
        let stderr = self.stderr.take();
        thread::scope(|scope| {
            let stdout = scope.spawn(move || read_output(stdout));
            let stderr = scope.spawn(move || read_output(stderr));
            let status = self.wait();
            let stdout = stdout.join().expect("child stdout reader panicked")?;
            let stderr = stderr.join().expect("child stderr reader panicked")?;
            Ok(Output { status: status?, stdout, stderr })
        })
    }
}

fn read_output(pipe: Option<ReadPipe>) -> io::Result<Vec<u8>> {
    let mut output = Vec::new();
    if let Some(mut pipe) = pipe {
        Read::read_to_end(&mut pipe, &mut output)?;
    }
    Ok(output)
}

pub struct AsyncChild {
    child: Child,
    kill_on_drop: bool,
}

impl AsyncChild {
    pub(crate) fn new(child: Child, kill_on_drop: bool) -> Self {
        Self { child, kill_on_drop }
    }

    #[must_use]
    pub fn id(&self) -> Option<u32> {
        self.child.status.is_none().then(|| self.child.id())
    }

    pub fn start_kill(&mut self) -> io::Result<()> {
        self.child.kill()
    }

    pub async fn kill(&mut self) -> io::Result<()> {
        self.start_kill()?;
        self.wait().await?;
        Ok(())
    }

    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.child.try_wait()
    }

    pub async fn wait(&mut self) -> io::Result<ExitStatus> {
        drop(self.child.stdin.take());
        if let Some(status) = self.child.status {
            return Ok(status);
        }
        let result = host::request_async(
            &json!({"operation":"process.wait", "handle":self.child.process.handle}),
        )
        .await?;
        let status = ExitStatus::from_host(result)?;
        self.child.status = Some(status);
        Ok(status)
    }

    pub async fn wait_with_output(mut self) -> io::Result<Output> {
        drop(self.child.stdin.take());
        let stdout = self.child.stdout.take();
        let stderr = self.child.stderr.take();
        let (status, stdout, stderr) =
            tokio::try_join!(self.wait(), read_output_async(stdout), read_output_async(stderr))?;
        Ok(Output { status, stdout, stderr })
    }
}

async fn read_output_async(pipe: Option<ReadPipe>) -> io::Result<Vec<u8>> {
    let mut output = Vec::new();
    if let Some(mut pipe) = pipe {
        AsyncReadExt::read_to_end(&mut pipe, &mut output).await?;
    }
    Ok(output)
}

impl Deref for AsyncChild {
    type Target = Child;
    fn deref(&self) -> &Child {
        &self.child
    }
}

impl DerefMut for AsyncChild {
    fn deref_mut(&mut self) -> &mut Child {
        &mut self.child
    }
}

impl Drop for AsyncChild {
    fn drop(&mut self) {
        if self.kill_on_drop
            && self.child.status.is_none()
            && let Err(error) = self.child.kill()
        {
            eprintln!("Failed to kill child process: {error}");
        }
    }
}
