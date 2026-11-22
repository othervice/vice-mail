//! A small read-only DOM for html5ever (the parser ammonia uses), and walks over it without
//! recursion, so deeply nested mails cannot overflow the stack. ammonia keeps its own DOM
//! private; `markup5ever_rcdom` is published as "unofficial", test-only code. This sink follows
//! the same structure (reference-counted nodes, weak parent links) and never panics: every
//! inconsistency the tree builder could ask for is ignored instead.

use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::mem;
use std::rc::{Rc, Weak};

use html5ever::interface::{
    Attribute, ElementFlags, ExpandedName, NodeOrText, QuirksMode, TreeSink,
};
use html5ever::tendril::{StrTendril, TendrilSink};
use html5ever::{ParseOpts, QualName, local_name, ns};

pub(crate) type Handle = Rc<Node>;

pub(crate) struct Node {
    parent: Cell<Option<Weak<Node>>>,
    pub(crate) children: RefCell<Vec<Handle>>,
    pub(crate) data: NodeData,
}

pub(crate) enum NodeData {
    Document,
    Doctype,
    Text {
        contents: RefCell<StrTendril>,
    },
    Comment,
    ProcessingInstruction,
    Element {
        name: QualName,
        attrs: RefCell<Vec<Attribute>>,
        /// Contents of a `<template>`, which are not children.
        template_contents: RefCell<Option<Handle>>,
        mathml_annotation_xml_integration_point: bool,
    },
}

impl Node {
    fn new(data: NodeData) -> Handle {
        Rc::new(Node {
            parent: Cell::new(None),
            children: RefCell::new(Vec::new()),
            data,
        })
    }

    /// The parent node, if any.
    pub(crate) fn parent(&self) -> Option<Handle> {
        let weak = self.parent.take();
        let parent = weak.as_ref().and_then(Weak::upgrade);
        self.parent.set(weak);
        parent
    }
}

impl Drop for Node {
    fn drop(&mut self) {
        // Iteratively, so that a deep tree does not recurse in drop.
        let mut nodes = mem::take(&mut *self.children.borrow_mut());
        if let NodeData::Element {
            template_contents, ..
        } = &self.data
            && let Some(contents) = template_contents.borrow_mut().take()
        {
            nodes.push(contents);
        }
        while let Some(node) = nodes.pop() {
            nodes.extend(mem::take(&mut *node.children.borrow_mut()));
            if let NodeData::Element {
                template_contents, ..
            } = &node.data
                && let Some(contents) = template_contents.borrow_mut().take()
            {
                nodes.push(contents);
            }
        }
    }
}

/// A parsed document.
pub(crate) struct Dom {
    pub(crate) document: Handle,
    quirks: Cell<QuirksMode>,
    /// Returned by `elem_name` for nodes that are not elements (never asked by html5ever).
    fallback_name: QualName,
}

impl Default for Dom {
    fn default() -> Self {
        Self {
            document: Node::new(NodeData::Document),
            quirks: Cell::new(QuirksMode::NoQuirks),
            fallback_name: QualName::new(None, ns!(html), local_name!("div")),
        }
    }
}

fn parent_and_index(target: &Handle) -> Option<(Handle, usize)> {
    let parent = target.parent()?;
    let index = parent
        .children
        .borrow()
        .iter()
        .position(|child| Rc::ptr_eq(child, target))?;
    Some((parent, index))
}

fn detach(target: &Handle) {
    if let Some((parent, index)) = parent_and_index(target) {
        parent.children.borrow_mut().remove(index);
    }
    target.parent.set(None);
}

fn append_node(parent: &Handle, child: Handle) {
    if Rc::ptr_eq(parent, &child) {
        return;
    }
    detach(&child);
    child.parent.set(Some(Rc::downgrade(parent)));
    parent.children.borrow_mut().push(child);
}

fn text_node(text: StrTendril) -> Handle {
    Node::new(NodeData::Text {
        contents: RefCell::new(text),
    })
}

impl TreeSink for Dom {
    type Handle = Handle;
    type Output = Self;
    type ElemName<'a> = ExpandedName<'a>;

    fn finish(self) -> Self {
        self
    }

    fn parse_error(&self, _msg: Cow<'static, str>) {}

    fn get_document(&self) -> Handle {
        self.document.clone()
    }

    fn elem_name<'a>(&'a self, target: &'a Handle) -> ExpandedName<'a> {
        match &target.data {
            NodeData::Element { name, .. } => name.expanded(),
            _ => self.fallback_name.expanded(),
        }
    }

    fn create_element(&self, name: QualName, attrs: Vec<Attribute>, flags: ElementFlags) -> Handle {
        Node::new(NodeData::Element {
            name,
            attrs: RefCell::new(attrs),
            template_contents: RefCell::new(flags.template.then(|| Node::new(NodeData::Document))),
            mathml_annotation_xml_integration_point: flags.mathml_annotation_xml_integration_point,
        })
    }

    fn create_comment(&self, _text: StrTendril) -> Handle {
        Node::new(NodeData::Comment)
    }

    fn create_pi(&self, _target: StrTendril, _data: StrTendril) -> Handle {
        Node::new(NodeData::ProcessingInstruction)
    }

    fn append(&self, parent: &Handle, child: NodeOrText<Handle>) {
        match child {
            NodeOrText::AppendNode(node) => append_node(parent, node),
            NodeOrText::AppendText(text) => {
                let last = parent.children.borrow().last().cloned();
                if let Some(last) = last
                    && let NodeData::Text { contents } = &last.data
                {
                    contents.borrow_mut().push_tendril(&text);
                    return;
                }
                append_node(parent, text_node(text));
            }
        }
    }

    fn append_based_on_parent_node(
        &self,
        element: &Handle,
        prev_element: &Handle,
        child: NodeOrText<Handle>,
    ) {
        if element.parent().is_some() {
            self.append_before_sibling(element, child);
        } else {
            self.append(prev_element, child);
        }
    }

    fn append_doctype_to_document(
        &self,
        _name: StrTendril,
        _public: StrTendril,
        _system: StrTendril,
    ) {
        append_node(&self.document, Node::new(NodeData::Doctype));
    }

    fn get_template_contents(&self, target: &Handle) -> Handle {
        match &target.data {
            NodeData::Element {
                template_contents, ..
            } => template_contents
                .borrow()
                .clone()
                .unwrap_or_else(|| target.clone()),
            _ => target.clone(),
        }
    }

    fn same_node(&self, x: &Handle, y: &Handle) -> bool {
        Rc::ptr_eq(x, y)
    }

    fn set_quirks_mode(&self, mode: QuirksMode) {
        self.quirks.set(mode);
    }

    fn append_before_sibling(&self, sibling: &Handle, new_node: NodeOrText<Handle>) {
        let Some((parent, index)) = parent_and_index(sibling) else {
            return;
        };
        let child = match new_node {
            NodeOrText::AppendText(text) => {
                let previous = index
                    .checked_sub(1)
                    .and_then(|i| parent.children.borrow().get(i).cloned());
                if let Some(previous) = previous
                    && let NodeData::Text { contents } = &previous.data
                {
                    contents.borrow_mut().push_tendril(&text);
                    return;
                }
                text_node(text)
            }
            NodeOrText::AppendNode(node) => node,
        };
        if Rc::ptr_eq(&child, &parent) {
            return;
        }
        detach(&child);
        // Detaching may have moved the sibling (if the child was before it).
        let Some((parent, index)) = parent_and_index(sibling) else {
            return;
        };
        child.parent.set(Some(Rc::downgrade(&parent)));
        parent.children.borrow_mut().insert(index, child);
    }

    fn add_attrs_if_missing(&self, target: &Handle, attrs: Vec<Attribute>) {
        if let NodeData::Element {
            attrs: existing, ..
        } = &target.data
        {
            let mut existing = existing.borrow_mut();
            for attr in attrs {
                if !existing.iter().any(|e| e.name == attr.name) {
                    existing.push(attr);
                }
            }
        }
    }

    fn remove_from_parent(&self, target: &Handle) {
        detach(target);
    }

    fn reparent_children(&self, node: &Handle, new_parent: &Handle) {
        if Rc::ptr_eq(node, new_parent) {
            return;
        }
        let children = mem::take(&mut *node.children.borrow_mut());
        for child in &children {
            child.parent.set(Some(Rc::downgrade(new_parent)));
        }
        new_parent.children.borrow_mut().extend(children);
    }

    fn is_mathml_annotation_xml_integration_point(&self, handle: &Handle) -> bool {
        matches!(
            handle.data,
            NodeData::Element {
                mathml_annotation_xml_integration_point: true,
                ..
            }
        )
    }
}

/// Parses a whole document (`<html>`, `<head>`, `<body>` are created as a browser would).
pub(crate) fn parse_document(html: &str) -> Dom {
    html5ever::parse_document(Dom::default(), ParseOpts::default()).one(html)
}

/// Parses HTML as the content of a `<body>`, as the browser does with our sanitised body.
pub(crate) fn parse_body_fragment(html: &str) -> Dom {
    html5ever::parse_fragment(
        Dom::default(),
        ParseOpts::default(),
        QualName::new(None, ns!(html), local_name!("body")),
        Vec::new(),
        false,
    )
    .one(html)
}

pub(crate) enum Visit<'a> {
    Enter(&'a Handle),
    Leave(&'a Handle),
}

/// What to do after entering a node.
#[derive(PartialEq, Eq)]
pub(crate) enum Descend {
    Yes,
    Skip,
}

/// Depth-first walk over `root` and its descendants with enter/leave events. Template contents
/// are not visited (they are not rendered).
pub(crate) fn walk(root: &Handle, mut visit: impl FnMut(Visit<'_>) -> Descend) {
    let mut stack: Vec<(Handle, bool)> = vec![(root.clone(), false)];
    while let Some((node, leaving)) = stack.pop() {
        if leaving {
            visit(Visit::Leave(&node));
            continue;
        }
        if visit(Visit::Enter(&node)) == Descend::Skip {
            continue;
        }
        stack.push((node.clone(), true));
        let children = node.children.borrow();
        stack.extend(children.iter().rev().map(|child| (child.clone(), false)));
    }
}

/// The HTML element name of a node, `None` for text and for SVG/MathML elements.
pub(crate) fn html_name(node: &Handle) -> Option<&str> {
    match &node.data {
        NodeData::Element { name, .. } if name.ns == ns!(html) => Some(&name.local),
        _ => None,
    }
}

/// Value of an attribute of an element node.
pub(crate) fn attribute(node: &Handle, attribute: &str) -> Option<String> {
    match &node.data {
        NodeData::Element { attrs, .. } => attrs
            .borrow()
            .iter()
            .find(|attr| &*attr.name.local == attribute)
            .map(|attr| attr.value.to_string()),
        _ => None,
    }
}

/// The first descendant element with this HTML name (document order).
pub(crate) fn find_element(root: &Handle, wanted: &str) -> Option<Handle> {
    let mut found = None;
    walk(root, |visit| match visit {
        Visit::Enter(node) if found.is_none() => {
            if html_name(node) == Some(wanted) {
                found = Some(node.clone());
                Descend::Skip
            } else {
                Descend::Yes
            }
        }
        _ => Descend::Skip,
    });
    found
}

/// Text content of all descendants.
pub(crate) fn text_content(root: &Handle) -> String {
    let mut out = String::new();
    walk(root, |visit| {
        if let Visit::Enter(node) = visit
            && let NodeData::Text { contents } = &node.data
        {
            out.push_str(&contents.borrow());
        }
        Descend::Yes
    });
    out
}

/// Collapses all whitespace runs to one space, trims, and cuts to `max_chars`.
pub(crate) fn squash_whitespace(text: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for (count, word) in text.split_whitespace().enumerate() {
        if count > 0 {
            out.push(' ');
        }
        out.push_str(word);
        if out.len() > max_chars * 4 {
            break;
        }
    }
    if out.chars().count() > max_chars {
        out = out.chars().take(max_chars).collect();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(dom: &Dom) -> Vec<String> {
        let mut out = Vec::new();
        walk(&dom.document, |visit| {
            if let Visit::Enter(node) = visit
                && let NodeData::Element { name, .. } = &node.data
            {
                out.push(name.local.to_string());
            }
            Descend::Yes
        });
        out
    }

    #[test]
    fn builds_the_tree_a_browser_builds() {
        // Foster parenting (text and <b> moved before the table), implied elements, adoption.
        let dom = parse_document("<table>x<b>y</b><tr><td>z</table><p>a<b>b<p>c");
        assert_eq!(
            names(&dom),
            [
                "html", "head", "body", "b", "table", "tbody", "tr", "td", "p", "b", "p", "b"
            ]
        );
        assert_eq!(text_content(&dom.document), "xyzabc");
        let fragment = parse_body_fragment("<style>p{}</style><svg><style><img></style></svg>");
        assert_eq!(names(&fragment), ["html", "style", "svg", "style", "img"]);
    }

    #[test]
    fn deep_trees_are_walked_and_dropped_without_recursion() {
        // Built directly: the parser itself is quadratic in the depth (see `guard`).
        let dom = Dom::default();
        let mut parent = dom.document.clone();
        for _ in 0..500_000 {
            let child = dom.create_element(
                QualName::new(None, ns!(html), local_name!("div")),
                Vec::new(),
                ElementFlags::default(),
            );
            append_node(&parent, child.clone());
            parent = child;
        }
        let mut depth = 0usize;
        walk(&dom.document, |visit| {
            if let Visit::Enter(_) = visit {
                depth += 1;
            }
            Descend::Yes
        });
        assert_eq!(depth, 500_001);
        drop(parent);
        drop(dom);
        let templates = "<template>".repeat(1_000);
        drop(parse_document(&templates));
    }

    /// Shape of a tree: element names (with namespace) and text, in document order.
    fn shape(dom: &Dom) -> Vec<String> {
        let mut out = Vec::new();
        walk(&dom.document, |visit| match visit {
            Visit::Enter(node) => {
                match &node.data {
                    NodeData::Element { name, attrs, .. } => {
                        let attrs: Vec<String> = attrs
                            .borrow()
                            .iter()
                            .map(|a| a.name.local.to_string())
                            .collect();
                        out.push(format!(
                            "<{:?}:{} {}>",
                            name.ns,
                            name.local,
                            attrs.join(",")
                        ));
                    }
                    NodeData::Text { contents } => out.push(contents.borrow().to_string()),
                    _ => {}
                }
                Descend::Yes
            }
            Visit::Leave(node) => {
                if let NodeData::Element { name, .. } = &node.data {
                    out.push(format!("</{}>", name.local));
                }
                Descend::Yes
            }
        });
        out
    }

    fn reference_shape(html: &str) -> Vec<String> {
        use markup5ever_rcdom::{Handle as RcHandle, NodeData as RcData, RcDom};
        let dom = html5ever::parse_document(RcDom::default(), ParseOpts::default()).one(html);
        let mut out = Vec::new();
        let mut stack: Vec<(RcHandle, bool)> = vec![(dom.document.clone(), false)];
        while let Some((node, leaving)) = stack.pop() {
            if let RcData::Element { name, attrs, .. } = &node.data {
                if leaving {
                    out.push(format!("</{}>", name.local));
                    continue;
                }
                let attrs: Vec<String> = attrs
                    .borrow()
                    .iter()
                    .map(|a| a.name.local.to_string())
                    .collect();
                out.push(format!(
                    "<{:?}:{} {}>",
                    name.ns,
                    name.local,
                    attrs.join(",")
                ));
                stack.push((node.clone(), true));
            } else if let RcData::Text { contents } = &node.data {
                out.push(contents.borrow().to_string());
            }
            let children = node.children.borrow();
            stack.extend(children.iter().rev().map(|c| (c.clone(), false)));
        }
        out
    }

    #[test]
    fn same_tree_as_the_reference_dom() {
        // Tree-construction corner cases: foster parenting, adoption agency, implied and
        // misnested tags, templates, foreign content and its breakouts, the seed's mXSS.
        let mut cases: Vec<String> = [
            "<table>x<b>y</b><tr><td>z</table>",
            "<p>a<b>b<i>c</p>d</i>e</b>f",
            "<a href=1><div><a href=2>x</a></div></a>",
            "<b><p>1</b>2</p>3",
            "<table><tr><td><table><tr><td>deep</table>after</td></tr></table>",
            "<ul><li>a<li>b<ul><li>c</ul></ul><dl><dt>t<dd>d<dt>t2</dl>",
            "<select><option>1<option>2<optgroup><option>3</select>",
            "<template><p>t</p><td>cell</td></template>after",
            "<svg><desc><p>html in svg</p></desc><foreignObject><b>x</b></foreignObject><p>out",
            "<math><mtext><table><mglyph><style><img src=x onerror=a()></style></mglyph></table></mtext></math>",
            "<math><annotation-xml encoding=text/html><div>x</div></annotation-xml></math>",
            "<form><math><mtext></form><form><mglyph><style></math><img src onerror=alert(1)>",
            "<frameset><frame></frameset>text",
            "<noscript><p title=\"</noscript><img src=x>\"></noscript>",
            "<!DOCTYPE html><html a=1><head><title>t</title></head><body b=2><body c=3>x</body></html>trailing",
            "<isindex prompt=x><image src=y><nobr>a<nobr>b",
            "</p></br><table></caption><tbody></tr>x",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        // Plus random tag soup from a fixed seed.
        let parts = [
            "<p>",
            "</p>",
            "<b>",
            "</b>",
            "<i>",
            "<table>",
            "<tr>",
            "<td>",
            "</td>",
            "</table>",
            "<div>",
            "</div>",
            "<a>",
            "</a>",
            "<svg>",
            "</svg>",
            "<math>",
            "<mtext>",
            "<li>",
            "<ul>",
            "<select>",
            "<option>",
            "<template>",
            "</template>",
            "<form>",
            "</form>",
            "text",
            "<br>",
            "</br>",
            "<font>",
            "<caption>",
            "<colgroup>",
            "<col>",
            "<frameset>",
            "<style>",
            "</style>",
            "<title>",
            "<textarea>",
            "<plaintext>",
            "<!--",
            "-->",
        ];
        let mut state: u64 = 0x5EED;
        for _ in 0..400 {
            let mut html = String::new();
            for _ in 0..30 {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                html.push_str(parts[(state % parts.len() as u64) as usize]);
            }
            cases.push(html);
        }
        for html in &cases {
            assert_eq!(
                shape(&parse_document(html)),
                reference_shape(html),
                "{html}"
            );
        }
    }
}
