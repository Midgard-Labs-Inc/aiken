//! What a cast needs from the types it converts, and the one walker that checks it.
//!
//! A type meets a requirement in its stored positions only: a phantom argument never appears
//! in a value, so it never counts. A type variable met in a stored position records the
//! requirement instead, and its type is checked when it becomes known.

use super::{
    Type, TypeInfo, TypeVar,
    error::{Error, UnifyErrorSituation},
    layout::{DataLayout, find_layout, stored_args, stored_contents},
};
use crate::{
    ast::{AssignmentKind, Definition, Span, TypedModule},
    expr::TypedExpr,
};
use std::{
    collections::{HashMap, HashSet},
    ops::Deref,
    rc::Rc,
};

/// The kinds of cast, each with the requirement it places on the type it converts, from the
/// weakest to the strongest.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub enum Cast {
    /// A value stored in a list, a tuple, a pair or a data type.
    Store,
    /// An operand of `==` or `!=`.
    Compare,
    /// A value converted to `Data`.
    Upcast,
    /// Either side of a `coerce`.
    Coerce,
    /// The type `expect` converts `Data` to.
    Expect,
    /// The type `if … is` converts `Data` to.
    IfIs,
}

/// What a type must contain, from the weakest requirement to the strongest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    /// No function and no `MillerLoopResult`.
    DataForm,
    /// Also no opaque type.
    Decodable,
    /// Also no `String`, `Value`, `G1Element` or `G2Element`, whose conversions fail instead of
    /// telling whether the `Data` is one.
    Testable,
}

impl Cast {
    /// The cast an `expect` or an `if … is` from `Data` makes.
    pub fn from_data<T>(kind: &AssignmentKind<T>) -> Cast {
        if kind.is_expect() {
            Cast::Expect
        } else {
            Cast::IfIs
        }
    }

    pub fn level(self) -> Level {
        match self {
            Cast::Store | Cast::Compare | Cast::Upcast | Cast::Coerce => Level::DataForm,
            Cast::Expect => Level::Decodable,
            Cast::IfIs => Level::Testable,
        }
    }

    /// The error this cast gives for a type with `violation`.
    pub fn error(self, violation: Violation, tipo: Rc<Type>, location: Span) -> Error {
        match (self, violation) {
            (Cast::Store, _) => Error::IllegalTypeInData { tipo, location },
            (Cast::Compare, _) => Error::IllegalComparison { location },
            (Cast::Coerce, _) => Error::IllegalCoerce { tipo, location },
            (Cast::Expect | Cast::IfIs, Violation::Opaque) => {
                Error::ExpectOnOpaqueType { location }
            }
            (Cast::IfIs, Violation::Untestable) => Error::IllegalSoftCast { tipo, location },
            (Cast::Upcast | Cast::Expect | Cast::IfIs, _) => Error::CouldNotUnify {
                location,
                expected: Type::data(),
                given: tipo,
                situation: Some(UnifyErrorSituation::NoDataForm),
                rigid_type_names: HashMap::new(),
            },
        }
    }
}

/// A cast that a type variable's eventual type must allow, and where it happens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Requirement {
    pub cast: Cast,
    pub location: Span,
}

impl Requirement {
    /// Whichever of the two asks more, `self` on a tie.
    pub fn strongest(self, other: Requirement) -> Requirement {
        if other.cast > self.cast { other } else { self }
    }
}

/// The strongest requirement any of `tables` records on the type variable `id`. A variable
/// that modules share, such as a constant's, can collect requirements in each of them.
pub fn strongest<'a>(
    tables: impl IntoIterator<Item = &'a HashMap<u64, Requirement>>,
    id: u64,
) -> Option<Requirement> {
    tables
        .into_iter()
        .filter_map(|table| table.get(&id).copied())
        .reduce(Requirement::strongest)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Violation {
    NoDataForm,
    Opaque,
    Untestable,
}

/// Checks that the stored positions of `tipo` meet `level`, and collects the ids of the type
/// variables they hold, which must meet it once known. `lookup` finds a data type's layout by
/// module and name.
pub fn check<'a>(
    tipo: &Type,
    level: Level,
    lookup: &impl Fn(&str, &str) -> Option<&'a DataLayout>,
    variables: &mut Vec<u64>,
) -> Result<(), Violation> {
    let contents = stored_contents(tipo, lookup, &mut |id| variables.push(id));

    if contents.no_data_form {
        Err(Violation::NoDataForm)
    } else if level >= Level::Testable && contents.untestable {
        Err(Violation::Untestable)
    } else if level >= Level::Decodable && contents.opaque {
        Err(Violation::Opaque)
    } else {
        Ok(())
    }
}

/// Checks that whatever a value of `tipo` stores has a Data form, as `check` does at
/// `Level::DataForm` for each list, tuple, pair and data type in `tipo`. A function, or a
/// `MillerLoopResult`, is fine outside of them.
pub fn check_stored<'a>(
    tipo: &Type,
    lookup: &impl Fn(&str, &str) -> Option<&'a DataLayout>,
    variables: &mut Vec<u64>,
) -> Result<(), Violation> {
    match tipo {
        Type::Fn { args, ret, .. } => {
            for arg in args {
                check_stored(arg, lookup, variables)?;
            }
            check_stored(ret, lookup, variables)
        }

        Type::Var { tipo, .. } => match tipo.borrow().deref() {
            TypeVar::Link { tipo } => check_stored(tipo, lookup, variables),
            TypeVar::Unbound { .. } | TypeVar::Generic { .. } => Ok(()),
        },

        Type::Tuple { elems, .. } => elems
            .iter()
            .try_for_each(|elem| check(elem, Level::DataForm, lookup, variables)),

        Type::Pair { fst, snd, .. } => [fst, snd]
            .into_iter()
            .try_for_each(|elem| check(elem, Level::DataForm, lookup, variables)),

        Type::App {
            module, name, args, ..
        } => stored_args(args, lookup(module, name))
            .try_for_each(|arg| check(arg, Level::DataForm, lookup, variables)),
    }
}

/// Whether a value of `tipo` can hold a value of an opaque type, in its stored positions: a
/// phantom argument holds none. A validator's parameters and an exported function's
/// arguments are applied off-chain without a check, so they can't.
pub fn holds_opaque<'a>(
    tipo: &Type,
    lookup: &impl Fn(&str, &str) -> Option<&'a DataLayout>,
) -> bool {
    stored_contents(tipo, lookup, &mut |_| ()).opaque
}

/// Whether `tipo` mentions an opaque type anywhere, in its fields and phantom arguments
/// included, as a validator's datum and redeemer require.
pub fn mentions_opaque<'a>(
    tipo: &Type,
    lookup: &impl Fn(&str, &str) -> Option<&'a DataLayout>,
) -> bool {
    mentions_opaque_in(tipo, lookup, &mut HashSet::new())
}

fn mentions_opaque_in<'a>(
    tipo: &Type,
    lookup: &impl Fn(&str, &str) -> Option<&'a DataLayout>,
    visited: &mut HashSet<(String, String)>,
) -> bool {
    match tipo {
        Type::App {
            module, name, args, ..
        } => {
            let in_fields = lookup(module, name).is_some_and(|layout| {
                layout.opaque
                    || (visited.insert((module.clone(), name.clone()))
                        && layout
                            .constructors
                            .iter()
                            .flat_map(|constructor| &constructor.fields)
                            .any(|field| mentions_opaque_in(field, lookup, visited)))
            });

            in_fields
                || args
                    .iter()
                    .any(|arg| mentions_opaque_in(arg, lookup, visited))
        }
        Type::Var { tipo, .. } => match tipo.borrow().deref() {
            TypeVar::Link { tipo } => mentions_opaque_in(tipo, lookup, visited),
            TypeVar::Unbound { .. } | TypeVar::Generic { .. } => false,
        },
        Type::Tuple { elems, .. } => elems
            .iter()
            .any(|elem| mentions_opaque_in(elem, lookup, visited)),
        Type::Pair { fst, snd, .. } => {
            mentions_opaque_in(fst, lookup, visited) || mentions_opaque_in(snd, lookup, visited)
        }
        Type::Fn { .. } => false,
    }
}

/// Settles a module's types once every module has type-checked: a type variable that nothing
/// fixed becomes `Data`. So an empty list whose element type nothing fixes is a `List<Data>`,
/// and a hole that nothing fixes, as in `expect x: Option<_> = d`, decodes `Data`.
pub fn default_unknown_types(module: &mut TypedModule) {
    let mut default = |tipo: &Rc<Type>| {
        tipo.default_unbound_to_data();
        tipo.clone()
    };

    for definition in &mut module.definitions {
        definition.map_types(&mut default);
    }

    for value in module.type_info.values.values() {
        value.tipo.default_unbound_to_data();
    }
}

/// Checks every upcast, `expect` and `if … is` of `module` again, now that its types are known,
/// and panics on one that its types don't allow. `module_types` holds every module's types.
/// Projects run it after `default_unknown_types`.
pub fn recheck_casts(module: &TypedModule, module_types: &HashMap<String, TypeInfo>) {
    let lookup = |module: &str, name: &str| find_layout(module_types, module, name);

    let recheck = |tipo: &Type, cast: Cast| {
        let mut variables = vec![];

        if let Err(violation) = check(tipo, cast.level(), &lookup, &mut variables) {
            panic!(
                "{cast:?} to or from {} was accepted, but is {violation:?}",
                tipo.to_pretty(0)
            );
        }

        for id in variables {
            let known = strongest(module_types.values().map(|info| &info.requirements), id);
            assert!(
                known.is_some_and(|known| known.cast.level() >= cast.level()),
                "{cast:?} to or from {} doesn't constrain its type variable {id}",
                tipo.to_pretty(0)
            );
        }
    };

    let mut recheck_expression = |expression: &TypedExpr| match expression {
        TypedExpr::Upcast { value, .. } => recheck(&value.tipo(), Cast::Upcast),
        TypedExpr::Assignment {
            kind, value, tipo, ..
        } if kind.is_expect() && value.tipo().is_data() && !tipo.is_data() => {
            recheck(tipo, Cast::Expect)
        }
        TypedExpr::If { branches, .. } => {
            for branch in branches {
                if let Some((_, tipo)) = &branch.is
                    && branch.condition.tipo().is_data()
                    && !tipo.is_data()
                {
                    recheck(tipo, Cast::IfIs);
                }
            }
        }
        _ => {}
    };

    for definition in &module.definitions {
        match definition {
            Definition::Fn(function) => function.body.walk(&mut recheck_expression),
            Definition::Test(test) | Definition::Benchmark(test) => {
                for argument in &test.arguments {
                    argument.via.walk(&mut recheck_expression);
                }
                test.body.walk(&mut recheck_expression);
            }
            Definition::Validator(validator) => {
                for handler in validator.handlers.iter().chain([&validator.fallback]) {
                    handler.body.walk(&mut recheck_expression);
                }
            }
            Definition::ModuleConstant(constant) => constant.value.walk(&mut recheck_expression),
            Definition::TypeAlias(_) | Definition::DataType(_) | Definition::Use(_) => {}
        }
    }
}
