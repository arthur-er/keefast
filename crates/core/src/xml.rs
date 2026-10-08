//! A lossless XML tree for the document inside a Vault file.
//!
//! KDBX clients each model a different part of the format, so keefast keeps the whole document
//! and reads only what it understands (ADR 0002). Everything else (unknown elements and
//! attributes, plugin data, comments) is written back exactly as it was read.

use std::fmt;

use quick_xml::escape::resolve_predefined_entity;
use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, XmlVersion};
use zeroize::Zeroizing;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Element {
    pub(crate) name: String,
    /// In document order, which is preserved on write.
    pub(crate) attributes: Vec<(String, String)>,
    pub(crate) children: Vec<Node>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Node {
    Element(Element),
    Text(String),
    /// The plaintext of an element marked `Protected="True"`. The file holds it encrypted with
    /// the inner random stream, which is applied while reading and writing the file.
    Protected(Secret),
    Comment(String),
}

/// Decrypted bytes that are wiped on drop and never printed.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Secret(Zeroizing<Vec<u8>>);

impl Secret {
    pub(crate) fn new(bytes: Vec<u8>) -> Self {
        Self(Zeroizing::new(bytes))
    }

    pub(crate) fn bytes(&self) -> &[u8] {
        &self.0
    }
}

impl From<Zeroizing<Vec<u8>>> for Secret {
    fn from(bytes: Zeroizing<Vec<u8>>) -> Self {
        Self(bytes)
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret([hidden])")
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub(crate) struct XmlError(String);

impl Element {
    pub(crate) fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            attributes: Vec::new(),
            children: Vec::new(),
        }
    }

    pub(crate) fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    /// Whether this element's content is encrypted with the inner random stream in the file.
    pub(crate) fn is_protected(&self) -> bool {
        self.attribute("Protected")
            .is_some_and(|value| value.eq_ignore_ascii_case("true"))
    }

    pub(crate) fn elements(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(|node| match node {
            Node::Element(element) => Some(element),
            _ => None,
        })
    }

    pub(crate) fn elements_mut(&mut self) -> impl Iterator<Item = &mut Element> {
        self.children.iter_mut().filter_map(|node| match node {
            Node::Element(element) => Some(element),
            _ => None,
        })
    }

    pub(crate) fn child(&self, name: &str) -> Option<&Element> {
        self.elements().find(|element| element.name == name)
    }

    pub(crate) fn child_mut(&mut self, name: &str) -> Option<&mut Element> {
        self.elements_mut().find(|element| element.name == name)
    }

    pub(crate) fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Element> {
        self.elements().filter(move |element| element.name == name)
    }

    /// Removes and returns the first child element called `name`.
    pub(crate) fn take_child(&mut self, name: &str) -> Option<Element> {
        let index = self
            .children
            .iter()
            .position(|node| matches!(node, Node::Element(element) if element.name == name))?;
        match self.children.remove(index) {
            Node::Element(element) => Some(element),
            _ => unreachable!("position matched an element"),
        }
    }

    /// The element's character data; protected content is decoded as UTF-8.
    pub(crate) fn text(&self) -> String {
        let mut text = String::new();
        for node in &self.children {
            match node {
                Node::Text(chunk) => text.push_str(chunk),
                Node::Protected(secret) => text.push_str(&String::from_utf8_lossy(secret.bytes())),
                Node::Element(_) | Node::Comment(_) => {}
            }
        }
        text
    }

    pub(crate) fn set_text(&mut self, text: impl Into<String>) {
        self.children.retain(|node| !matches!(node, Node::Text(_)));
        self.children.push(Node::Text(text.into()));
    }
}

/// Parses an XML document as an XML 1.0 processor would (line endings and attribute whitespace
/// normalised, references resolved), keeping its root element.
///
/// Whitespace-only text between child elements is indentation and is dropped; every other
/// character of text is kept. The declaration, processing instructions and anything outside
/// the root element are dropped.
pub(crate) fn parse(xml: &str) -> Result<Element, XmlError> {
    let xml = xml.strip_prefix('\u{feff}').unwrap_or(xml);
    let mut reader = Reader::from_str(xml);
    reader.config_mut().check_end_names = true;

    let mut stack: Vec<Element> = Vec::new();
    let mut text = String::new();
    let mut root = None;
    let error = |error: quick_xml::Error| XmlError(error.to_string());

    loop {
        let event = reader.read_event().map_err(error)?;
        if !matches!(
            event,
            Event::Text(_) | Event::CData(_) | Event::GeneralRef(_)
        ) {
            flush_text(&mut stack, &mut text);
        }
        match event {
            Event::Start(start) => stack.push(start_element(&start)?),
            Event::Empty(start) => {
                let element = start_element(&start)?;
                close(&mut stack, &mut root, element)?;
            }
            Event::End(_) => {
                let element = stack
                    .pop()
                    .ok_or_else(|| XmlError("unbalanced end tag".into()))?;
                close(&mut stack, &mut root, element)?;
            }
            Event::Text(chunk) => text.push_str(&chunk.xml10_content()),
            Event::CData(chunk) => text.push_str(&chunk.xml10_content()),
            Event::GeneralRef(reference) => {
                if let Some(ch) = reference.resolve_char_ref().map_err(error)? {
                    text.push(ch);
                } else {
                    let name: &str = &reference;
                    let resolved = resolve_predefined_entity(name)
                        .ok_or_else(|| XmlError(format!("unknown entity &{name};")))?;
                    text.push_str(resolved);
                }
            }
            Event::Comment(comment) => {
                if let Some(parent) = stack.last_mut() {
                    parent
                        .children
                        .push(Node::Comment(comment.xml10_content().into_owned()));
                }
            }
            Event::Decl(_) | Event::PI(_) | Event::DocType(_) => {}
            Event::Eof => break,
        }
    }

    if !stack.is_empty() {
        return Err(XmlError("unexpected end of document".into()));
    }
    root.ok_or_else(|| XmlError("the document has no root element".into()))
}

fn start_element(start: &BytesStart<'_>) -> Result<Element, XmlError> {
    let mut element = Element::new(start.name().as_ref());
    for attribute in start.attributes() {
        let attribute = attribute.map_err(|error| XmlError(error.to_string()))?;
        let value = attribute
            .normalized_value(XmlVersion::Implicit1_0)
            .map_err(|error| XmlError(error.to_string()))?;
        element
            .attributes
            .push((attribute.key.as_ref().to_owned(), value.into_owned()));
    }
    Ok(element)
}

/// Text outside the root element is dropped.
fn flush_text(stack: &mut [Element], text: &mut String) {
    let text = std::mem::take(text);
    if let (false, Some(parent)) = (text.is_empty(), stack.last_mut()) {
        parent.children.push(Node::Text(text));
    }
}

fn close(
    stack: &mut [Element],
    root: &mut Option<Element>,
    mut element: Element,
) -> Result<(), XmlError> {
    if element.elements().next().is_some() {
        element
            .children
            .retain(|node| !matches!(node, Node::Text(text) if text.trim().is_empty()));
    }
    match stack.last_mut() {
        Some(parent) => parent.children.push(Node::Element(element)),
        None if root.is_none() => *root = Some(element),
        None => return Err(XmlError("more than one root element".into())),
    }
    Ok(())
}

/// Writes the document with tab indentation, as KeePass does. `protect` turns each protected
/// value into its encoded, encrypted form; it is called in document order.
pub(crate) fn write(root: &Element, protect: &mut dyn FnMut(&[u8]) -> String) -> String {
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"utf-8\" standalone=\"yes\"?>\n");
    write_element(&mut out, root, Some(0), protect);
    out.push('\n');
    out
}

/// `indent` is `None` inside elements that mix text and elements, where whitespace would be
/// content.
fn write_element(
    out: &mut String,
    element: &Element,
    indent: Option<usize>,
    protect: &mut dyn FnMut(&[u8]) -> String,
) {
    out.push('<');
    out.push_str(&element.name);
    for (key, value) in &element.attributes {
        out.push(' ');
        out.push_str(key);
        out.push_str("=\"");
        escape(out, value, true);
        out.push('"');
    }
    if element.children.is_empty() {
        out.push_str("/>");
        return;
    }
    out.push('>');

    let has_text = element
        .children
        .iter()
        .any(|node| matches!(node, Node::Text(_) | Node::Protected(_)));
    let has_elements = element.elements().next().is_some();
    let child_indent = match indent {
        // Indentation is only dropped again between elements, so text and comment-only
        // content stays on one line.
        Some(depth) if has_elements && !has_text => Some(depth + 1),
        _ => None,
    };
    for node in &element.children {
        if let Some(depth) = child_indent {
            out.push('\n');
            out.extend(std::iter::repeat_n('\t', depth));
        }
        match node {
            Node::Element(child) => write_element(out, child, child_indent, protect),
            Node::Text(text) => escape(out, text, false),
            Node::Protected(secret) => out.push_str(&protect(secret.bytes())),
            Node::Comment(comment) => {
                out.push_str("<!--");
                out.extend(comment.chars().filter(|&ch| is_xml_char(ch)));
                out.push_str("-->");
            }
        }
    }
    if let (Some(depth), Some(_)) = (indent, child_indent) {
        out.push('\n');
        out.extend(std::iter::repeat_n('\t', depth));
    }
    out.push_str("</");
    out.push_str(&element.name);
    out.push('>');
}

/// Whether XML 1.0 allows the character in a document at all (its `Char` production): `#x9`,
/// `#xA`, `#xD`, `#x20-#xD7FF`, `#xE000-#xFFFD` and `#x10000-#x10FFFF`. Other control
/// characters can't be written even as character references.
fn is_xml_char(ch: char) -> bool {
    matches!(ch, '\t' | '\n' | '\r' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..='\u{10FFFF}')
}

/// Escapes markup, and the characters an XML processor would otherwise normalise away.
///
/// Characters XML 1.0 doesn't allow (an escape character pasted from a terminal, say) are
/// dropped, as KeePassXC and KeePass2 drop them when they write: KeePassXC's and KeePass2's
/// XML readers reject a document that holds one as corrupt, which would make the Vault file
/// unreadable to them (ADR 0002).
fn escape(out: &mut String, text: &str, attribute: bool) {
    for ch in text.chars().filter(|&ch| is_xml_char(ch)) {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\r' => out.push_str("&#13;"),
            '"' if attribute => out.push_str("&quot;"),
            '\t' if attribute => out.push_str("&#9;"),
            '\n' if attribute => out.push_str("&#10;"),
            _ => out.push(ch),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_protection(_: &[u8]) -> String {
        unreachable!("no protected values in these documents")
    }

    fn round_trip(xml: &str) -> Element {
        let parsed = parse(xml).expect("parses");
        let written = write(&parsed, &mut no_protection);
        let reparsed = parse(&written).expect("reparses");
        assert_eq!(parsed, reparsed, "written as:\n{written}");
        parsed
    }

    #[test]
    fn unknown_elements_and_attributes_survive_in_order() {
        let root = round_trip(
            r#"<KeePassFile><Meta><Future b="2" a="1"><Deep x="y"/></Future><Generator>x</Generator></Meta></KeePassFile>"#,
        );

        let meta = root.child("Meta").unwrap();
        let names: Vec<&str> = meta.elements().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["Future", "Generator"]);
        let future = meta.child("Future").unwrap();
        assert_eq!(
            future.attributes,
            [("b".into(), "2".into()), ("a".into(), "1".into())]
        );
        assert_eq!(future.child("Deep").unwrap().attribute("x"), Some("y"));
    }

    #[test]
    fn indentation_is_dropped_but_text_whitespace_is_kept() {
        let root = round_trip("<a>\n\t<b>  two spaces  </b>\n\t<c> </c>\n\t<d/>\n</a>");

        assert_eq!(root.child("b").unwrap().text(), "  two spaces  ");
        assert_eq!(root.child("c").unwrap().text(), " ");
        assert_eq!(root.children.len(), 3);
    }

    #[test]
    fn references_and_line_endings_are_resolved_as_an_xml_processor_would() {
        let root = round_trip(
            "<a><b>1 &lt; 2 &amp;&amp; &#x263A; &quot;x&quot;</b><c>line\r\nnext&#13;&#10;kept</c><d k=\"tab&#9;x\ny\"/></a>",
        );

        assert_eq!(root.child("b").unwrap().text(), "1 < 2 && \u{263A} \"x\"");
        assert_eq!(root.child("c").unwrap().text(), "line\nnext\r\nkept");
        assert_eq!(root.child("d").unwrap().attribute("k"), Some("tab\tx y"));
    }

    #[test]
    fn cdata_and_comments_survive() {
        let root =
            round_trip("<a><!-- note --><b><![CDATA[<raw> & stuff]]></b><c><!--only--></c></a>");

        assert_eq!(root.children[0], Node::Comment(" note ".into()));
        assert_eq!(root.child("b").unwrap().text(), "<raw> & stuff");
    }

    #[test]
    fn mixed_content_is_written_without_indentation() {
        let root = round_trip("<a>before<b>inner</b> after </a>");

        assert_eq!(root.text(), "before after ");
    }

    #[test]
    fn protected_values_are_handed_to_the_protector_in_document_order() {
        let mut root = parse("<a><v Protected=\"True\"/><v Protected=\"True\"/></a>").unwrap();
        for (value, secret) in root.elements_mut().zip(["one", "two"]) {
            value.children = vec![Node::Protected(Secret::new(secret.into()))];
        }

        let mut seen = Vec::new();
        let written = write(&root, &mut |bytes| {
            seen.push(String::from_utf8(bytes.to_vec()).unwrap());
            format!("enc{}", seen.len())
        });

        assert_eq!(seen, ["one", "two"]);
        assert!(written.contains("<v Protected=\"True\">enc1</v>"));
        assert!(written.contains("<v Protected=\"True\">enc2</v>"));
        assert!(!format!("{root:?}").contains("one"));
    }

    /// A strict reader's view of a document: every character is one XML 1.0 allows.
    fn is_valid_document(written: &str) -> bool {
        written.chars().all(is_xml_char)
    }

    #[test]
    fn characters_xml_does_not_allow_are_not_written() {
        let mut root = Element::new("a");
        root.attributes
            .push(("k".into(), "a\u{1b}b\u{0}c\u{c}d".into()));
        root.children = vec![
            Node::Text("note\u{c}\u{1b}[0m\u{b} \u{FFFE}\u{FFFF}kept\tq \u{1F600}\r\n".into()),
            Node::Comment("c\u{1}d".into()),
        ];

        let written = write(&root, &mut no_protection);

        assert!(is_valid_document(&written), "written as:\n{written:?}");
        let reparsed = parse(&written).expect("reparses");
        assert_eq!(reparsed.attribute("k"), Some("abcd"));
        assert_eq!(reparsed.text(), "note[0m kept\tq \u{1F600}\r\n");
        assert_eq!(reparsed.children[1], Node::Comment("cd".into()));
    }

    #[test]
    fn every_character_xml_allows_is_written_as_it_was() {
        let all = "\t\n\r \u{D7FF}\u{E000}\u{FFFD}\u{10000}\u{10FFFF}é✓ & < > \"";
        let mut root = Element::new("a");
        root.attributes.push(("k".into(), all.replace('\n', "")));
        root.children = vec![Node::Text(all.into())];

        let written = write(&root, &mut no_protection);

        assert!(is_valid_document(&written));
        let reparsed = parse(&written).expect("reparses");
        assert_eq!(reparsed.text(), all);
        assert_eq!(
            reparsed.attribute("k"),
            Some(all.replace('\n', "").as_str())
        );
    }

    #[test]
    fn malformed_documents_are_rejected() {
        assert!(parse("<a><b></a>").is_err());
        assert!(parse("<a>").is_err());
        assert!(parse("").is_err());
        assert!(parse("<a/><b/>").is_err());
        assert!(parse("<a>&bogus;</a>").is_err());
    }
}
