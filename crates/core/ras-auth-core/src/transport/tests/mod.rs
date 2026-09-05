use super::csrf::DEFAULT_CSRF_HEADER;
use super::*;
use http::{HeaderMap, HeaderValue, header::HeaderName};

fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
    let mut headers = HeaderMap::new();
    for (name, value) in pairs {
        headers.append(
            HeaderName::from_bytes(name.as_bytes()).unwrap(),
            HeaderValue::from_str(value).unwrap(),
        );
    }
    headers
}

/// Minimal `tracing` subscriber that records the messages of `WARN` events.
/// Kept dependency-free (no `tracing-subscriber`) since it only needs to
/// capture a handful of events for the A1 regression tests.
struct WarnCapture(std::sync::Mutex<Vec<String>>);

impl tracing::Subscriber for WarnCapture {
    fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
        *metadata.level() <= tracing::Level::WARN
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        struct Msg(String);
        impl tracing::field::Visit for Msg {
            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                self.0.push_str(&format!("{}={:?} ", field.name(), value));
            }
        }
        let mut msg = Msg(String::new());
        event.record(&mut msg);
        self.0.lock().unwrap().push(msg.0);
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

fn capture_warnings(f: impl FnOnce()) -> Vec<String> {
    let capture = std::sync::Arc::new(WarnCapture(std::sync::Mutex::new(Vec::new())));
    tracing::subscriber::with_default(capture.clone(), f);
    capture.0.lock().unwrap().clone()
}

mod config;
mod cookie;
mod credential;
mod csrf;
mod redaction;
