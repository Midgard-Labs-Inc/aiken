//! How the values of a data type look at runtime, computed once per data type.
//!
//! Codegen reads the constructor indices, field types and representation. The type checker
//! reads the summary, which says what a type's values can contain, so it never needs to look a
//! type's constructors up by name.

use super::{Type, TypeVar};
use crate::ast::{DecoratorKind, TypedDataType, well_known};
use std::{collections::HashMap, ops::Deref, rc::Rc};

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DataLayout {
    /// The type's parameters, as generic type variables.
    pub parameters: Vec<Rc<Type>>,
    pub constructors: Vec<ConstructorLayout>,
    pub representation: Representation,
    pub opaque: bool,
    pub summary: Summary,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConstructorLayout {
    pub name: String,
    /// Its constructor index: its own `@tag`, else the type's `@tag`, else its position.
    pub index: usize,
    /// The field types, in terms of the type's parameters.
    pub fields: Vec<Rc<Type>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Representation {
    /// An opaque type with one constructor, one field and no decorator: a value is its field.
    Erased,
    /// `Constr index [fields]`.
    Constr,
    /// `@list`: the list of the fields.
    List,
}

/// What a type's values can contain.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Summary {
    /// For each parameter, whether some field stores a value of it. A parameter that isn't
    /// stored is phantom: it never appears in a value.
    pub stored: Vec<bool>,
    /// What the fields contain, apart from the values of the parameters.
    pub contents: Contents,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Contents {
    /// A function or a `MillerLoopResult`.
    pub no_data_form: bool,
    /// An opaque type.
    pub opaque: bool,
    /// A `String`, `Value`, `G1Element` or `G2Element`, whose conversion from `Data` fails
    /// instead of telling whether the `Data` is one.
    pub untestable: bool,
}

impl Contents {
    fn union(self, other: Contents) -> Contents {
        Contents {
            no_data_form: self.no_data_form || other.no_data_form,
            opaque: self.opaque || other.opaque,
            untestable: self.untestable || other.untestable,
        }
    }
}

impl DataLayout {
    pub fn constructor(&self, name: &str) -> Option<&ConstructorLayout> {
        self.constructors
            .iter()
            .find(|constructor| constructor.name == name)
    }

    pub fn is_erased(&self) -> bool {
        self.representation == Representation::Erased
    }

    pub fn is_list(&self) -> bool {
        self.representation == Representation::List
    }
}

/// Computes the layouts of a module's data types. `imported` finds the layout of a data type
/// defined elsewhere, by module and name; the prelude's module is `""`.
pub fn compute_layouts<'a>(
    module: &str,
    data_types: &[&TypedDataType],
    imported: impl Fn(&str, &str) -> Option<&'a DataLayout>,
) -> HashMap<String, DataLayout> {
    let mut layouts: HashMap<String, DataLayout> = data_types
        .iter()
        .map(|data_type| (data_type.name.clone(), initial_layout(data_type)))
        .collect();

    // The summaries are the least fixed point over the module's types: start from "stores
    // nothing, contains nothing" and grow until nothing changes, so that a parameter which only
    // appears as a phantom argument of a field's type stays phantom.
    loop {
        let mut changed = false;

        for data_type in data_types {
            let summary = {
                let lookup = |type_module: &str, name: &str| {
                    if type_module == module {
                        layouts.get(name)
                    } else {
                        imported(type_module, name)
                    }
                };

                summarise(&layouts[&data_type.name], &lookup)
            };

            let layout = layouts.get_mut(&data_type.name).unwrap();

            if layout.summary != summary {
                layout.summary = summary;
                changed = true;
            }
        }

        if !changed {
            return layouts;
        }
    }
}

fn initial_layout(data_type: &TypedDataType) -> DataLayout {
    let tag = |decorators: &[crate::ast::Decorator]| {
        decorators
            .iter()
            .find_map(|decorator| match decorator.kind {
                DecoratorKind::Tag { value, .. } => Some(value),
                DecoratorKind::List => None,
            })
    };

    let constructors: Vec<ConstructorLayout> = data_type
        .constructors
        .iter()
        .enumerate()
        .map(|(position, constructor)| ConstructorLayout {
            name: constructor.name.clone(),
            index: tag(&constructor.decorators)
                .or_else(|| tag(&data_type.decorators))
                .unwrap_or(position),
            fields: constructor
                .arguments
                .iter()
                .map(|argument| argument.tipo.clone())
                .collect(),
        })
        .collect();

    let has_decorator = !data_type.decorators.is_empty()
        || data_type
            .constructors
            .iter()
            .any(|constructor| !constructor.decorators.is_empty());

    let representation = if data_type
        .decorators
        .iter()
        .any(|decorator| matches!(decorator.kind, DecoratorKind::List))
    {
        Representation::List
    } else if data_type.opaque
        && !has_decorator
        && matches!(&constructors[..], [constructor] if constructor.fields.len() == 1)
    {
        Representation::Erased
    } else {
        Representation::Constr
    };

    DataLayout {
        parameters: data_type.typed_parameters.clone(),
        constructors,
        representation,
        opaque: data_type.opaque,
        summary: Summary {
            stored: vec![false; data_type.typed_parameters.len()],
            contents: Contents::default(),
        },
    }
}

fn summarise<'a>(
    layout: &DataLayout,
    lookup: &impl Fn(&str, &str) -> Option<&'a DataLayout>,
) -> Summary {
    let parameter_ids: Vec<Option<u64>> = layout
        .parameters
        .iter()
        .map(|parameter| parameter.get_generic_id())
        .collect();

    let mut stored = vec![false; parameter_ids.len()];
    let mut contents = Contents::default();

    for field in layout.constructors.iter().flat_map(|c| &c.fields) {
        let mut found = |id: u64| {
            if let Some(index) = parameter_ids.iter().position(|p| *p == Some(id)) {
                stored[index] = true;
            }
        };

        contents = contents.union(stored_contents(field, lookup, &mut found));
    }

    Summary { stored, contents }
}

/// What a value of `tipo` contains in its stored positions, apart from its generic type
/// variables, which it reports to `found`.
fn stored_contents<'a>(
    tipo: &Type,
    lookup: &impl Fn(&str, &str) -> Option<&'a DataLayout>,
    found: &mut impl FnMut(u64),
) -> Contents {
    match tipo {
        Type::Fn { args, ret, .. } => {
            for tipo in args.iter().chain(std::iter::once(ret)) {
                stored_contents(tipo, lookup, found);
            }

            Contents {
                no_data_form: true,
                ..Contents::default()
            }
        }

        Type::Var { tipo, .. } => match tipo.borrow().deref() {
            TypeVar::Link { tipo } => stored_contents(tipo, lookup, found),
            TypeVar::Generic { id } => {
                found(*id);
                Contents::default()
            }
            TypeVar::Unbound { .. } => Contents::default(),
        },

        Type::Tuple { elems, .. } => elems.iter().fold(Contents::default(), |acc, elem| {
            acc.union(stored_contents(elem, lookup, found))
        }),

        Type::Pair { fst, snd, .. } => {
            stored_contents(fst, lookup, found).union(stored_contents(snd, lookup, found))
        }

        Type::App {
            module, name, args, ..
        } => {
            if module.is_empty() {
                match name.as_str() {
                    well_known::MILLER_LOOP_RESULT => {
                        return Contents {
                            no_data_form: true,
                            ..Contents::default()
                        };
                    }
                    well_known::STRING
                    | well_known::VALUE
                    | well_known::G1_ELEMENT
                    | well_known::G2_ELEMENT => {
                        return Contents {
                            untestable: true,
                            ..Contents::default()
                        };
                    }
                    well_known::LIST => {
                        return args.iter().fold(Contents::default(), |acc, arg| {
                            acc.union(stored_contents(arg, lookup, found))
                        });
                    }
                    _ => {}
                }
            }

            match lookup(module, name) {
                Some(layout) => {
                    let mut own = layout.summary.contents;
                    own.opaque |= layout.opaque;

                    args.iter()
                        .zip(&layout.summary.stored)
                        .filter(|(_, stored)| **stored)
                        .fold(own, |acc, (arg, _)| {
                            acc.union(stored_contents(arg, lookup, found))
                        })
                }
                // Primitives (`Int`, `ByteArray`, `Data`, ...) have no layout and no arguments.
                None => args.iter().fold(Contents::default(), |acc, arg| {
                    acc.union(stored_contents(arg, lookup, found))
                }),
            }
        }
    }
}
