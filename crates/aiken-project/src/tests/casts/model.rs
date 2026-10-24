//! A reference model of the casts through `Data`, written from their specification
//! (`site/src/content/docs/casts.mdx`) and from nothing else. It shares no code
//! with the compiler: it has its own description of types and values, its own encodings and
//! decodings, and its own typing requirements. It also prints its types and values as Aiken
//! source, so that the compiler can be asked the same questions.

use num_bigint::{BigInt, Sign};
use std::fmt::{self, Display};
use uplc::{PlutusData, ast::Data};

/// A `Data` value: `Constr i [d…]`, `Map [(k, v)…]`, `List [d…]`, `I n` or `B bytes`.
#[derive(Clone, Debug, PartialEq)]
pub enum D {
    Constr(u64, Vec<D>),
    Map(Vec<(D, D)>),
    List(Vec<D>),
    I(BigInt),
    B(Vec<u8>),
}

impl D {
    pub fn int(n: i64) -> D {
        D::I(n.into())
    }

    pub fn from_plutus(data: &PlutusData) -> D {
        match data {
            PlutusData::Constr(constr) => {
                let index = match constr.tag {
                    121..=127 => constr.tag - 121,
                    1280..=1400 => constr.tag - 1280 + 7,
                    _ => constr.any_constructor.expect("a constructor index"),
                };

                D::Constr(index, constr.fields.iter().map(D::from_plutus).collect())
            }
            PlutusData::Map(entries) => D::Map(
                entries
                    .iter()
                    .map(|(key, value)| (D::from_plutus(key), D::from_plutus(value)))
                    .collect(),
            ),
            PlutusData::Array(items) => D::List(items.iter().map(D::from_plutus).collect()),
            PlutusData::BigInt(uplc::BigInt::Int(n)) => D::I(i128::from(*n).into()),
            PlutusData::BigInt(uplc::BigInt::BigUInt(bytes)) => {
                D::I(BigInt::from_bytes_be(Sign::Plus, bytes))
            }
            PlutusData::BigInt(uplc::BigInt::BigNInt(bytes)) => {
                D::I(-1 - BigInt::from_bytes_be(Sign::Plus, bytes))
            }
            PlutusData::BoundedBytes(bytes) => D::B(bytes.to_vec()),
        }
    }

    pub fn to_plutus(&self) -> PlutusData {
        PlutusData::from(&self.to_data())
    }

    fn to_data(&self) -> Data {
        match self {
            D::Constr(index, fields) => {
                Data::constr(*index, fields.iter().map(D::to_data).collect())
            }
            D::Map(entries) => Data::map(
                entries
                    .iter()
                    .map(|(key, value)| (key.to_data(), value.to_data()))
                    .collect(),
            ),
            D::List(items) => Data::list(items.iter().map(D::to_data).collect()),
            D::I(n) => Data::integer(n.clone()),
            D::B(bytes) => Data::bytestring(bytes.clone()),
        }
    }
}

impl Display for D {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fn all<T>(
            f: &mut fmt::Formatter<'_>,
            items: &[T],
            show: impl Fn(&mut fmt::Formatter<'_>, &T) -> fmt::Result,
        ) -> fmt::Result {
            for (position, item) in items.iter().enumerate() {
                if position > 0 {
                    write!(f, ", ")?;
                }
                show(f, item)?;
            }
            Ok(())
        }

        match self {
            D::Constr(index, fields) => {
                write!(f, "Constr {index} [")?;
                all(f, fields, |f, field| write!(f, "{field}"))?;
                write!(f, "]")
            }
            D::Map(entries) => {
                write!(f, "Map [")?;
                all(f, entries, |f, (key, value)| write!(f, "({key}, {value})"))?;
                write!(f, "]")
            }
            D::List(items) => {
                write!(f, "List [")?;
                all(f, items, |f, item| write!(f, "{item}"))?;
                write!(f, "]")
            }
            D::I(n) => write!(f, "I {n}"),
            D::B(bytes) => write!(f, "B #{}", hex::encode(bytes)),
        }
    }
}

/// A type, as the tests describe it.
#[derive(Clone, Debug, PartialEq)]
pub enum Ty {
    Int,
    ByteArray,
    String,
    Bool,
    Void,
    Data,
    Value,
    G1,
    G2,
    MillerLoopResult,
    /// `fn(Int) -> Int`.
    Fn,
    Option(Box<Ty>),
    /// A `List<Pair<k, v>>` is a `Pairs<k, v>`.
    List(Box<Ty>),
    Pair(Box<Ty>, Box<Ty>),
    Tuple(Vec<Ty>),
    /// A user data type, by position in the definitions, with its type arguments.
    Named(usize, Vec<Ty>),
    /// A type parameter of the definition the type appears in.
    Param(usize),
}

impl Ty {
    pub fn list(item: Ty) -> Ty {
        Ty::List(Box::new(item))
    }

    pub fn pairs(key: Ty, value: Ty) -> Ty {
        Ty::list(Ty::pair(key, value))
    }

    pub fn option(item: Ty) -> Ty {
        Ty::Option(Box::new(item))
    }

    pub fn pair(fst: Ty, snd: Ty) -> Ty {
        Ty::Pair(Box::new(fst), Box::new(snd))
    }

    fn substitute(&self, args: &[Ty]) -> Ty {
        match self {
            Ty::Param(position) => args[*position].clone(),
            Ty::Option(item) => Ty::option(item.substitute(args)),
            Ty::List(item) => Ty::list(item.substitute(args)),
            Ty::Pair(fst, snd) => Ty::pair(fst.substitute(args), snd.substitute(args)),
            Ty::Tuple(items) => Ty::Tuple(items.iter().map(|item| item.substitute(args)).collect()),
            Ty::Named(def, own) => {
                Ty::Named(*def, own.iter().map(|arg| arg.substitute(args)).collect())
            }
            leaf => leaf.clone(),
        }
    }

    /// The type as Aiken source, with the parameters of the definition it appears in named
    /// `a`, `b`, …
    pub fn source(&self) -> String {
        match self {
            Ty::Int => "Int".into(),
            Ty::ByteArray => "ByteArray".into(),
            Ty::String => "String".into(),
            Ty::Bool => "Bool".into(),
            Ty::Void => "Void".into(),
            Ty::Data => "Data".into(),
            Ty::Value => "Value".into(),
            Ty::G1 => "G1Element".into(),
            Ty::G2 => "G2Element".into(),
            Ty::MillerLoopResult => "MillerLoopResult".into(),
            Ty::Fn => "fn(Int) -> Int".into(),
            Ty::Option(item) => format!("Option<{}>", item.source()),
            Ty::List(item) => match item.as_ref() {
                Ty::Pair(key, value) => format!("Pairs<{}, {}>", key.source(), value.source()),
                item => format!("List<{}>", item.source()),
            },
            Ty::Pair(fst, snd) => format!("Pair<{}, {}>", fst.source(), snd.source()),
            Ty::Tuple(items) => format!("({})", join(items.iter().map(Ty::source))),
            Ty::Named(def, args) if args.is_empty() => type_name(*def),
            Ty::Named(def, args) => {
                format!("{}<{}>", type_name(*def), join(args.iter().map(Ty::source)))
            }
            Ty::Param(position) => param_name(*position),
        }
    }
}

/// A user data type definition.
#[derive(Clone, Debug)]
pub struct TypeDef {
    pub params: usize,
    pub opaque: bool,
    pub tag: Tag,
    /// `type R { a: Int }`: one constructor named like the type, with labelled fields.
    pub shorthand: bool,
    pub ctors: Vec<Ctor>,
}

#[derive(Clone, Debug)]
pub struct Ctor {
    pub tag: Option<u64>,
    pub labelled: bool,
    pub fields: Vec<Ty>,
}

/// The policies of a `Value` in ascending order, each with its assets in ascending order and
/// their quantities.
pub type Policies = Vec<(Vec<u8>, Vec<(Vec<u8>, BigInt)>)>;

/// The decorator on the type itself.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tag {
    None,
    Fixed(u64),
    List,
}

/// A value of a type.
#[derive(Clone, Debug, PartialEq)]
pub enum V {
    Int(BigInt),
    Bytes(Vec<u8>),
    Str(String),
    Bool(bool),
    Void,
    Data(D),
    Value(Policies),
    /// A point, as its compressed bytes.
    G1(Vec<u8>),
    G2(Vec<u8>),
    Option(Option<Box<V>>),
    List(Vec<V>),
    Pair(Box<V>, Box<V>),
    Tuple(Vec<V>),
    /// The position of a constructor in its type, with its fields.
    Ctor(usize, Vec<V>),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Cast {
    Upcast,
    Expect,
    IfIs,
    Coerce,
}

const OPAQUE: u8 = 1;
/// A `String`, `Value`, `G1Element` or `G2Element`.
const SOFT: u8 = 2;
/// A `MillerLoopResult` or a function.
const NO_DATA_FORM: u8 = 4;

/// The most tokens (pairs of policy and asset name) `valueData` takes.
const VALUE_DATA_MAX_TOKENS: usize = 40_000;

pub struct Model {
    defs: Vec<TypeDef>,
    /// Whether a parameter is stored: the least set such that some field's type mentions it
    /// outside a phantom position.
    stored: Vec<Vec<bool>>,
    /// What the fields of each type contain, without what its parameters add.
    intrinsic: Vec<u8>,
}

impl Model {
    pub fn new(defs: Vec<TypeDef>) -> Model {
        let mut stored: Vec<Vec<bool>> = defs.iter().map(|def| vec![false; def.params]).collect();

        loop {
            let mut changed = false;

            for (d, def) in defs.iter().enumerate() {
                for p in 0..def.params {
                    if !stored[d][p]
                        && def
                            .ctors
                            .iter()
                            .flat_map(|ctor| &ctor.fields)
                            .any(|field| mentions(field, p, &stored))
                    {
                        stored[d][p] = true;
                        changed = true;
                    }
                }
            }

            if !changed {
                break;
            }
        }

        let mut intrinsic = vec![0; defs.len()];

        loop {
            let next: Vec<u8> = defs
                .iter()
                .map(|def| {
                    def.ctors
                        .iter()
                        .flat_map(|ctor| &ctor.fields)
                        .fold(0, |flags, field| {
                            flags | contains(field, &defs, &stored, &intrinsic)
                        })
                })
                .collect();

            if next == intrinsic {
                break;
            }

            intrinsic = next;
        }

        Model {
            defs,
            stored,
            intrinsic,
        }
    }

    pub fn defs(&self) -> &[TypeDef] {
        &self.defs
    }

    pub fn is_stored(&self, def: usize, param: usize) -> bool {
        self.stored[def][param]
    }

    /// An opaque type with one constructor, one field and no decorator is erased.
    pub fn is_erased(&self, def: usize) -> bool {
        let def = &self.defs[def];

        def.opaque
            && def.tag == Tag::None
            && matches!(def.ctors.as_slice(), [ctor] if ctor.fields.len() == 1 && ctor.tag.is_none())
    }

    /// The index of a constructor: its own tag, else the type's tag, else its position.
    pub fn index(&self, def: usize, ctor: usize) -> u64 {
        let def = &self.defs[def];

        def.ctors[ctor].tag.unwrap_or(match def.tag {
            Tag::Fixed(tag) => tag,
            _ => ctor as u64,
        })
    }

    /// Whether the typing rules let `ty` take part in `cast`:
    /// - upcast and `coerce` need a Data form;
    /// - `expect` needs it to be decodable too: a Data form and no opaque type;
    /// - `if … is` needs it to be testable: decodable, and no `String`, `Value` or point.
    ///
    /// Only the stored positions of `ty` count.
    pub fn allows(&self, cast: Cast, ty: &Ty) -> bool {
        let flags = contains(ty, &self.defs, &self.stored, &self.intrinsic);

        let refused = match cast {
            Cast::Upcast | Cast::Coerce => NO_DATA_FORM,
            Cast::Expect => NO_DATA_FORM | OPAQUE,
            Cast::IfIs => NO_DATA_FORM | OPAQUE | SOFT,
        };

        flags & refused == 0
    }

    /// The types of the fields of constructor `ctor` of `def`, at `args`.
    pub fn fields(&self, def: usize, args: &[Ty], ctor: usize) -> Vec<Ty> {
        self.defs[def].ctors[ctor]
            .fields
            .iter()
            .map(|field| field.substitute(args))
            .collect()
    }

    /// The Data form of `value`, a `ty`. The only failure is `valueData` on more than 40,000
    /// tokens.
    pub fn encode(&self, ty: &Ty, value: &V) -> Option<D> {
        let all = |tys: Vec<&Ty>, values: &[V]| -> Option<Vec<D>> {
            tys.into_iter()
                .zip(values)
                .map(|(ty, value)| self.encode(ty, value))
                .collect()
        };

        Some(match (ty, value) {
            (Ty::Int, V::Int(n)) => D::I(n.clone()),
            (Ty::ByteArray, V::Bytes(bytes)) => D::B(bytes.clone()),
            (Ty::String, V::Str(text)) => D::B(text.as_bytes().to_vec()),
            (Ty::Bool, V::Bool(b)) => D::Constr(u64::from(*b), vec![]),
            (Ty::Void, V::Void) => D::Constr(0, vec![]),
            (Ty::Data, V::Data(d)) => d.clone(),
            (Ty::Value, V::Value(policies)) => {
                let tokens: usize = policies.iter().map(|(_, assets)| assets.len()).sum();

                if tokens > VALUE_DATA_MAX_TOKENS {
                    return None;
                }

                D::Map(
                    policies
                        .iter()
                        .map(|(policy, assets)| {
                            let assets = assets
                                .iter()
                                .map(|(name, quantity)| {
                                    (D::B(name.clone()), D::I(quantity.clone()))
                                })
                                .collect();

                            (D::B(policy.clone()), D::Map(assets))
                        })
                        .collect(),
                )
            }
            (Ty::G1 | Ty::G2, V::G1(bytes) | V::G2(bytes)) => D::B(bytes.clone()),
            (Ty::Option(item), V::Option(Some(x))) => D::Constr(0, vec![self.encode(item, x)?]),
            (Ty::Option(_), V::Option(None)) => D::Constr(1, vec![]),
            (Ty::List(item), V::List(xs)) => match item.as_ref() {
                Ty::Pair(key, val) => D::Map(
                    xs.iter()
                        .map(|x| match x {
                            V::Pair(k, v) => Some((self.encode(key, k)?, self.encode(val, v)?)),
                            other => panic!("not a pair: {other:?}"),
                        })
                        .collect::<Option<_>>()?,
                ),
                item => D::List(
                    xs.iter()
                        .map(|x| self.encode(item, x))
                        .collect::<Option<_>>()?,
                ),
            },
            (Ty::Pair(a, b), V::Pair(x, y)) => {
                D::List(vec![self.encode(a, x)?, self.encode(b, y)?])
            }
            (Ty::Tuple(tys), V::Tuple(xs)) => D::List(all(tys.iter().collect(), xs)?),
            (Ty::Named(def, args), V::Ctor(ctor, fields)) => {
                let tys = self.fields(*def, args, *ctor);
                let encoded = all(tys.iter().collect(), fields)?;

                if self.is_erased(*def) {
                    encoded.into_iter().next().unwrap()
                } else if self.defs[*def].tag == Tag::List {
                    D::List(encoded)
                } else {
                    D::Constr(self.index(*def, *ctor), encoded)
                }
            }
            (ty, value) => panic!("{value:?} is not a value of {ty:?}"),
        })
    }

    /// The value of type `ty` that `d` is the Data form of, which is what `expect x: ty = d`
    /// and `if d is x: ty` bind: `None` when `d` is the Data form of no value of that type,
    /// which makes `expect` fail and `if` take `else`. Using the bound value can still fail, as
    /// `encode` can.
    pub fn decode(&self, ty: &Ty, d: &D) -> Option<V> {
        match (ty, d) {
            (Ty::Int, D::I(n)) => Some(V::Int(n.clone())),
            (Ty::ByteArray, D::B(bytes)) => Some(V::Bytes(bytes.clone())),
            (Ty::String, D::B(bytes)) => String::from_utf8(bytes.clone()).ok().map(V::Str),
            (Ty::Bool, D::Constr(index @ (0 | 1), fields)) if fields.is_empty() => {
                Some(V::Bool(*index == 1))
            }
            (Ty::Void, D::Constr(0, fields)) if fields.is_empty() => Some(V::Void),
            (Ty::Data, d) => Some(V::Data(d.clone())),
            (Ty::Value, d) => decode_value(d).map(V::Value),
            (Ty::G1, D::B(bytes)) => valid_g1(bytes).then(|| V::G1(bytes.clone())),
            (Ty::G2, D::B(bytes)) => valid_g2(bytes).then(|| V::G2(bytes.clone())),
            (Ty::Option(item), D::Constr(0, fields)) => match fields.as_slice() {
                [x] => Some(V::Option(Some(Box::new(self.decode(item, x)?)))),
                _ => None,
            },
            (Ty::Option(_), D::Constr(1, fields)) if fields.is_empty() => Some(V::Option(None)),
            (Ty::List(item), d) => match (item.as_ref(), d) {
                (Ty::Pair(key, val), D::Map(entries)) => entries
                    .iter()
                    .map(|(k, v)| {
                        Some(V::Pair(
                            Box::new(self.decode(key, k)?),
                            Box::new(self.decode(val, v)?),
                        ))
                    })
                    .collect::<Option<_>>()
                    .map(V::List),
                (Ty::Pair(..), _) => None,
                (item, D::List(xs)) => xs
                    .iter()
                    .map(|x| self.decode(item, x))
                    .collect::<Option<_>>()
                    .map(V::List),
                _ => None,
            },
            (Ty::Pair(a, b), D::List(xs)) => match xs.as_slice() {
                [x, y] => Some(V::Pair(
                    Box::new(self.decode(a, x)?),
                    Box::new(self.decode(b, y)?),
                )),
                _ => None,
            },
            (Ty::Tuple(tys), D::List(xs)) if tys.len() == xs.len() => tys
                .iter()
                .zip(xs)
                .map(|(ty, x)| self.decode(ty, x))
                .collect::<Option<_>>()
                .map(V::Tuple),
            (Ty::Named(def, args), d) => self.decode_named(*def, args, d),
            _ => None,
        }
    }

    fn decode_named(&self, def: usize, args: &[Ty], d: &D) -> Option<V> {
        let decode_fields = |ctor: usize, fields: &[D]| {
            let tys = self.fields(def, args, ctor);

            if tys.len() != fields.len() {
                return None;
            }

            tys.iter()
                .zip(fields)
                .map(|(ty, field)| self.decode(ty, field))
                .collect::<Option<Vec<V>>>()
        };

        if self.is_erased(def) {
            let tys = self.fields(def, args, 0);

            return Some(V::Ctor(0, vec![self.decode(&tys[0], d)?]));
        }

        if self.defs[def].tag == Tag::List {
            return match d {
                D::List(fields) => Some(V::Ctor(0, decode_fields(0, fields)?)),
                _ => None,
            };
        }

        match d {
            D::Constr(index, fields) => {
                let ctor = (0..self.defs[def].ctors.len())
                    .find(|ctor| self.index(def, *ctor) == *index)?;

                Some(V::Ctor(ctor, decode_fields(ctor, fields)?))
            }
            _ => None,
        }
    }

    /// The definitions as Aiken source.
    pub fn definitions(&self) -> String {
        let mut source = String::new();

        for (d, def) in self.defs.iter().enumerate() {
            match def.tag {
                Tag::None => {}
                Tag::Fixed(tag) => source.push_str(&format!("@tag({tag})\n")),
                Tag::List => source.push_str("@list\n"),
            }

            let params = if def.params == 0 {
                String::new()
            } else {
                format!("<{}>", join((0..def.params).map(param_name)))
            };

            source.push_str(&format!(
                "pub {}type {}{params} {{\n",
                if def.opaque { "opaque " } else { "" },
                type_name(d)
            ));

            if def.shorthand {
                for (f, field) in def.ctors[0].fields.iter().enumerate() {
                    source.push_str(&format!("  f{f}: {},\n", field.source()));
                }
            } else {
                for (c, ctor) in def.ctors.iter().enumerate() {
                    if let Some(tag) = ctor.tag {
                        source.push_str(&format!("  @tag({tag})\n"));
                    }

                    source.push_str(&format!("  {}", self.ctor_name(d, c)));

                    if !ctor.fields.is_empty() && ctor.labelled {
                        let fields = ctor
                            .fields
                            .iter()
                            .enumerate()
                            .map(|(f, field)| format!("f{f}: {}", field.source()));

                        source.push_str(&format!(" {{ {} }}", join(fields)));
                    } else if !ctor.fields.is_empty() {
                        source.push_str(&format!("({})", join(ctor.fields.iter().map(Ty::source))));
                    }

                    source.push('\n');
                }
            }

            source.push_str("}\n\n");
        }

        source
    }

    fn ctor_name(&self, def: usize, ctor: usize) -> String {
        if self.defs[def].shorthand {
            type_name(def)
        } else {
            format!("K{def}_{ctor}")
        }
    }

    /// An expression of type `ty` with value `value`. The expression is only meant to be
    /// checked against an annotation: an empty list, for instance, doesn't say what it is a
    /// list of.
    pub fn literal(&self, ty: &Ty, value: &V) -> String {
        match (ty, value) {
            (_, V::Int(n)) => n.to_string(),
            (_, V::Bytes(bytes)) => format!("#\"{}\"", hex::encode(bytes)),
            (_, V::Str(text)) => format!("@\"{text}\""),
            (_, V::Bool(true)) => "True".into(),
            (_, V::Bool(false)) => "False".into(),
            (_, V::Void) => "Void".into(),
            (_, V::Data(d)) => data_literal(d),
            (_, V::Value(policies)) => {
                let mut value = "builtin.un_value_data(builtin.map_data([]))".to_string();

                for (policy, assets) in policies {
                    for (name, quantity) in assets {
                        value = format!(
                            "builtin.insert_value(#\"{}\", #\"{}\", {quantity}, {value})",
                            hex::encode(policy),
                            hex::encode(name)
                        );
                    }
                }

                value
            }
            (_, V::G1(bytes)) => {
                format!(
                    "builtin.bls12_381_g1_uncompress(#\"{}\")",
                    hex::encode(bytes)
                )
            }
            (_, V::G2(bytes)) => {
                format!(
                    "builtin.bls12_381_g2_uncompress(#\"{}\")",
                    hex::encode(bytes)
                )
            }
            (Ty::Option(item), V::Option(Some(x))) => format!("Some({})", self.literal(item, x)),
            (_, V::Option(None)) => "None".into(),
            (Ty::List(item), V::List(xs)) => {
                format!("[{}]", join(xs.iter().map(|x| self.literal(item, x))))
            }
            (Ty::Pair(a, b), V::Pair(x, y)) => {
                format!("Pair({}, {})", self.literal(a, x), self.literal(b, y))
            }
            (Ty::Tuple(tys), V::Tuple(xs)) => format!(
                "({})",
                join(tys.iter().zip(xs).map(|(ty, x)| self.literal(ty, x)))
            ),
            (Ty::Named(def, args), V::Ctor(ctor, fields)) => {
                let name = self.ctor_name(*def, *ctor);

                if fields.is_empty() {
                    name
                } else {
                    let tys = self.fields(*def, args, *ctor);
                    let declared = &self.defs[*def].ctors[*ctor].fields;

                    // A field declared as `Data` takes an integer or a byte string as is:
                    // the constructor's argument is upcast.
                    let fields = tys
                        .iter()
                        .zip(fields)
                        .zip(declared)
                        .map(|((ty, x), declared)| match (declared, x) {
                            (Ty::Data, V::Data(D::I(n))) => n.to_string(),
                            (Ty::Data, V::Data(D::B(bytes))) => {
                                format!("#\"{}\"", hex::encode(bytes))
                            }
                            _ => self.literal(ty, x),
                        });

                    format!("{name}({})", join(fields))
                }
            }
            (ty, value) => panic!("{value:?} is not a value of {ty:?}"),
        }
    }
}

pub fn power_of_two(exponent: u32) -> BigInt {
    BigInt::from(1u8) << exponent
}

pub fn type_name(def: usize) -> String {
    format!("T{def}")
}

fn param_name(position: usize) -> String {
    char::from(b'a' + position as u8).to_string()
}

fn join(items: impl Iterator<Item = String>) -> String {
    items.collect::<Vec<_>>().join(", ")
}

/// A `Data` value, built with the builtins.
fn data_literal(d: &D) -> String {
    match d {
        D::Constr(index, fields) => format!(
            "builtin.constr_data({index}, [{}])",
            join(fields.iter().map(data_literal))
        ),
        D::Map(entries) => format!(
            "builtin.map_data([{}])",
            join(entries.iter().map(|(k, v)| format!(
                "Pair({}, {})",
                data_literal(k),
                data_literal(v)
            )))
        ),
        D::List(items) => format!(
            "builtin.list_data([{}])",
            join(items.iter().map(data_literal))
        ),
        D::I(n) => format!("builtin.i_data({n})"),
        D::B(bytes) => format!("builtin.b_data(#\"{}\")", hex::encode(bytes)),
    }
}

fn mentions(ty: &Ty, param: usize, stored: &[Vec<bool>]) -> bool {
    match ty {
        Ty::Param(position) => *position == param,
        Ty::Option(item) | Ty::List(item) => mentions(item, param, stored),
        Ty::Pair(fst, snd) => mentions(fst, param, stored) || mentions(snd, param, stored),
        Ty::Tuple(items) => items.iter().any(|item| mentions(item, param, stored)),
        Ty::Named(def, args) => args
            .iter()
            .enumerate()
            .any(|(position, arg)| stored[*def][position] && mentions(arg, param, stored)),
        _ => false,
    }
}

/// What the stored positions of `ty` contain: opaque types, `String`s and the like, and types
/// without a Data form. A type parameter contains nothing: what it stands for is counted
/// where it is an argument.
fn contains(ty: &Ty, defs: &[TypeDef], stored: &[Vec<bool>], intrinsic: &[u8]) -> u8 {
    let of = |ty: &Ty| contains(ty, defs, stored, intrinsic);

    match ty {
        Ty::String | Ty::Value | Ty::G1 | Ty::G2 => SOFT,
        Ty::MillerLoopResult | Ty::Fn => NO_DATA_FORM,
        Ty::Option(item) | Ty::List(item) => of(item),
        Ty::Pair(fst, snd) => of(fst) | of(snd),
        Ty::Tuple(items) => items.iter().fold(0, |flags, item| flags | of(item)),
        Ty::Named(def, args) => args
            .iter()
            .enumerate()
            .filter(|(position, _)| stored[*def][*position])
            .fold(
                intrinsic[*def] | if defs[*def].opaque { OPAQUE } else { 0 },
                |flags, (_, arg)| flags | of(arg),
            ),
        Ty::Int | Ty::ByteArray | Ty::Bool | Ty::Void | Ty::Data | Ty::Param(_) => 0,
    }
}

/// `unValueData`: policies and asset names of at most 32 bytes in strictly ascending order,
/// no empty asset map, and non-zero quantities within the signed 128-bit range.
fn decode_value(d: &D) -> Option<Policies> {
    fn ascending_keys(entries: &[(D, D)]) -> Option<Vec<(&Vec<u8>, &D)>> {
        let keys: Vec<(&Vec<u8>, &D)> = entries
            .iter()
            .map(|(key, value)| match key {
                D::B(bytes) if bytes.len() <= 32 => Some((bytes, value)),
                _ => None,
            })
            .collect::<Option<_>>()?;

        keys.windows(2)
            .all(|pair| pair[0].0 < pair[1].0)
            .then_some(keys)
    }

    let low = -power_of_two(127);
    let high = power_of_two(127) - 1;

    let D::Map(policies) = d else { return None };

    ascending_keys(policies)?
        .into_iter()
        .map(|(policy, assets)| {
            let D::Map(assets) = assets else { return None };

            if assets.is_empty() {
                return None;
            }

            let assets = ascending_keys(assets)?
                .into_iter()
                .map(|(name, quantity)| match quantity {
                    D::I(n) if *n != BigInt::from(0) && *n >= low && *n <= high => {
                        Some((name.clone(), n.clone()))
                    }
                    _ => None,
                })
                .collect::<Option<Vec<_>>>()?;

            Some((policy.clone(), assets))
        })
        .collect()
}

/// A compressed G1 point that `bls12_381_G1_uncompress` accepts: 48 bytes of a point of the
/// subgroup (CIP-0381).
fn valid_g1(bytes: &[u8]) -> bool {
    if bytes.len() != 48 {
        return false;
    }

    let mut affine = blst::blst_p1_affine::default();
    let mut point = blst::blst_p1::default();

    unsafe {
        blst::blst_p1_uncompress(&mut affine, bytes.as_ptr()) == blst::BLST_ERROR::BLST_SUCCESS && {
            blst::blst_p1_from_affine(&mut point, &affine);
            blst::blst_p1_in_g1(&point)
        }
    }
}

/// The same for G2, with 96 bytes.
fn valid_g2(bytes: &[u8]) -> bool {
    if bytes.len() != 96 {
        return false;
    }

    let mut affine = blst::blst_p2_affine::default();
    let mut point = blst::blst_p2::default();

    unsafe {
        blst::blst_p2_uncompress(&mut affine, bytes.as_ptr()) == blst::BLST_ERROR::BLST_SUCCESS && {
            blst::blst_p2_from_affine(&mut point, &affine);
            blst::blst_p2_in_g2(&point)
        }
    }
}
