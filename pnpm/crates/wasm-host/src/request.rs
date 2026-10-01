use crate::bindings;
use futures_util::task::AtomicWaker;
use serde::Deserialize;
use serde_json::Value;
use std::{
    collections::HashMap,
    error::Error,
    fmt,
    future::Future,
    pin::Pin,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll},
};

static REQUESTS: Mutex<Option<HashMap<u32, Arc<AtomicWaker>>>> = Mutex::new(None);
static COMPLETION_PUMP: OnceLock<Result<(), String>> = OnceLock::new();
static HOST_CLOSED: AtomicBool = AtomicBool::new(false);
const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Deserialize)]
pub struct HostError {
    pub message: String,
    pub code: Option<String>,
}

impl fmt::Display for HostError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for HostError {}

/// Closes an owned host resource and waits until cleanup completes.
pub fn close_resource(handle: u32) -> Result<(), HostError> {
    // SAFETY: the supervisor only accesses its own resource handle table.
    if unsafe { bindings::resource_close(handle) } == 0 {
        Ok(())
    } else {
        Err(HostError::new("Failed to close WASM host resource"))
    }
}

impl HostError {
    fn new(message: impl Into<String>) -> Self {
        Self { message: message.into(), code: None }
    }

    fn closed() -> Self {
        Self { message: "WASM host supervisor closed".into(), code: Some("ABORT_ERR".into()) }
    }
}

/// An in-flight operation. Dropping it cancels its host-owned resources.
#[must_use]
pub struct Request {
    identifier: Option<u32>,
    waker: Arc<AtomicWaker>,
}

/// Starts a host operation without waiting for its result.
///
/// The WASI guest must run in a worker separate from the Node supervisor.
/// The build must support threads; one completion thread wakes all requests.
pub fn request(message: &Value) -> Result<Request, HostError> {
    ensure_completion_pump()?;
    if HOST_CLOSED.load(Ordering::Acquire) {
        return Err(HostError::closed());
    }
    let payload = serde_json::to_vec(message).map_err(|error| HostError::new(error.to_string()))?;
    // SAFETY: the import copies this valid slice synchronously and retains no pointer.
    let identifier = unsafe { bindings::operation_start(payload.as_ptr(), payload.len()) };
    if identifier <= 0 {
        return Err(HostError::new("Failed to start WASM host operation"));
    }
    let identifier = identifier as u32;
    let waker = Arc::new(AtomicWaker::new());
    REQUESTS
        .lock()
        .expect("host request registry poisoned")
        .get_or_insert_with(HashMap::new)
        .insert(identifier, Arc::clone(&waker));
    Ok(Request { identifier: Some(identifier), waker })
}

impl Future for Request {
    type Output = Result<Value, HostError>;

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let identifier = self.identifier.expect("host request polled after completion");
        self.waker.register(context.waker());
        if HOST_CLOSED.load(Ordering::Acquire) {
            self.release();
            return Poll::Ready(Err(HostError::closed()));
        }
        // SAFETY: this import only queries a supervisor-owned operation identifier.
        let length = unsafe { bindings::response_len(identifier) };
        if length == -1 {
            return Poll::Pending;
        }
        let response = read_response(identifier, length);
        self.release();
        Poll::Ready(response)
    }
}

impl Request {
    fn release(&mut self) {
        let Some(identifier) = self.identifier.take() else { return };
        if let Some(requests) = REQUESTS
            .lock()
            .expect("host request registry poisoned")
            .as_mut()
        {
            requests.remove(&identifier);
        }
        // SAFETY: cancellation accepts consumed IDs and never accesses guest memory.
        unsafe { bindings::operation_cancel(identifier) };
    }
}

impl Drop for Request {
    fn drop(&mut self) {
        self.release();
    }
}

fn read_response(identifier: u32, length: i32) -> Result<Value, HostError> {
    let length = usize::try_from(length).map_err(|_| HostError::new("Invalid host response"))?;
    if length > MAX_RESPONSE_BYTES {
        return Err(HostError::new("WASM host response exceeds 16 MiB"));
    }
    let mut payload = vec![0; length];
    // SAFETY: the import copies at most payload.len() bytes into this exclusive buffer.
    let copied =
        unsafe { bindings::response_read(identifier, payload.as_mut_ptr(), payload.len()) };
    if usize::try_from(copied) != Ok(length) {
        return Err(HostError::new("Failed to read WASM host response"));
    }
    let response: Response = serde_json::from_slice(&payload)
        .map_err(|error| HostError::new(format!("Invalid WASM host response: {error}")))?;
    match (response.ok, response.value, response.error) {
        (true, value, None) => Ok(value),
        (false, Value::Null, Some(error)) => Err(error),
        _ => Err(HostError::new("Invalid WASM host response envelope")),
    }
}

#[derive(Deserialize)]
struct Response {
    ok: bool,
    #[serde(default)]
    value: Value,
    error: Option<HostError>,
}

fn ensure_completion_pump() -> Result<(), HostError> {
    COMPLETION_PUMP
        .get_or_init(|| {
            std::thread::Builder::new()
                .name("pnpm-host-completions".into())
                .spawn(pump_completions)
                .map(|_| ())
                .map_err(|error| error.to_string())
        })
        .as_ref()
        .copied()
        .map_err(|error| HostError::new(error.clone()))
}

fn pump_completions() {
    loop {
        // SAFETY: only this dedicated thread blocks in the supervisor's completion queue.
        let identifier = unsafe { bindings::wait_completion() };
        if identifier <= 0 {
            HOST_CLOSED.store(true, Ordering::Release);
            wake_all();
            return;
        }
        let waker = REQUESTS
            .lock()
            .expect("host request registry poisoned")
            .as_ref()
            .and_then(|requests| {
                requests
                    .get(&(identifier as u32))
                    .map(Arc::clone)
            });
        if let Some(waker) = waker {
            waker.wake();
        }
    }
}

fn wake_all() {
    let wakers: Vec<_> = REQUESTS
        .lock()
        .expect("host request registry poisoned")
        .as_ref()
        .map(|requests| {
            requests
                .values()
                .map(Arc::clone)
                .collect()
        })
        .unwrap_or_default();
    for waker in wakers {
        waker.wake();
    }
}
