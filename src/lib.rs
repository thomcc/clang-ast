//! This library provides deserialization logic for efficiently processing
//! Clang's `-ast-dump=json` format.
//!
//! <br>
//!
//! # Format overview
//!
//! An AST dump is generated by a compiler command like:
//!
//! <pre>
//! <code>$  <b>clang++ -Xclang -ast-dump=json -fsyntax-only path/to/source.cc</b></code>
//! </pre>
//!
//! The high-level structure is a tree of nodes, each of which has an `"id"` and
//! a `"kind"`, zero or more further fields depending on what the node kind is,
//! and finally an optional `"inner"` array of child nodes.
//!
//! As an example, for an input file containing just the declaration `class S;`,
//! the AST would be as follows:
//!
//! ```
//! # stringify! {
//! {
//!   "id": "0x1fcea38",                 //<-- root node
//!   "kind": "TranslationUnitDecl",
//!   "inner": [
//!     {
//!       "id": "0xadf3a8",              //<-- first child node
//!       "kind": "CXXRecordDecl",
//!       "loc": {
//!         "offset": 6,
//!         "file": "source.cc",
//!         "line": 1,
//!         "col": 7,
//!         "tokLen": 1
//!       },
//!       "range": {
//!         "begin": {
//!           "offset": 0,
//!           "col": 1,
//!           "tokLen": 5
//!         },
//!         "end": {
//!           "offset": 6,
//!           "col": 7,
//!           "tokLen": 1
//!         }
//!       },
//!       "name": "S",
//!       "tagUsed": "class"
//!     }
//!   ]
//! }
//! # };
//! ```
//!
//! <br><br>
//!
//! # Library design
//!
//! By design, the clang-ast crate *does not* provide a single great big data
//! structure that exhaustively covers every possible field of every possible
//! Clang node type. There are three major reasons:
//!
//! - **Performance** &mdash; these ASTs get quite large. For a reasonable
//!   mid-sized translation unit that includes several platform headers, you can
//!   easily get an AST that is tens to hundreds of megabytes of JSON. To
//!   maintain performance of downstream tooling built on the AST, it's critical
//!   that you deserialize only the few fields which are directly required by
//!   your use case, and allow Serde's deserializer to efficiently ignore all
//!   the rest.
//!
//! - **Stability** &mdash; as Clang is developed, the specific fields
//!   associated with each node kind are expected to change over time in
//!   non-additive ways. This is nonproblematic because the churn on the scale
//!   of individual nodes is minimal (maybe one change every several years).
//!   However, if there were a data structure that promised to be able to
//!   deserialize every possible piece of information in every node, practically
//!   every change to Clang would be a breaking change to some node *somewhere*
//!   despite your tooling not caring anything at all about that node kind. By
//!   deserializing only those fields which are directly relevant to your use
//!   case, you become insulated from the vast majority of syntax tree changes.
//!
//! - **Compile time** &mdash; a typical use case involves inspecting only a
//!   tiny fraction of the possible nodes or fields, on the order of 1%.
//!   Consequently your code will compile 100&times; faster than if you tried to
//!   include everything in the data structure.
//!
//! <br>
//!
//! # Data structures
//!
//! The core data structure of the clang-ast crate is `Node<T>`.
//!
//! ```
//! # use clang_ast::Id;
//! #
//! pub struct Node<T> {
//!     pub id: Id,
//!     pub kind: T,
//!     pub inner: Vec<Node<T>>,
//! }
//! ```
//!
//! The caller must provide their own kind type `T`, which is an enum or struct
//! as described below. `T` determines exactly what information the clang-ast
//! crate will deserialize out of the AST dump.
//!
//! By convention you should name your `T` type `Clang`.
//!
//! <br>
//!
//! # T = enum
//!
//! Most often, you'll want `Clang` to be an enum. In this case your enum must
//! have one variant per node kind that you care about. The name of each variant
//! matches the `"kind"` entry seen in the AST.
//!
//! Additionally there must be a fallback variant, which must be named either
//! `Unknown` or `Other`, into which clang-ast will put all tree nodes not
//! matching one of the expected kinds.
//!
//! ```no_run
//! use serde::Deserialize;
//!
//! pub type Node = clang_ast::Node<Clang>;
//!
//! #[derive(Deserialize)]
//! pub enum Clang {
//!     NamespaceDecl { name: Option<String> },
//!     EnumDecl { name: Option<String> },
//!     EnumConstantDecl { name: String },
//!     Other,
//! }
//!
//! fn main() {
//!     let json = std::fs::read_to_string("ast.json").unwrap();
//!     let node: Node = serde_json::from_str(&json).unwrap();
//!
//! }
//! ```
//!
//! The above is a simple example with variants for processing `"kind":
//! "NamespaceDecl"`,&ensp;`"kind": "EnumDecl"`,&ensp;and `"kind":
//! "EnumConstantDecl"` nodes. This is sufficient to extract the set of variants
//! of every enum in the translation unit, and the enums' namespace (possibly
//! anonymous) and enum name (possibly anonymous).
//!
//! Newtype variants are fine too, particularly if you'll be deserializing more
//! than one field for some nodes.
//!
//! ```
//! use serde::Deserialize;
//!
//! pub type Node = clang_ast::Node<Clang>;
//!
//! #[derive(Deserialize)]
//! pub enum Clang {
//!     NamespaceDecl(NamespaceDecl),
//!     EnumDecl(EnumDecl),
//!     EnumConstantDecl(EnumConstantDecl),
//!     Other,
//! }
//!
//! #[derive(Deserialize, Debug)]
//! pub struct NamespaceDecl {
//!     pub name: Option<String>,
//! }
//!
//! #[derive(Deserialize, Debug)]
//! pub struct EnumDecl {
//!     pub name: Option<String>,
//! }
//!
//! #[derive(Deserialize, Debug)]
//! pub struct EnumConstantDecl {
//!     pub name: String,
//! }
//! ```
//!
//! <br><br>
//!
//! # T = struct
//!
//! Rarely, it can make sense to instantiate Node with `Clang` being a struct
//! type, instead of an enum. This allows for deserializing a uniform group of
//! data out of *every* node in the syntax tree.
//!
//! The following example struct collects the `"loc"` and `"range"` of every
//! node if present; these fields provide the file name / line / column position
//! of nodes. Not every node kind contains this information, so we use `Option`
//! to collect it for just the nodes that have it.
//!
//! ```
//! use serde::Deserialize;
//!
//! pub type Node = clang_ast::Node<Clang>;
//!
//! #[derive(Deserialize)]
//! pub struct Clang {
//!     pub kind: String,  // or clang_ast::Kind
//!     pub loc: Option<clang_ast::SourceLocation>,
//!     pub range: Option<clang_ast::SourceRange>,
//! }
//! ```
//!
//! If you really need, it's also possible to store *every other piece of
//! key/value information about every node* via a weakly typed `Map<String,
//! Value>` and the Serde `flatten` attribute.
//!
//! ```
//! use serde::Deserialize;
//! use serde_json::{Map, Value};
//!
//! #[derive(Deserialize)]
//! pub struct Clang {
//!     pub kind: String,  // or clang_ast::Kind
//!     #[serde(flatten)]
//!     pub data: Map<String, Value>,
//! }
//! ```
//!
//! <br><br>
//!
//! # Hybrid approach
//!
//! To deserialize kind-specific information about a fixed set of node kinds you
//! care about, as well as some uniform information about every other kind of
//! node, you can use a hybrid of the two approaches by giving your `Other` /
//! `Unknown` fallback variant some fields.
//!
//! ```
//! use serde::Deserialize;
//!
//! pub type Node = clang_ast::Node<Clang>;
//!
//! #[derive(Deserialize)]
//! pub enum Clang {
//!     NamespaceDecl(NamespaceDecl),
//!     EnumDecl(EnumDecl),
//!     Other {
//!         kind: clang_ast::Kind,
//!     },
//! }
//! #
//! # #[derive(Deserialize)]
//! # struct NamespaceDecl;
//! #
//! # #[derive(Deserialize)]
//! # struct EnumDecl;
//! ```
//!
//! <br><br>
//!
//! # Source locations
//!
//! Many node kinds expose the source location of the corresponding source code
//! tokens, which includes:
//!
//! - the filepath at which they're located;
//! - the chain of `#include`s by which that file was brought into the
//!   translation unit;
//! - line/column positions within the source file;
//! - macro expansion trace for tokens constructed by expansion of a C
//!   preprocessor macro.
//!
//! You'll find this information in fields called `"loc"` and/or `"range"` in
//! the JSON representation.
//!
//! ```
//! # stringify! {
//! {
//!   "id": "0x1251428",
//!   "kind": "NamespaceDecl",
//!   "loc": {                           //<--
//!     "offset": 7004,
//!     "file": "/usr/include/x86_64-linux-gnu/c++/10/bits/c++config.h",
//!     "line": 258,
//!     "col": 11,
//!     "tokLen": 3,
//!     "includedFrom": {
//!       "file": "/usr/include/c++/10/utility"
//!     }
//!   },
//!   "range": {                         //<--
//!     "begin": {
//!       "offset": 6994,
//!       "col": 1,
//!       "tokLen": 9
//!     },
//!     "end": {
//!       "offset": 7155,
//!       "line": 266,
//!       "col": 1,
//!       "tokLen": 1
//!     }
//!   },
//!   ...
//! }
//! # };
//! ```
//!
//! The naive deserialization of these structures is challenging to work with
//! because Clang uses field omission to mean "same as previous". So if a
//! `"loc"` is printed without a `"file"` inside, it means the loc is in the
//! same file as the immediately previous loc in serialization order.
//!
//! The clang-ast crate provides types for deserializing this source location
//! information painlessly, producing `Arc<str>` as the type of filepaths which
//! may be shared across multiple source locations.
//!
//! ```
//! use serde::Deserialize;
//!
//! pub type Node = clang_ast::Node<Clang>;
//!
//! #[derive(Deserialize)]
//! pub enum Clang {
//!     NamespaceDecl(NamespaceDecl),
//!     Other,
//! }
//!
//! #[derive(Deserialize, Debug)]
//! pub struct NamespaceDecl {
//!     pub name: Option<String>,
//!     pub loc: clang_ast::SourceLocation,    //<--
//!     pub range: clang_ast::SourceRange,     //<--
//! }
//! ```
//!
//! <br><br>
//!
//! # Node identifiers
//!
//! Every syntax tree node has an `"id"`. In JSON it's the memory address of
//! Clang's internal memory allocation for that node, serialized to a hex
//! string.
//!
//! The AST dump uses ids as backreferences in nodes of directed acyclic graph
//! nature. For example the following MemberExpr node is part of the invocation
//! of an `operator bool` conversion, and thus its syntax tree refers to the
//! resolved `operator bool` conversion function declaration:
//!
//! ```
//! # stringify! {
//! {
//!   "id": "0x9918b88",
//!   "kind": "MemberExpr",
//!   "valueCategory": "rvalue",
//!   "referencedMemberDecl": "0x12d8330",     //<--
//!   ...
//! }
//! # };
//! ```
//!
//! The node it references, with memory address 0x12d8330, is found somewhere
//! earlier in the syntax tree:
//!
//! ```
//! # stringify! {
//! {
//!   "id": "0x12d8330",                       //<--
//!   "kind": "CXXConversionDecl",
//!   "name": "operator bool",
//!   "mangledName": "_ZNKSt17integral_constantIbLb1EEcvbEv",
//!   "type": {
//!     "qualType": "std::integral_constant<bool, true>::value_type () const noexcept"
//!   },
//!   "constexpr": true,
//!   ...
//! }
//! # };
//! ```
//!
//! Due to the ubiquitous use of ids for backreferencing, it is valuable to
//! deserialize them not as strings but as a 64-bit integer. The clang-ast crate
//! provides an `Id` type for this purpose, which is cheaply copyable, hashable,
//! and comparible more cheaply than a string. You may find yourself with lots
//! of hashtables keyed on `Id`.

#![doc(html_root_url = "https://docs.rs/clang-ast/0.0.0")]
#![allow(
    clippy::let_underscore_drop,
    clippy::must_use_candidate,
    clippy::option_if_let_else,
    clippy::ptr_arg
)]

mod deserializer;
mod id;
mod intern;
mod kind;
mod loc;

extern crate serde;

use crate::deserializer::NodeDeserializer;
use crate::kind::AnyKind;
use serde::de::{Deserializer, MapAccess, Visitor};
use serde::Deserialize;
use std::fmt;
use std::marker::PhantomData;

pub use crate::id::Id;
pub use crate::kind::Kind;
pub use crate::loc::{BareSourceLocation, IncludedFrom, SourceLocation, SourceRange};

/// <font style="font-variant:small-caps">syntax tree root</font>
#[derive(Debug)]
pub struct Node<T> {
    pub id: Id,
    pub kind: T,
    pub inner: Vec<Node<T>>,
}

struct NodeVisitor<T> {
    marker: PhantomData<fn() -> T>,
}

impl<'de, T> Visitor<'de> for NodeVisitor<T>
where
    T: Deserialize<'de>,
{
    type Value = Node<T>;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("clang syntax tree node")
    }

    fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
    where
        M: MapAccess<'de>,
    {
        #[derive(Deserialize)]
        #[serde(field_identifier, rename_all = "lowercase")]
        enum FirstField {
            Id,
            Kind,
            Inner,
        }

        let mut id = None;
        let mut inner = Vec::new();
        let kind = loop {
            match map.next_key()? {
                None => {
                    let kind = AnyKind::Kind(Kind::null);
                    let deserializer = NodeDeserializer::new(kind, &mut inner, map);
                    break T::deserialize(deserializer)?;
                }
                Some(FirstField::Id) => {
                    if id.is_some() {
                        return Err(serde::de::Error::duplicate_field("id"));
                    }
                    id = Some(map.next_value()?);
                }
                Some(FirstField::Kind) => {
                    let kind: AnyKind = map.next_value()?;
                    let deserializer = NodeDeserializer::new(kind, &mut inner, map);
                    break T::deserialize(deserializer)?;
                }
                Some(FirstField::Inner) => {
                    return Err(serde::de::Error::missing_field("kind"));
                }
            }
        };

        let id = id.unwrap_or_default();

        Ok(Node { id, kind, inner })
    }
}

impl<'de, T> Deserialize<'de> for Node<T>
where
    T: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let _intern = intern::activate();
        let marker = PhantomData;
        let visitor = NodeVisitor { marker };
        deserializer.deserialize_map(visitor)
    }
}
