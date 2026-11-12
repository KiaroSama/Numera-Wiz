//! Complete JSON validation with bounded location retention; no raw findings log.
use crate::{PrivateResult, Result, ScanFailure, classifier::Counts, require};
use serde::de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use std::{fmt, fs::OpenOptions, io::Write, path::Path};

#[derive(Default)]
pub struct Row {
    pub rule: String,
    pub file: String,
    pub line: String,
    pub commit: String,
}
pub struct Report {
    pub count: usize,
    pub rows: Vec<Row>,
}
/// Validate unknown JSON recursively without retaining its values. In particular,
/// use Serde's normal string/key decoder, not IgnoredAny's weaker skip path.
/// The bounded input and serde_json's maintained recursion limit bound this walk.
struct ValidatedDiscard;
impl<'de> Deserialize<'de> for ValidatedDiscard {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct DiscardVisitor;
        impl<'de> Visitor<'de> for DiscardVisitor {
            type Value = ValidatedDiscard;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("valid JSON value")
            }
            fn visit_bool<E: de::Error>(self, _: bool) -> std::result::Result<Self::Value, E> {
                Ok(ValidatedDiscard)
            }
            fn visit_i64<E: de::Error>(self, _: i64) -> std::result::Result<Self::Value, E> {
                Ok(ValidatedDiscard)
            }
            fn visit_u64<E: de::Error>(self, _: u64) -> std::result::Result<Self::Value, E> {
                Ok(ValidatedDiscard)
            }
            fn visit_f64<E: de::Error>(self, _: f64) -> std::result::Result<Self::Value, E> {
                Ok(ValidatedDiscard)
            }
            fn visit_unit<E: de::Error>(self) -> std::result::Result<Self::Value, E> {
                Ok(ValidatedDiscard)
            }
            fn visit_str<E: de::Error>(self, _: &str) -> std::result::Result<Self::Value, E> {
                Ok(ValidatedDiscard)
            }
            fn visit_seq<A: SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                while seq.next_element::<ValidatedDiscard>()?.is_some() {}
                Ok(ValidatedDiscard)
            }
            fn visit_map<M: MapAccess<'de>>(
                self,
                mut map: M,
            ) -> std::result::Result<Self::Value, M::Error> {
                while map.next_key::<String>()?.is_some() {
                    map.next_value::<ValidatedDiscard>()?;
                }
                Ok(ValidatedDiscard)
            }
        }
        deserializer.deserialize_any(DiscardVisitor)
    }
}
impl<'de> Deserialize<'de> for Row {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct RowVisitor;
        impl<'de> Visitor<'de> for RowVisitor {
            type Value = Row;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("finding object")
            }
            fn visit_map<M: MapAccess<'de>>(
                self,
                mut map: M,
            ) -> std::result::Result<Row, M::Error> {
                let mut row = Row::default();
                let mut seen = [false; 4];
                while let Some(key) = map.next_key::<String>()? {
                    let index = match key.as_str() {
                        "RuleID" => 0,
                        "File" => 1,
                        "StartLine" => 2,
                        "Commit" => 3,
                        _ => {
                            map.next_value::<ValidatedDiscard>()?;
                            continue;
                        }
                    };
                    if seen[index] {
                        return Err(de::Error::custom("duplicate location"));
                    }
                    seen[index] = true;
                    match index {
                        0 => row.rule = map.next_value::<String>()?,
                        1 => row.file = map.next_value::<String>()?,
                        2 => {
                            // Typed integer decoding cannot accept arbitrary_precision's
                            // internal Number-map representation as a JSON object.
                            row.line = map.next_value::<u64>()?.to_string();
                        }
                        _ => row.commit = map.next_value::<String>()?,
                    }
                }
                Ok(row)
            }
        }
        deserializer.deserialize_map(RowVisitor)
    }
}
pub fn parse(bytes: &[u8]) -> Result<Report> {
    require(
        bytes.len() as u64 <= crate::private::REPORT_LIMIT,
        "SECURITY_REPORT_OVERSIZE",
    )?;
    let text = std::str::from_utf8(bytes).private("SECURITY_REPORT_INVALID")?;
    struct ReportVisitor;
    impl<'de> Visitor<'de> for ReportVisitor {
        type Value = Report;
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("finding array")
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<Report, A::Error> {
            let mut report = Report {
                count: 0,
                rows: Vec::with_capacity(200),
            };
            while let Some(row) = seq.next_element::<Row>()? {
                report.count = report
                    .count
                    .checked_add(1)
                    .ok_or_else(|| de::Error::custom("count overflow"))?;
                if report.rows.len() < 200 {
                    report.rows.push(row);
                }
            }
            Ok(report)
        }
    }
    let mut deserializer = serde_json::Deserializer::from_str(text);
    let report = deserializer
        .deserialize_seq(ReportVisitor)
        .private("SECURITY_REPORT_INVALID")?;
    deserializer.end().private("SECURITY_REPORT_INVALID")?;
    Ok(report)
}
pub fn agreement(status: Option<i32>, report: &Report, history: usize, quiet: bool) -> Result<()> {
    require(
        quiet
            && history > 0
            && matches!(status, Some(0 | 2))
            && (status == Some(2)) == (report.count > 0),
        "SECURITY_RESULT_DISAGREEMENT",
    )
}
/// Second maintained-Serde pass over the same immutable, already agreed report.
/// A callback failure keeps its fixed category; malformed JSON never yields totals.
pub fn visit_rows(bytes: &[u8], mut callback: impl FnMut(Row) -> Result<()>) -> Result<usize> {
    require(
        bytes.len() as u64 <= crate::private::REPORT_LIMIT,
        "SECURITY_REPORT_OVERSIZE",
    )?;
    let text = std::str::from_utf8(bytes).private("SECURITY_REPORT_INVALID")?;
    struct Rows<'a, F> {
        callback: &'a mut F,
        failure: &'a mut Option<ScanFailure>,
    }
    impl<'de, F: FnMut(Row) -> Result<()>> Visitor<'de> for Rows<'_, F> {
        type Value = usize;
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("finding array")
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<usize, A::Error> {
            let mut count = 0usize;
            while let Some(row) = seq.next_element::<Row>()? {
                if let Err(error) = (self.callback)(row) {
                    *self.failure = Some(error);
                    return Err(de::Error::custom("classification interrupted"));
                }
                count = count
                    .checked_add(1)
                    .ok_or_else(|| de::Error::custom("count overflow"))?;
            }
            Ok(count)
        }
    }
    let mut failure = None;
    let mut deserializer = serde_json::Deserializer::from_str(text);
    let result = deserializer.deserialize_seq(Rows {
        callback: &mut callback,
        failure: &mut failure,
    });
    if let Some(error) = failure {
        return Err(error);
    }
    let count = result.private("SECURITY_REPORT_INVALID")?;
    deserializer.end().private("SECURITY_REPORT_INVALID")?;
    Ok(count)
}
pub fn escape(value: &str) -> String {
    let mut output = String::new();
    for c in value.chars() {
        match c {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '"' => output.push_str("&quot;"),
            '\'' => output.push_str("&#x27;"),
            '|' => output.push_str("&#124;"),
            '\r' | '\n' => output.push(' '),
            _ => output.push(c),
        }
    }
    output
}
pub fn summary(report: &Report, pull_request: bool) -> String {
    let scope = if pull_request {
        "Commits introduced by this pull request, including merge diffs"
    } else {
        "All fetched branches and tags, including full history and merge diffs"
    };
    let mut output = format!(
        "## Gitleaks\n\nScanner: **8.30.1**\n\nScope: **{scope}**\n\nRules: Pinned built-in Gitleaks rules with standard exclusions. Full history describes revision coverage, not universal byte or credential coverage.\n\n"
    );
    if report.count == 0 {
        output.push_str("No matching secrets were found in the scanned Git history.\n");
    } else {
        output.push_str(&format!("**{} potential secrets detected.** Review the locations below; rotate any real exposed credentials.\n\nSecret values, source snippets, and commit messages are never included in this summary.\n\n| Rule | File | Line | Commit |\n| --- | --- | --- | --- |\n", report.count));
        for row in &report.rows {
            output.push_str(&format!(
                "| <code>{}</code> | <code>{}</code> | <code>{}</code> | <code>{}</code> |\n",
                escape(&row.rule),
                escape(&row.file),
                escape(&row.line),
                escape(&row.commit)
            ));
        }
        if report.count > 200 {
            output.push_str(&format!("\nShowing 200 of {} findings. Repeat the same version locally for the complete report.\n", report.count));
        }
    }
    output
}
pub fn write_summary(path: Option<&Path>, report: &Report, pull_request: bool) -> Result<()> {
    publish(path, &summary(report, pull_request))
}
pub fn classified_summary(report: &Report, pull_request: bool, counts: &Counts) -> Result<String> {
    require(
        report.count == counts.total
            && counts.reviewed.checked_add(counts.unreviewed) == Some(counts.total),
        "SECURITY_CLASSIFIER_COUNT_DISAGREEMENT",
    )?;
    let mut text = summary(report, pull_request);
    if counts.total > 0 {
        // Keep the original four-column table and every retained row intact.
        let position = text
            .find("| Rule | File | Line | Commit |")
            .ok_or(ScanFailure("SECURITY_SUMMARY_WRITE_FAILED"))?;
        text.insert_str(
            position,
            &format!(
                "Total: **{}**. Reviewed public data: **{}**. Unreviewed: **{}**.\n\n{}\n\n",
                counts.total,
                counts.reviewed,
                counts.unreviewed,
                if counts.unreviewed == 0 {
                    "All detected findings match reviewed public data."
                } else {
                    "Unreviewed findings require review."
                }
            ),
        );
    }
    Ok(text)
}
pub fn publish(path: Option<&Path>, text: &str) -> Result<()> {
    if let Some(path) = path.filter(|p| !p.as_os_str().is_empty()) {
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .and_then(|mut f| f.write_all(text.as_bytes()))
            .private("SECURITY_SUMMARY_WRITE_FAILED")?;
    }
    Ok(())
}
