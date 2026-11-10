use std::{
    fmt::{self, Write as _},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tracing::{
    Event, Level, Metadata, Subscriber,
    field::{Field, Visit},
    span::{Attributes, Id, Record},
};

#[derive(Clone)]
struct RecordedEvents(Arc<Mutex<Vec<String>>>);

impl Subscriber for RecordedEvents {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.level() == &Level::WARN
    }

    fn new_span(&self, _attributes: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }

    fn record(&self, _span: &Id, _values: &Record<'_>) {}
    fn record_follows_from(&self, _span: &Id, _follows: &Id) {}
    fn enter(&self, _span: &Id) {}
    fn exit(&self, _span: &Id) {}

    fn event(&self, event: &Event<'_>) {
        let mut fields = String::new();
        event.record(&mut RecordedFields(&mut fields));
        self.0.lock().unwrap().push(fields);
    }
}

struct RecordedFields<'record>(&'record mut String);

impl Visit for RecordedFields<'_> {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        write!(self.0, "{}={value:?} ", field.name()).unwrap();
    }
}

#[tokio::test(start_paused = true)]
async fn last_attempt_timeout_preserves_driver_cause_and_redacts_retry_logs() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let _guard = tracing::subscriber::set_default(RecordedEvents(Arc::clone(&events)));
    let attempts = AtomicUsize::new(0);
    let count = &attempts;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
    let target =
        "postgres://example:example-secret@database.example.test/registry?token=another-secret";
    let error = super::connect_with_retry(target, deadline, move || async move {
        if count.fetch_add(1, Ordering::Relaxed) == 0 {
            Err(sqlx::Error::Io(std::io::ErrorKind::ConnectionRefused.into()))
        } else {
            std::future::pending::<std::result::Result<(), sqlx::Error>>().await
        }
    })
    .await
    .unwrap_err()
    .to_string();
    assert_eq!(attempts.load(Ordering::Relaxed), 2);
    assert!(tokio::time::Instant::now() >= deadline);
    let events = events.lock().unwrap();
    assert_eq!(events.len(), 1);
    assert!(events[0].contains("retrying"), "{events:?}");
    assert!(events[0].contains("delay="), "{events:?}");
    for message in [&events[0], &error] {
        assert!(message.contains("database.example.test/registry"), "{message}");
        assert!(message.contains("refused"), "{message}");
        assert!(!message.contains("example-secret"), "{message}");
        assert!(!message.contains("another-secret"), "{message}");
    }
}
