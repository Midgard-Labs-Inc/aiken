//! How the values of a data type look at runtime, computed once per data type.
//!
//! Codegen reads the constructor indices, field types and representation. The type checker
//! reads the summary, which says what a type's values can contain, so it never needs to look a
//! type's constructors up by name.

use super::{Type, TypeInfo, TypeVar, find_and_replace_generics};
use crate::{
    ast::{DecoratorKind, TypedDataType, well_known},
    builtins::PRELUDE,
};
use indexmap::IndexMap;
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
    pub labels: Vec<Option<String>>,
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
    /// A `Value`, `G1Element` or `G2Element`, which casts don't convert yet.
    pub value_or_point: bool,
}

impl Contents {
    fn union(self, other: Contents) -> Contents {
        Contents {
            no_data_form: self.no_data_form || other.no_data_form,
            opaque: self.opaque || other.opaque,
            untestable: self.untestable || other.untestable,
            value_or_point: self.value_or_point || other.value_or_point,
        }
    }
}

impl DataLayout {
    pub fn constructor(&self, name: &str) -> Option<&ConstructorLayout> {
        self.constructors
            .iter()
            .find(|constructor| constructor.name == name)
    }

    pub fn constructor_at(&self, index: usize) -> Option<&ConstructorLayout> {
        self.constructors
            .iter()
            .find(|constructor| constructor.index == index)
    }

    pub fn is_erased(&self) -> bool {
        self.representation == Representation::Erased
    }

    pub fn is_list(&self) -> bool {
        self.representation == Representation::List
    }

    /// The field types of `constructor`, in a value of this type applied to `args`.
    pub fn field_types(&self, constructor: &ConstructorLayout, args: &[Rc<Type>]) -> Vec<Rc<Type>> {
        let mono_types = self
            .parameters
            .iter()
            .zip(args)
            .filter_map(|(parameter, arg)| Some((parameter.get_generic_id()?, arg.clone())))
            .collect();

        constructor
            .fields
            .iter()
            .map(|field| find_and_replace_generics(field, &mono_types))
            .collect()
    }
}

/// The layout of the data type `tipo` is (or returns, for a constructor), looked up by its
/// module and name in `module_types`.
pub fn lookup_layout<'a>(
    module_types: &IndexMap<&str, &'a TypeInfo>,
    tipo: &Type,
) -> Option<&'a DataLayout> {
    match tipo {
        Type::App { module, name, .. } => {
            let module = if module.is_empty() { PRELUDE } else { module };

            module_types.get(module)?.layouts.get(name)
        }
        Type::Fn { ret, .. } => lookup_layout(module_types, ret),
        Type::Var { tipo, .. } => match tipo.borrow().deref() {
            TypeVar::Link { tipo } => lookup_layout(module_types, tipo),
            TypeVar::Unbound { .. } | TypeVar::Generic { .. } => None,
        },
        Type::Tuple { .. } | Type::Pair { .. } => None,
    }
}

/// The layout of the data type `name` defined in `module`, among `modules`. Types name the
/// prelude's module `""`.
pub fn find_layout<'a>(
    modules: &'a HashMap<String, TypeInfo>,
    module: &str,
    name: &str,
) -> Option<&'a DataLayout> {
    let module = if module.is_empty() { PRELUDE } else { module };

    modules.get(module)?.layouts.get(name)
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

    // `Never`'s first constructor is a placeholder: its only value is `Constr 1 []`.
    let placeholders = usize::from(data_type.is_never());

    let constructors: Vec<ConstructorLayout> = data_type
        .constructors
        .iter()
        .enumerate()
        .skip(placeholders)
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
            labels: constructor
                .arguments
                .iter()
                .map(|argument| argument.label.clone())
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

/// What a value of `tipo` contains in its stored positions, apart from its type variables,
/// which it reports to `found`.
pub(super) fn stored_contents<'a>(
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
            TypeVar::Unbound { id } | TypeVar::Generic { id } => {
                found(*id);
                Contents::default()
            }
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
                    well_known::STRING => {
                        return Contents {
                            untestable: true,
                            ..Contents::default()
                        };
                    }
                    well_known::VALUE | well_known::G1_ELEMENT | well_known::G2_ELEMENT => {
                        return Contents {
                            untestable: true,
                            value_or_point: true,
                            ..Contents::default()
                        };
                    }
                    _ => {}
                }
            }

            let layout = lookup(module, name);
            let own = layout.map_or(Contents::default(), |layout| Contents {
                opaque: layout.summary.contents.opaque || layout.opaque,
                ..layout.summary.contents
            });

            stored_args(args, layout).fold(own, |acc, arg| {
                acc.union(stored_contents(arg, lookup, found))
            })
        }
    }
}

/// The arguments of a type with `layout` that its values store. A type without a layout, such
/// as `List` or a primitive (`Int`, `ByteArray`, `Data`, ...), stores every argument.
pub(super) fn stored_args<'a>(
    args: &'a [Rc<Type>],
    layout: Option<&'a DataLayout>,
) -> impl Iterator<Item = &'a Rc<Type>> {
    args.iter().enumerate().filter_map(move |(at, arg)| {
        layout
            .is_none_or(|layout| layout.summary.stored.get(at) == Some(&true))
            .then_some(arg)
    })
}
