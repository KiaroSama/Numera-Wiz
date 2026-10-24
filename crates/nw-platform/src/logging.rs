//! Primary severity names and UTC timestamps shared by native entry points.
mod persisted;
pub use persisted::{activate, bind_owner, flush, status};
pub(crate) use persisted::JsonWriter;
use std::{collections::BTreeMap, fmt};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::{
    fmt::{FmtContext, FormatEvent, FormatFields, format::Writer},
    registry::LookupSpan,
};
pub struct PrimaryFormatter;
struct LimitedText { text: String, truncated: bool }
impl fmt::Write for LimitedText {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        let remaining = 4096usize.saturating_sub(self.text.len());
        let mut end = text.len().min(remaining);
        while !text.is_char_boundary(end) { end -= 1; }
        self.text.push_str(&text[..end]); self.truncated |= end != text.len(); Ok(())
    }
}
#[derive(Default)]
struct SafeFields { values: BTreeMap<String, String>, truncated: bool }
impl tracing::field::Visit for SafeFields {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn fmt::Debug) {
        use fmt::Write as _;
        if self.values.len() >= 32 { self.truncated = true; return; }
        if persisted::sensitive(field.name()) { self.values.insert(field.name().into(), "<redacted>".into()); return; }
        let mut text = LimitedText { text: String::new(), truncated: false };
        let _ = write!(&mut text, "{value:?}"); self.truncated |= text.truncated;
        self.values.insert(field.name().into(), text.text);
    }
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        use fmt::Write as _;
        if self.values.len() >= 32 { self.truncated = true; return; }
        if persisted::sensitive(field.name()) { self.values.insert(field.name().into(), "<redacted>".into()); return; }
        let mut text = LimitedText { text: String::new(), truncated: false };
        let _ = text.write_str(value); self.truncated |= text.truncated;
        self.values.insert(field.name().into(), text.text);
    }
}
impl<S, N> FormatEvent<S, N> for PrimaryFormatter
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
    N: for<'writer> FormatFields<'writer> + 'static,
{
    fn format_event(
        &self,
        _ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        let severity = match *event.metadata().level() {
            Level::ERROR => "ERROR",
            Level::WARN => "WARNING",
            Level::INFO => "INFO",
            Level::DEBUG | Level::TRACE => "DEBUG",
        };
        write!(
            writer,
            "[{}] [{}] ",
            chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            severity
        )?;
        let mut fields = SafeFields::default(); event.record(&mut fields);
        if let Some(message) = fields.values.remove("message") { write!(writer, "{}", message.escape_debug())?; }
        for (key, value) in fields.values { write!(writer, " {key}={}", value.escape_debug())?; }
        if fields.truncated { write!(writer, " [diagnostic fields truncated]")?; }
        writeln!(writer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::Write, sync::{Arc, Mutex}};
    struct Capture(Arc<Mutex<Vec<u8>>>);
    impl Write for Capture {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> { self.0.lock().unwrap().extend_from_slice(bytes); Ok(bytes.len()) }
        fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
    }
    #[test]
    fn human_diagnostics_redact_credentials_and_escape_injected_lines() {
        let bytes = Arc::new(Mutex::new(Vec::new()));
        let writer = bytes.clone();
        let subscriber = tracing_subscriber::fmt().with_ansi(false).event_format(PrimaryFormatter).with_writer(move || Capture(writer.clone())).finish();
        tracing::subscriber::with_default(subscriber, || {
            tracing::warn!(bearer="unit-secret", token_id="safe-id", request_id="line\n[forged]", "Human diagnostic fixture");
        });
        let text = String::from_utf8(bytes.lock().unwrap().clone()).unwrap();
        assert!(text.contains("[WARNING]"));
        assert!(text.contains("safe-id"));
        assert!(text.contains("<redacted>"));
        assert!(!text.contains("unit-secret"));
        assert_eq!(text.lines().count(), 1);
        assert!(text.contains("\\n[forged]"));
    }
}
