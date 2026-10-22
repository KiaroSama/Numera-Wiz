//! Primary severity names and UTC timestamps shared by native entry points.
use std::fmt;
use tracing::{Event,Level,Subscriber};
use tracing_subscriber::{fmt::{FmtContext,FormatEvent,FormatFields,format::Writer},registry::LookupSpan};
pub struct PrimaryFormatter;
impl<S,N> FormatEvent<S,N> for PrimaryFormatter
where S:Subscriber+for<'lookup>LookupSpan<'lookup>,N:for<'writer>FormatFields<'writer>+'static {
    fn format_event(&self,ctx:&FmtContext<'_,S,N>,mut writer:Writer<'_>,event:&Event<'_>)->fmt::Result{
        let severity=match *event.metadata().level(){Level::ERROR=>"ERROR",Level::WARN=>"WARNING",Level::INFO=>"INFO",Level::DEBUG|Level::TRACE=>"DEBUG"};
        write!(writer,"[{}] [{}] ",chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis,true),severity)?;
        ctx.field_format().format_fields(writer.by_ref(),event)?;
        writeln!(writer)
    }
}
