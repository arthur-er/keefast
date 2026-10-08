//! Attachments: files kept with an Entry.
//!
//! KDBX 4 stores each attachment's contents once, in the binary pool of the Vault file, and an
//! Entry refers to them by index: `<Binary><Key>name</Key><Value Ref="N"/></Binary>`. Several
//! Entries and history versions can refer to the same contents, so edits never change what is
//! in the pool in place; they add contents, point references at them, and drop what nothing
//! refers to any more.

use std::fmt;

use crate::edit::{push, text_element};
use crate::kdbx::Binary;
use crate::xml::{Element, Node, Secret};

/// A file kept with an Entry.
#[derive(Clone, PartialEq, Eq)]
pub struct Attachment {
    pub name: String,
    data: Secret,
}

/// How an attachment can be shown in the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachmentPreview<'a> {
    Text(&'a str),
    Image(ImageFormat),
    /// Neither text nor an image keefast can show.
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    Png,
    Jpeg,
    Gif,
    Webp,
    Bmp,
    Svg,
}

/// The bytes each image format starts with.
const IMAGE_SIGNATURES: &[(&[u8], ImageFormat)] = &[
    (b"\x89PNG\r\n\x1a\n", ImageFormat::Png),
    (b"\xff\xd8\xff", ImageFormat::Jpeg),
    (b"GIF87a", ImageFormat::Gif),
    (b"GIF89a", ImageFormat::Gif),
    (b"BM", ImageFormat::Bmp),
];

impl Attachment {
    /// An attachment holding `data`, which is wiped when the last copy of it is dropped.
    pub fn new(name: impl Into<String>, data: Vec<u8>) -> Self {
        Self {
            name: name.into(),
            data: Secret::new(data),
        }
    }

    pub fn data(&self) -> &[u8] {
        self.data.bytes()
    }

    /// Adds the attachment to an Entry's `attachments`, numbering its name if another one has
    /// it already, as in `codes (2).txt`.
    pub fn add_to(mut self, attachments: &mut Vec<Attachment>) {
        self.name = unique_name(&self.name, |name| {
            attachments.iter().any(|other| other.name == name)
        });
        attachments.push(self);
    }

    /// The size in bytes.
    pub fn size(&self) -> usize {
        self.data().len()
    }

    /// Images are told by their first bytes, SVG by its name; text is UTF-8 without control
    /// characters other than line breaks and tabs.
    pub fn preview(&self) -> AttachmentPreview<'_> {
        let data = self.data();
        if let Some((_, format)) = IMAGE_SIGNATURES
            .iter()
            .find(|(signature, _)| data.starts_with(signature))
        {
            return AttachmentPreview::Image(*format);
        }
        if data.len() >= 12 && data.starts_with(b"RIFF") && &data[8..12] == b"WEBP" {
            return AttachmentPreview::Image(ImageFormat::Webp);
        }
        let Ok(text) = std::str::from_utf8(data) else {
            return AttachmentPreview::None;
        };
        if self.name.to_lowercase().ends_with(".svg") {
            return AttachmentPreview::Image(ImageFormat::Svg);
        }
        let printable = text
            .chars()
            .all(|c| !c.is_control() || matches!(c, '\n' | '\r' | '\t'));
        if printable {
            AttachmentPreview::Text(text)
        } else {
            AttachmentPreview::None
        }
    }
}

impl fmt::Debug for Attachment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Attachment")
            .field("name", &self.name)
            .field("size", &self.size())
            .finish()
    }
}

/// `name`, or `name (2)`, `name (3)`… before its extension, whichever `taken` doesn't hold.
fn unique_name(name: &str, taken: impl Fn(&str) -> bool) -> String {
    if !taken(name) {
        return name.to_owned();
    }
    let (stem, extension) = match name.rfind('.') {
        Some(dot) if dot > 0 => name.split_at(dot),
        _ => (name, ""),
    };
    (2..)
        .map(|n| format!("{stem} ({n}){extension}"))
        .find(|candidate| !taken(candidate))
        .expect("some number is free")
}

/// An Entry element's attachments, in order. References to contents the pool doesn't have are
/// left out; the next edit drops them (see [`drop_unused`]).
pub(crate) fn read(entry: &Element, pool: &[Binary]) -> Vec<Attachment> {
    entry
        .children_named("Binary")
        .filter_map(|binary| {
            let name = binary.child("Key").map(Element::text)?;
            let data = &pool.get(reference(binary)?)?.data;
            Some(Attachment {
                name,
                data: data.clone(),
            })
        })
        .collect()
}

/// The pool index a `<Binary>` refers to.
fn reference(binary: &Element) -> Option<usize> {
    binary.child("Value")?.attribute("Ref")?.trim().parse().ok()
}

/// Replaces an Entry element's attachments with `attachments`, adding contents the pool
/// hasn't got yet. Contents already in the pool are referred to, not stored again.
pub(crate) fn write(entry: &mut Element, attachments: &[Attachment], pool: &mut Vec<Binary>) {
    let at = entry
        .children
        .iter()
        .position(|node| matches!(node, Node::Element(e) if e.name == "Binary"));
    entry
        .children
        .retain(|node| !matches!(node, Node::Element(e) if e.name == "Binary"));
    let at = at.unwrap_or_else(|| {
        // KeePass writes attachments after the fields, before auto-type and history.
        let before = ["AutoType", "History"];
        entry
            .children
            .iter()
            .position(|node| matches!(node, Node::Element(e) if before.contains(&&*e.name)))
            .unwrap_or(entry.children.len())
    });
    for (offset, attachment) in attachments.iter().enumerate() {
        let index = pool
            .iter()
            .position(|binary| binary.data == attachment.data)
            .unwrap_or_else(|| {
                pool.push(Binary {
                    protected: true,
                    data: attachment.data.clone(),
                });
                pool.len() - 1
            });
        let mut binary = Element::new("Binary");
        push(&mut binary, text_element("Key", &attachment.name));
        let mut value = Element::new("Value");
        value.attributes.push(("Ref".into(), index.to_string()));
        push(&mut binary, value);
        entry.children.insert(at + offset, Node::Element(binary));
    }
}

/// Drops the pool's contents that no reference in `document` points at, and renumbers the
/// references to what is left. References to contents the pool never had are dropped too, as
/// KeePassXC drops them: no client can read them, and once the pool grows they would point at
/// contents added for another Entry.
pub(crate) fn drop_unused(document: &mut Element, pool: &mut Vec<Binary>) {
    drop_dangling(document, pool.len());
    let mut used = vec![false; pool.len()];
    visit_references(document, &mut |index| {
        if let Some(used) = used.get_mut(*index) {
            *used = true;
        }
    });
    if used.iter().all(|&used| used) {
        return;
    }
    let mut renumbered = Vec::with_capacity(pool.len());
    let mut next = 0;
    for &used in &used {
        renumbered.push(next);
        next += usize::from(used);
    }
    let mut kept = used.iter();
    pool.retain(|_| *kept.next().expect("one flag per binary"));
    visit_references(document, &mut |index| {
        if let Some(&new) = renumbered.get(*index) {
            *index = new;
        }
    });
}

/// Removes the attachments whose reference points past the pool.
fn drop_dangling(element: &mut Element, pool_len: usize) {
    element.children.retain(|node| {
        !matches!(node, Node::Element(binary) if binary.name == "Binary"
            && reference(binary).is_some_and(|index| index >= pool_len))
    });
    for child in element.elements_mut() {
        drop_dangling(child, pool_len);
    }
}

/// Calls `visit` with every attachment reference in the document, in Entries and their history
/// alike, and writes back the index it leaves.
fn visit_references(element: &mut Element, visit: &mut impl FnMut(&mut usize)) {
    if element.name == "Binary"
        && let Some(mut index) = reference(element)
    {
        let before = index;
        visit(&mut index);
        if index != before {
            let value = element.child_mut("Value").expect("a reference has a Value");
            for (key, reference) in &mut value.attributes {
                if key == "Ref" {
                    *reference = index.to_string();
                }
            }
        }
        return;
    }
    for child in element.elements_mut() {
        visit_references(child, visit);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml;

    fn binary(data: &[u8]) -> Binary {
        Binary {
            protected: false,
            data: Secret::new(data.to_vec()),
        }
    }

    fn refs(document: &Element) -> Vec<String> {
        let mut found = Vec::new();
        fn walk(element: &Element, found: &mut Vec<String>) {
            if element.name == "Value"
                && let Some(reference) = element.attribute("Ref")
            {
                found.push(reference.to_owned());
            }
            for child in element.elements() {
                walk(child, found);
            }
        }
        walk(document, &mut found);
        found
    }

    #[test]
    fn unused_contents_are_dropped_and_references_renumbered() {
        let mut document = xml::parse(
            r#"<Root><Entry>
                <Binary><Key>c</Key><Value Ref="2"/></Binary>
                <History><Entry><Binary><Key>a</Key><Value Ref="0"/></Binary></Entry></History>
                <Binary><Key>gone</Key><Value Ref="9"/></Binary>
            </Entry></Root>"#,
        )
        .unwrap();
        let mut pool = vec![binary(b"a"), binary(b"b"), binary(b"c")];

        drop_unused(&mut document, &mut pool);

        let data: Vec<&[u8]> = pool.iter().map(|binary| binary.data.bytes()).collect();
        assert_eq!(data, [&b"a"[..], b"c"]);
        assert_eq!(
            refs(&document),
            ["1", "0"],
            "the dangling reference is gone"
        );
    }

    #[test]
    fn a_pool_in_use_is_left_alone() {
        let mut document =
            xml::parse(r#"<Entry><Binary><Key>a</Key><Value Ref="0"/></Binary></Entry>"#).unwrap();
        let mut pool = vec![binary(b"a")];
        drop_unused(&mut document, &mut pool);
        assert_eq!(pool.len(), 1);
        assert_eq!(refs(&document), ["0"]);
    }

    #[test]
    fn written_attachments_reuse_pool_contents() {
        let mut entry = xml::parse(
            r#"<Entry><String/><Binary><Key>old</Key><Value Ref="0"/></Binary><AutoType/></Entry>"#,
        )
        .unwrap();
        let mut pool = vec![binary(b"a")];
        let attachments = [
            Attachment::new("x", b"b".to_vec()),
            Attachment::new("y", b"a".to_vec()),
        ];

        write(&mut entry, &attachments, &mut pool);

        assert_eq!(pool.len(), 2);
        let names: Vec<&str> = entry.elements().map(|e| &*e.name).collect();
        assert_eq!(names, ["String", "Binary", "Binary", "AutoType"]);
        assert_eq!(refs(&entry), ["1", "0"]);
        assert_eq!(read(&entry, &pool), attachments);
    }
}
