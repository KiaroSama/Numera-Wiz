use quick_xml::{
    Reader, XmlVersion,
    events::{BytesStart, Event},
};
use std::collections::BTreeMap;

const MAX_DEPTH: usize = 24;
const MAX_NODES: usize = 4096;

#[derive(Default)]
pub(super) struct Element {
    pub name: String,
    pub attrs: BTreeMap<String, String>,
    pub text: String,
    pub elements: Vec<Element>,
}

impl Element {
    pub fn child(&self, name: &str) -> std::result::Result<Option<&Self>, ()> {
        let mut matches = self.elements.iter().filter(|node| node.name == name);
        let first = matches.next();
        if matches.next().is_some() {
            return Err(());
        }
        Ok(first)
    }
}

fn node(start: &BytesStart<'_>, reader: &Reader<&[u8]>) -> std::result::Result<Element, ()> {
    let mut node = Element {
        name: std::str::from_utf8(start.name().as_ref())
            .map_err(|_| ())?
            .into(),
        ..Element::default()
    };
    for attr in start.attributes() {
        let attr = attr.map_err(|_| ())?;
        let key = std::str::from_utf8(attr.key.as_ref()).map_err(|_| ())?;
        let value = attr
            .decoded_and_normalized_value(XmlVersion::Implicit1_0, reader.decoder())
            .map_err(|_| ())?;
        node.attrs.insert(key.into(), value.into_owned());
    }
    Ok(node)
}

/// Parse UTF-8 XML with a bounded tree and at most one inert, exact doctype name.
pub(super) fn parse(text: &str, doctype: &str) -> std::result::Result<Element, ()> {
    if text
        .chars()
        .any(|character| character < ' ' && !matches!(character, '\t' | '\n' | '\r'))
    {
        return Err(());
    }
    let mut reader = Reader::from_str(text);
    let mut stack = vec![Element::default()];
    let mut seen_doctype = false;
    let mut seen_declaration = false;
    let mut count = 0;
    loop {
        match reader.read_event().map_err(|_| ())? {
            Event::Start(start) => {
                count += 1;
                if count > MAX_NODES || stack.len() >= MAX_DEPTH {
                    return Err(());
                }
                stack.push(node(&start, &reader)?);
            }
            Event::Empty(start) => {
                count += 1;
                if count > MAX_NODES || stack.len() >= MAX_DEPTH {
                    return Err(());
                }
                stack
                    .last_mut()
                    .ok_or(())?
                    .elements
                    .push(node(&start, &reader)?);
            }
            Event::End(_) => {
                let finished = stack.pop().ok_or(())?;
                stack.last_mut().ok_or(())?.elements.push(finished);
            }
            Event::Text(text) => stack
                .last_mut()
                .ok_or(())?
                .text
                .push_str(&text.xml10_content().map_err(|_| ())?),
            Event::CData(text) => stack
                .last_mut()
                .ok_or(())?
                .text
                .push_str(&text.xml10_content().map_err(|_| ())?),
            Event::GeneralRef(value) => {
                let text = &mut stack.last_mut().ok_or(())?.text;
                if let Some(character) = value.resolve_char_ref().map_err(|_| ())? {
                    text.push(character);
                } else {
                    let name = std::str::from_utf8(value.as_ref()).map_err(|_| ())?;
                    text.push_str(quick_xml::escape::resolve_predefined_entity(name).ok_or(())?);
                }
            }
            Event::DocType(name) => {
                if seen_doctype
                    || stack.len() != 1
                    || count != 0
                    || std::str::from_utf8(name.as_ref()).map_err(|_| ())?.trim() != doctype
                {
                    return Err(());
                }
                seen_doctype = true;
            }
            Event::Decl(decl) => {
                if seen_declaration
                    || seen_doctype
                    || count != 0
                    || decl
                        .encoding()
                        .transpose()
                        .map_err(|_| ())?
                        .is_some_and(|encoding| !encoding.as_ref().eq_ignore_ascii_case(b"utf-8"))
                {
                    return Err(());
                }
                seen_declaration = true;
            }
            Event::Comment(_) => {}
            Event::PI(_) => return Err(()),
            Event::Eof => break,
        }
    }
    if stack.len() != 1 {
        return Err(());
    }
    let root = stack.pop().ok_or(())?;
    if !root.text.trim().is_empty() || root.elements.len() != 1 {
        return Err(());
    }
    root.elements.into_iter().next().ok_or(())
}
