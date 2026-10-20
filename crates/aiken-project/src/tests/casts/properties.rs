//! Properties of the casts through `Data`, against the reference model of `model.rs`.
//!
//! Each case is one module: generated data types, and for each of a few generated types the
//! programs that observe a cast:
//!
//! - `e_N(d)`: `expect _x: T = d`, then `True`;
//! - `ed_N(d)`: `expect x: T = d`, then `x` as `Data`;
//! - `i_N(d)`, `id_N(d)`: the same with `if d is _x: T`, taking `else` on a mismatch;
//! - `u_N_J()`: the upcast of a literal of type `T`;
//! - `c_N(d)`: `coerce x: T = d`, then `x` as `Data`;
//! - `b_N(d, flag)`: `expect x: T = d`, whose result only one branch of an `if` on `flag` uses;
//! - `g_N_M(d, e)`: `expect` of a `T` and of an `M`, both run through one generic function.
//!
//! The properties, one module each, are:
//!
//! 1. `baseline_agrees_with_the_model`: the baseline returns what the model says, on
//!    well-formed, near-miss and arbitrary `Data`, at both trace levels;
//! 2. `optimized_agrees_with_the_model`: the same for the optimized program, on the forms whose
//!    result the optimizer can't shorten to the input (`e_N`, `i_N`, `u_N_J` and `c_N`, and
//!    `b_N`), not on `ed_N` and `id_N`;
//! 3. `optimizer_agrees_with_baseline`: the optimized program returns what the baseline
//!    returns, on `e_N`, `i_N`, `u_N_J` and `c_N` (`coerce` on well-formed inputs only);
//! 4. `refuses_what_the_model_refuses`: the compiler refuses the casts the model's typing rules
//!    refuse.

use super::{
    Compiled, LEVELS, Outcome, compile,
    model::{Cast, Ctor, D, Model, Tag, Ty, TypeDef, V, power_of_two},
};
use crate::error::Error;
use miette::Diagnostic;
use num_bigint::BigInt;
use proptest::{
    prelude::*,
    test_runner::{Config, RngAlgorithm, RngSeed, TestCaseError, TestRng},
};
use std::{
    fmt,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::LazyLock,
};
use uplc::{ast::Term, data::arbitrary::arb_plutus_data};

/// What a generated type may be made of besides the core: `Int`, `ByteArray`, `Bool`, `Void`,
/// `Data`, lists, options, pairs, tuples and plain data types.
#[derive(Clone, Copy, Debug)]
struct Features {
    strings: bool,
    values: bool,
    points: bool,
    /// Tags on constructors.
    tags: bool,
    /// `@tag` on a type.
    type_tags: bool,
    /// `@list` types.
    lists: bool,
    opaque: bool,
    /// Types with parameters, phantom ones included.
    generics: bool,
    /// Types that mention themselves or each other.
    recursion: bool,
    /// Arguments that a type doesn't store may be anything, such as a function.
    phantom_args: bool,
    /// Anything may appear in a cast's type.
    illegal: bool,
}

impl Features {
    fn everything() -> Features {
        Features {
            strings: true,
            values: true,
            points: true,
            tags: true,
            type_tags: true,
            lists: true,
            opaque: true,
            generics: true,
            recursion: true,
            phantom_args: true,
            illegal: true,
        }
    }
}

const TAGS: [u64; 14] = [0, 1, 2, 3, 5, 6, 7, 8, 12, 130, 1300, 1400, 1401, 5000];

struct Gen {
    rng: TestRng,
    features: Features,
}

impl Gen {
    fn new(seed: u64, features: Features) -> Gen {
        let mut bytes = [0; 32];
        bytes[..8].copy_from_slice(&seed.to_le_bytes());

        Gen {
            rng: TestRng::from_seed(RngAlgorithm::ChaCha, &bytes),
            features,
        }
    }

    fn below(&mut self, n: usize) -> usize {
        (self.rng.next_u64() % n as u64) as usize
    }

    fn chance(&mut self, numerator: usize, denominator: usize) -> bool {
        self.below(denominator) < numerator
    }

    fn pick<T: Clone>(&mut self, items: &[T]) -> T {
        items[self.below(items.len())].clone()
    }

    fn bytes(&mut self, max: usize) -> Vec<u8> {
        let length = self.below(max + 1);

        (0..length).map(|_| self.rng.next_u32() as u8).collect()
    }

    // Types

    fn leaves(&self) -> Vec<Ty> {
        let mut leaves = vec![Ty::Int, Ty::ByteArray, Ty::Bool, Ty::Void, Ty::Data];
        let features = self.features;

        if features.strings {
            leaves.push(Ty::String);
        }

        if features.values {
            leaves.push(Ty::Value);
        }

        if features.points {
            leaves.extend([Ty::G1, Ty::G2]);
        }

        leaves
    }

    fn definitions(&mut self, count: usize) -> Vec<TypeDef> {
        let arities: Vec<usize> = (0..count)
            .map(|_| {
                if self.features.generics {
                    self.pick(&[0, 1, 1, 2])
                } else {
                    0
                }
            })
            .collect();

        (0..count).map(|at| self.definition(at, &arities)).collect()
    }

    fn definition(&mut self, at: usize, arities: &[usize]) -> TypeDef {
        let features = self.features;

        let tag = if features.lists && self.chance(1, 4) {
            Tag::List
        } else if features.type_tags && self.chance(1, 3) {
            Tag::Fixed(self.pick(&TAGS))
        } else {
            Tag::None
        };

        let count = if tag != Tag::None || self.chance(1, 2) {
            1
        } else {
            2 + self.below(2)
        };

        let opaque = features.opaque && self.chance(1, 3);
        let erasable = opaque && tag == Tag::None && count == 1 && self.chance(2, 3);

        let mut ctors: Vec<Ctor> = (0..count)
            .map(|position| {
                let size = if erasable {
                    1
                } else if tag == Tag::List {
                    1 + self.below(2)
                } else {
                    self.below(4)
                };

                Ctor {
                    tag: None,
                    labelled: self.chance(1, 2),
                    fields: (0..size)
                        .map(|_| self.field(at, arities, position == 0, 2))
                        .collect(),
                }
            })
            .collect();

        if features.tags {
            for ctor in &mut ctors {
                ctor.tag = self.chance(1, 3).then(|| self.pick(&TAGS));
            }

            let mut indexes: Vec<u64> = ctors
                .iter()
                .enumerate()
                .map(|(position, ctor)| ctor.tag.unwrap_or(position as u64))
                .collect();

            indexes.sort();
            indexes.dedup();

            if indexes.len() != ctors.len() {
                ctors.iter_mut().for_each(|ctor| ctor.tag = None);
            }
        }

        TypeDef {
            params: arities[at],
            opaque,
            tag,
            shorthand: count == 1
                && ctors[0].tag.is_none()
                && !ctors[0].fields.is_empty()
                && self.chance(1, 2),
            ctors,
        }
    }

    /// The type of a field of the type at `at`. The first constructor only mentions types
    /// defined before `at`, so every type has a value. The others may also mention `at`
    /// itself, with its own parameters (the compiler refuses a definition that mentions
    /// itself at other types), and, if it has none, the later types without parameters.
    fn field(&mut self, at: usize, arities: &[usize], first: bool, depth: usize) -> Ty {
        let params = arities[at];
        let mut referable: Vec<usize> = (0..at).collect();

        if !first && self.features.recursion {
            referable.push(at);

            if params == 0 {
                referable.extend((at + 1..arities.len()).filter(|later| arities[*later] == 0));
            }
        }

        if depth == 0 {
            return self.field_leaf(params);
        }

        match self.below(10) {
            0..=3 => self.field_leaf(params),
            4 => Ty::list(self.field(at, arities, first, depth - 1)),
            5 => Ty::option(self.field(at, arities, first, depth - 1)),
            6 => {
                let key = self.field(at, arities, first, depth - 1);
                let value = self.field(at, arities, first, depth - 1);

                if self.chance(1, 2) {
                    Ty::pairs(key, value)
                } else {
                    Ty::pair(key, value)
                }
            }
            7 => Ty::Tuple(
                (0..2 + self.below(2))
                    .map(|_| self.field(at, arities, first, depth - 1))
                    .collect(),
            ),
            _ if referable.is_empty() => self.field_leaf(params),
            _ => {
                let def = self.pick(&referable);

                let args = if def == at {
                    (0..params).map(Ty::Param).collect()
                } else {
                    (0..arities[def])
                        .map(|_| self.field(at, arities, first, depth - 1))
                        .collect()
                };

                Ty::Named(def, args)
            }
        }
    }

    fn field_leaf(&mut self, params: usize) -> Ty {
        let mut leaves = self.leaves();

        for position in 0..params {
            leaves.extend([Ty::Param(position), Ty::Param(position)]);
        }

        self.pick(&leaves)
    }

    /// A type without parameters, for a cast to convert to.
    fn concrete(&mut self, model: &Model, depth: usize) -> Ty {
        let features = self.features;

        if depth == 0 {
            let mut leaves = self.leaves();

            if features.illegal {
                leaves.extend([Ty::Fn, Ty::MillerLoopResult]);
            }

            return self.pick(&leaves);
        }

        match self.below(10) {
            0..=1 => self.concrete(model, 0),
            2 => Ty::list(self.concrete(model, depth - 1)),
            3 => Ty::option(self.concrete(model, depth - 1)),
            4 => Ty::pairs(
                self.concrete(model, depth - 1),
                self.concrete(model, depth - 1),
            ),
            5 => Ty::pair(
                self.concrete(model, depth - 1),
                self.concrete(model, depth - 1),
            ),
            6 => Ty::Tuple(
                (0..2 + self.below(2))
                    .map(|_| self.concrete(model, depth - 1))
                    .collect(),
            ),
            _ if model.defs().is_empty() => self.concrete(model, 0),
            _ => {
                let def = self.below(model.defs().len());
                let args = (0..model.defs()[def].params)
                    .map(|position| self.argument(model, def, position, depth - 1))
                    .collect();

                Ty::Named(def, args)
            }
        }
    }

    fn argument(&mut self, model: &Model, def: usize, position: usize, depth: usize) -> Ty {
        if self.features.phantom_args
            && !model.is_stored(def, position)
            && self.chance(1, 2)
            && self.chance(1, 2)
        {
            return self.pick(&[Ty::Fn, Ty::MillerLoopResult, Ty::String]);
        }

        self.concrete(model, depth)
    }

    // Values

    fn int(&mut self) -> BigInt {
        match self.below(6) {
            0..=2 => (self.below(5) as i64 - 2).into(),
            3 => (self.rng.next_u64() as i64).into(),
            4 => power_of_two(70),
            _ => -BigInt::from(self.rng.next_u64()) * power_of_two(66),
        }
    }

    fn text(&mut self) -> String {
        let alphabet = ['a', 'b', 'Z', '0', ' ', 'é', '€', '日'];
        let length = self.below(5);

        (0..length).map(|_| self.pick(&alphabet)).collect()
    }

    fn data(&mut self, depth: usize) -> D {
        match if depth == 0 {
            self.below(2)
        } else {
            self.below(5)
        } {
            0 => D::I(self.int()),
            1 => D::B(self.bytes(4)),
            2 => D::List((0..self.below(3)).map(|_| self.data(depth - 1)).collect()),
            3 => D::Map(
                (0..self.below(3))
                    .map(|_| (self.data(depth - 1), self.data(depth - 1)))
                    .collect(),
            ),
            _ => D::Constr(
                self.pick(&TAGS),
                (0..self.below(3)).map(|_| self.data(depth - 1)).collect(),
            ),
        }
    }

    fn value(&mut self, model: &Model, ty: &Ty, fuel: usize) -> V {
        match ty {
            Ty::Int => V::Int(self.int()),
            Ty::ByteArray => V::Bytes(self.bytes(4)),
            Ty::String => V::Str(self.text()),
            Ty::Bool => V::Bool(self.chance(1, 2)),
            Ty::Void => V::Void,
            Ty::Data => V::Data(self.data(2)),
            Ty::Value => {
                let mut policies: Vec<Vec<u8>> =
                    (0..self.below(3)).map(|_| self.bytes(3)).collect();

                policies.sort();
                policies.dedup();

                V::Value(
                    policies
                        .into_iter()
                        .map(|policy| {
                            let mut names: Vec<Vec<u8>> =
                                (0..1 + self.below(2)).map(|_| self.bytes(3)).collect();

                            names.sort();
                            names.dedup();

                            let assets = names
                                .into_iter()
                                .map(|name| (name, BigInt::from(self.below(9) as i64 + 1)))
                                .collect();

                            (policy, assets)
                        })
                        .collect(),
                )
            }
            Ty::G1 => V::G1(self.pick(&G1_POINTS)),
            Ty::G2 => V::G2(self.pick(&G2_POINTS)),
            Ty::Option(item) => {
                if fuel > 0 && self.chance(1, 2) {
                    V::Option(Some(Box::new(self.value(model, item, fuel - 1))))
                } else {
                    V::Option(None)
                }
            }
            Ty::List(item) => {
                let length = if fuel == 0 { 0 } else { self.below(3) };

                V::List(
                    (0..length)
                        .map(|_| self.value(model, item, fuel - 1))
                        .collect(),
                )
            }
            Ty::Pair(a, b) => V::Pair(
                Box::new(self.value(model, a, fuel)),
                Box::new(self.value(model, b, fuel)),
            ),
            Ty::Tuple(items) => V::Tuple(
                items
                    .iter()
                    .map(|item| self.value(model, item, fuel))
                    .collect(),
            ),
            Ty::Named(def, args) => {
                let ctor = if fuel == 0 {
                    0
                } else {
                    self.below(model.defs()[*def].ctors.len())
                };

                V::Ctor(
                    ctor,
                    model
                        .fields(*def, args, ctor)
                        .iter()
                        .map(|field| self.value(model, field, fuel.saturating_sub(1)))
                        .collect(),
                )
            }
            Ty::MillerLoopResult | Ty::Fn | Ty::Param(_) => unreachable!("{ty:?} has no values"),
        }
    }

    /// Data that is almost a `ty`: a well-formed encoding with one thing wrong.
    fn near_miss(&mut self, model: &Model, ty: &Ty, d: &D) -> D {
        let descend = self.chance(2, 3);

        match (ty, d) {
            (Ty::Option(item), D::Constr(0, fields)) if descend && fields.len() == 1 => {
                D::Constr(0, vec![self.near_miss(model, item, &fields[0])])
            }
            (Ty::List(item), D::Map(entries)) if descend && !entries.is_empty() => {
                let Ty::Pair(key, value) = item.as_ref() else {
                    return self.local(ty, d);
                };

                let at = self.below(entries.len());
                let mut entries = entries.clone();

                if self.chance(1, 2) {
                    entries[at].0 = self.near_miss(model, key, &entries[at].0);
                } else {
                    entries[at].1 = self.near_miss(model, value, &entries[at].1);
                }

                D::Map(entries)
            }
            (Ty::List(item), D::List(items)) if descend && !items.is_empty() => {
                D::List(self.near_miss_at(model, items, |_| item))
            }
            (Ty::Pair(a, b), D::List(items)) if descend && items.len() == 2 => {
                D::List(self.near_miss_at(model, items, |at| if at == 0 { a } else { b }))
            }
            (Ty::Tuple(tys), D::List(items)) if descend && !items.is_empty() => {
                D::List(self.near_miss_at(model, items, |at| &tys[at]))
            }
            (Ty::Named(def, args), d) if model.is_erased(*def) => {
                let field = &model.fields(*def, args, 0)[0];

                self.near_miss(model, field, d)
            }
            (Ty::Named(def, args), D::Constr(index, fields)) if descend && !fields.is_empty() => {
                let ctor = (0..model.defs()[*def].ctors.len())
                    .find(|ctor| model.index(*def, *ctor) == *index);

                let Some(ctor) = ctor else {
                    return self.local(ty, d);
                };

                let tys = model.fields(*def, args, ctor);

                D::Constr(*index, self.near_miss_at(model, fields, |at| &tys[at]))
            }
            (Ty::Named(def, args), D::List(fields))
                if descend && !fields.is_empty() && model.defs()[*def].tag == Tag::List =>
            {
                let tys = model.fields(*def, args, 0);

                D::List(self.near_miss_at(model, fields, |at| &tys[at]))
            }
            _ => self.local(ty, d),
        }
    }

    /// `items` with one of them, at a random position `at`, replaced by a near miss of its
    /// type `ty_at(at)`.
    fn near_miss_at<'a>(
        &mut self,
        model: &Model,
        items: &[D],
        ty_at: impl Fn(usize) -> &'a Ty,
    ) -> Vec<D> {
        let at = self.below(items.len());
        let mut items = items.to_vec();

        items[at] = self.near_miss(model, ty_at(at), &items[at]);

        items
    }

    /// A change to `d` itself.
    fn local(&mut self, ty: &Ty, d: &D) -> D {
        let mut candidates = vec![
            D::int(0),
            D::B(vec![]),
            D::List(vec![]),
            D::Map(vec![]),
            D::Constr(0, vec![]),
            D::Constr(1, vec![]),
            D::Constr(7, vec![D::int(1)]),
        ];

        match d {
            D::Constr(index, fields) => {
                let mut longer = fields.clone();
                longer.push(D::int(1));

                let mut shorter = fields.clone();
                shorter.pop();

                candidates.extend([
                    D::Constr(index + 1, fields.clone()),
                    D::Constr(index.saturating_sub(1), fields.clone()),
                    D::Constr(*index, longer),
                    D::Constr(*index, shorter),
                ]);
            }
            D::List(items) => {
                let mut longer = items.clone();
                longer.push(D::int(1));

                let mut shorter = items.clone();
                shorter.pop();

                candidates.extend([D::List(longer), D::List(shorter)]);
            }
            D::Map(entries) => {
                let mut longer = entries.clone();
                longer.push((D::int(1), D::int(2)));

                candidates.push(D::Map(longer));
                candidates.push(D::List(
                    entries.iter().map(|(key, _)| key.clone()).collect(),
                ));
            }
            D::I(_) | D::B(_) => {}
        }

        match ty {
            Ty::String => candidates.extend([
                D::B(vec![0xff]),
                D::B(vec![0xc3, 0x28]),
                D::B(vec![0xed, 0xa0, 0x80]),
                D::B(vec![0xc0, 0x80]),
                D::B(vec![0xe2, 0x82]),
            ]),
            Ty::G1 | Ty::G2 => {
                let size = if *ty == Ty::G1 { 48 } else { 96 };
                let mut random: Vec<u8> = (0..size).map(|_| self.rng.next_u32() as u8).collect();
                random[0] |= 0x80;

                let mut uncompressed = random.clone();
                uncompressed[0] &= 0x7f;

                candidates.extend([
                    D::B(vec![0; size]),
                    D::B(vec![0xc0; size]),
                    D::B(random),
                    D::B(uncompressed),
                    D::B(vec![0xc0; size - 1]),
                    D::B(vec![0xc0; size + 1]),
                ]);

                if let D::B(bytes) = d {
                    let mut flipped = bytes.clone();
                    let at = self.below(flipped.len().max(1));

                    if let Some(byte) = flipped.get_mut(at) {
                        *byte ^= 0x01;
                    }

                    let mut cleared = bytes.clone();
                    cleared[0] &= 0x7f;

                    candidates.extend([D::B(flipped), D::B(cleared)]);
                }
            }
            Ty::Value => {
                let asset = |name: u8, quantity: D| D::Map(vec![(D::B(vec![name]), quantity)]);
                let policy = |name: u8, assets: D| (D::B(vec![name]), assets);

                candidates.extend([
                    D::Map(vec![policy(1, asset(1, D::int(0)))]),
                    D::Map(vec![policy(1, D::Map(vec![]))]),
                    D::Map(vec![
                        policy(2, asset(1, D::int(1))),
                        policy(1, asset(1, D::int(1))),
                    ]),
                    D::Map(vec![
                        policy(1, asset(1, D::int(1))),
                        policy(1, asset(1, D::int(1))),
                    ]),
                    D::Map(vec![policy(
                        1,
                        D::Map(vec![(D::B(vec![2]), D::int(1)), (D::B(vec![1]), D::int(1))]),
                    )]),
                    D::Map(vec![(D::B(vec![1; 33]), asset(1, D::int(1)))]),
                    D::Map(vec![policy(1, asset(1, D::I(power_of_two(127))))]),
                    D::Map(vec![policy(1, asset(1, D::I(-power_of_two(127) - 1)))]),
                    D::Map(vec![policy(1, asset(1, D::B(vec![1])))]),
                    D::Map(vec![policy(1, D::int(1))]),
                    D::Map(vec![(D::int(1), D::int(1))]),
                ]);
            }
            _ => {}
        }

        self.pick(&candidates)
    }
}

/// Compressed points of the group, and its infinity.
static G1_POINTS: LazyLock<Vec<Vec<u8>>> = LazyLock::new(|| {
    let mut points: Vec<Vec<u8>> = (0..4u8)
        .map(|seed| {
            let mut point = blst::blst_p1::default();
            let mut compressed = vec![0; 48];

            unsafe {
                blst::blst_hash_to_g1(
                    &mut point,
                    &seed,
                    1,
                    b"casts".as_ptr(),
                    5,
                    std::ptr::null(),
                    0,
                );
                blst::blst_p1_compress(compressed.as_mut_ptr(), &point);
            }

            compressed
        })
        .collect();

    let mut infinity = vec![0; 48];
    infinity[0] = 0xc0;
    points.push(infinity);
    points
});

static G2_POINTS: LazyLock<Vec<Vec<u8>>> = LazyLock::new(|| {
    let mut points: Vec<Vec<u8>> = (0..4u8)
        .map(|seed| {
            let mut point = blst::blst_p2::default();
            let mut compressed = vec![0; 96];

            unsafe {
                blst::blst_hash_to_g2(
                    &mut point,
                    &seed,
                    1,
                    b"casts".as_ptr(),
                    5,
                    std::ptr::null(),
                    0,
                );
                blst::blst_p2_compress(compressed.as_mut_ptr(), &point);
            }

            compressed
        })
        .collect();

    let mut infinity = vec![0; 96];
    infinity[0] = 0xc0;
    points.push(infinity);
    points
});

/// A type to cast to, with what the programs on it are run on.
struct Target {
    ty: Ty,
    /// Values of the type, for the upcasts.
    literals: Vec<V>,
    /// Data to cast: encodings of values, near-misses, and what other types accept.
    inputs: Vec<D>,
}

struct Case {
    model: Model,
    targets: Vec<Target>,
}

/// A generic function that takes the first element of a list, to run decoded values through.
const FIRST: &str = "pub fn first(xs: List<a>) -> a {\n  expect [x, ..] = xs\n  x\n}\n";

/// `expect _x: ty = d`, then `True`.
fn expect_function(name: &str, ty: &str) -> String {
    format!("pub fn {name}(d: Data) -> Bool {{\n  expect _x: {ty} = d\n  True\n}}\n")
}

/// `if d is _x: ty`, which is `True` unless it takes `else`.
fn if_is_function(name: &str, ty: &str) -> String {
    format!(
        "pub fn {name}(d: Data) -> Bool {{\n  if d is _x: {ty} {{\n    True\n  }} else {{\n    False\n  }}\n}}\n"
    )
}

impl Case {
    fn generate(seed: u64, features: Features, defs: usize, targets: usize, wild: &[D]) -> Case {
        let mut rng = Gen::new(seed, features);
        let model = Model::new(rng.definitions(defs));

        let types: Vec<Ty> = (0..targets).map(|_| rng.concrete(&model, 2)).collect();
        let mut targets: Vec<Target> = types
            .into_iter()
            .map(|ty| {
                if !model.allows(Cast::Upcast, &ty) {
                    return Target {
                        ty,
                        literals: vec![],
                        inputs: vec![],
                    };
                }

                let values: Vec<V> = (0..4).map(|_| rng.value(&model, &ty, 2)).collect();
                let encoded: Vec<D> = values
                    .iter()
                    .map(|value| model.encode(&ty, value).expect("a small value"))
                    .collect();

                let mut inputs = encoded[..2].to_vec();
                inputs.extend(
                    (0..4).map(|n| rng.near_miss(&model, &ty, &encoded[n % encoded.len()])),
                );

                Target {
                    literals: values[2..].to_vec(),
                    inputs,
                    ty,
                }
            })
            .collect();

        let crossed: Vec<D> = targets
            .iter()
            .filter_map(|target| target.inputs.first().cloned())
            .collect();

        for target in &mut targets {
            target.inputs.extend(crossed.iter().cloned());
            target.inputs.extend(wild.iter().cloned());
        }

        Case { model, targets }
    }

    fn source(&self, programs: &[Program]) -> String {
        let mut source = format!("use aiken/builtin\n\n{}", self.model.definitions());

        source.push_str(FIRST);

        for program in programs {
            source.push('\n');
            source.push_str(&self.function(program));
        }

        source
    }

    fn function(&self, program: &Program) -> String {
        let ty = self.targets[program.target].ty.source();
        let name = &program.name;

        match program.form {
            Form::Expect => expect_function(name, &ty),
            Form::ExpectData => format!(
                "pub fn {name}(d: Data) -> Data {{\n  expect x: {ty} = d\n  as_data(x)\n}}\n"
            ),
            Form::IfIs => if_is_function(name, &ty),
            Form::IfIsData => format!(
                "pub fn {name}(d: Data) -> Data {{\n  if d is x: {ty} {{\n    as_data(x)\n  }} else {{\n    builtin.i_data(-1)\n  }}\n}}\n"
            ),
            Form::Coerce => format!(
                "pub fn {name}(d: Data) -> Data {{\n  coerce x: {ty} = d\n  as_data(x)\n}}\n"
            ),
            Form::Upcast(literal) => {
                let target = &self.targets[program.target];

                format!(
                    "pub fn {name}() -> Data {{\n  let v: {ty} = {}\n  let d: Data = v\n  d\n}}\n",
                    self.model.literal(&target.ty, &target.literals[literal])
                )
            }
            Form::Branch => format!(
                "pub fn {name}(d: Data, flag: Data) -> Bool {{\n  expect x: {ty} = d\n  if flag == builtin.i_data(0) {{\n    as_data(x) == d\n  }} else {{\n    True\n  }}\n}}\n"
            ),
            Form::Generic => format!(
                "pub fn {name}(d: Data, e: Data) -> Data {{\n  expect x: {ty} = d\n  expect y: {} = e\n  let a = first([x])\n  let b = first([y])\n  builtin.list_data([as_data(a), as_data(b)])\n}}\n",
                self.targets[program.other].ty.source()
            ),
        }
    }

    /// The programs of `forms` on the targets the model lets each cast apply to.
    fn programs(&self, forms: Forms) -> Vec<Program> {
        let mut programs = vec![];
        let mut expectable = vec![];

        for (n, target) in self.targets.iter().enumerate() {
            let mut add = |name: String, form: Form| {
                if forms.has(form) {
                    programs.push(Program {
                        name,
                        form,
                        target: n,
                        other: n,
                    })
                }
            };

            for literal in 0..target.literals.len() {
                add(format!("u_{n}_{literal}"), Form::Upcast(literal));
            }

            if self.model.allows(Cast::Expect, &target.ty) {
                add(format!("e_{n}"), Form::Expect);
                add(format!("ed_{n}"), Form::ExpectData);
                add(format!("b_{n}"), Form::Branch);
                expectable.push(n);
            }

            if self.model.allows(Cast::IfIs, &target.ty) {
                add(format!("i_{n}"), Form::IfIs);
                add(format!("id_{n}"), Form::IfIsData);
            }

            if self.model.allows(Cast::Coerce, &target.ty) {
                add(format!("c_{n}"), Form::Coerce);
            }
        }

        if forms.has(Form::Generic) {
            for pair in expectable.windows(2) {
                programs.push(Program {
                    name: format!("g_{}_{}", pair[0], pair[1]),
                    form: Form::Generic,
                    target: pair[0],
                    other: pair[1],
                });
            }
        }

        programs
    }

    /// What the program is run on, one list of arguments per run: nothing for an upcast,
    /// the data a `coerce` may take (the well-formed ones) for a `coerce`, any data else.
    fn arguments(&self, program: &Program) -> Vec<Vec<D>> {
        let target = &self.targets[program.target];

        match program.form {
            Form::Upcast(_) => vec![vec![]],
            Form::Coerce => target
                .inputs
                .iter()
                .filter(|d| self.model.decode(&target.ty, d).is_some())
                .map(|d| vec![d.clone()])
                .collect(),
            Form::Branch => target
                .inputs
                .iter()
                .flat_map(|d| [0, 1].map(|flag| vec![d.clone(), D::int(flag)]))
                .collect(),
            Form::Generic => target
                .inputs
                .iter()
                .zip(&self.targets[program.other].inputs)
                .map(|(d, e)| vec![d.clone(), e.clone()])
                .collect(),
            _ => target.inputs.iter().map(|d| vec![d.clone()]).collect(),
        }
    }

    /// What using a bound value as `Data` gives: its encoding, which can fail.
    fn reencoded(&self, ty: &Ty, bound: Option<V>) -> Observed {
        match bound.and_then(|value| self.model.encode(ty, &value)) {
            Some(d) => Observed::Data(d),
            None => Observed::Failed,
        }
    }

    /// What the model says the program returns.
    fn expected(&self, program: &Program, args: &[D]) -> Observed {
        let target = &self.targets[program.target];
        let ty = &target.ty;
        let model = &self.model;

        match program.form {
            Form::Expect => match model.decode(ty, &args[0]) {
                Some(_) => Observed::Bool(true),
                None => Observed::Failed,
            },
            Form::Branch => match self.reencoded(ty, model.decode(ty, &args[0])) {
                Observed::Data(_) => Observed::Bool(true),
                _ => Observed::Failed,
            },
            Form::ExpectData | Form::Coerce => self.reencoded(ty, model.decode(ty, &args[0])),
            Form::IfIs => Observed::Bool(model.decode(ty, &args[0]).is_some()),
            Form::IfIsData => match model.decode(ty, &args[0]) {
                None => Observed::Data(D::int(-1)),
                bound => self.reencoded(ty, bound),
            },
            // Literals are among the values encoded when the case was generated.
            Form::Upcast(literal) => Observed::Data(
                model
                    .encode(ty, &target.literals[literal])
                    .expect("a small value"),
            ),
            Form::Generic => {
                let other = &self.targets[program.other].ty;

                match (
                    self.reencoded(ty, model.decode(ty, &args[0])),
                    self.reencoded(other, model.decode(other, &args[1])),
                ) {
                    (Observed::Data(a), Observed::Data(b)) => Observed::Data(D::List(vec![a, b])),
                    _ => Observed::Failed,
                }
            }
        }
    }
}

impl fmt::Debug for Case {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "\n{}", self.source(&self.programs(Forms::ALL)))?;

        for (n, target) in self.targets.iter().enumerate() {
            writeln!(f, "// target {n}: {}", target.ty.source())?;

            for input in &target.inputs {
                writeln!(f, "//   input {input}")?;
            }
        }

        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Form {
    Expect,
    ExpectData,
    IfIs,
    IfIsData,
    Coerce,
    /// The upcast of the target's literal at this position.
    Upcast(usize),
    /// An `expect` whose result only one branch uses.
    Branch,
    /// Two decoded values through a generic function.
    Generic,
}

struct Program {
    name: String,
    form: Form,
    target: usize,
    /// The second target of a generic program.
    other: usize,
}

#[derive(Debug, PartialEq)]
enum Observed {
    Failed,
    Bool(bool),
    Data(D),
    Other(String),
}

impl Observed {
    fn of(outcome: &Outcome) -> Observed {
        match outcome {
            Outcome::Failed => Observed::Failed,
            outcome if outcome.is_true() => Observed::Bool(true),
            Outcome::Returned(term) if *term == Term::bool(false) => Observed::Bool(false),
            outcome => match outcome.data() {
                Some(data) => Observed::Data(D::from_plutus(&data)),
                None => Observed::Other(format!("{outcome:?}")),
            },
        }
    }
}

/// What the outcomes of a program are compared to.
#[derive(Clone, Copy, PartialEq)]
enum Property {
    /// The baseline against the model.
    BaselineModel,
    /// The optimized program against the model.
    OptimizedModel,
    /// The optimized program against the baseline.
    Differential,
}

/// Which of the programs a property runs.
#[derive(Clone, Copy)]
struct Forms(u8);

impl Forms {
    /// The checks and the upcasts, which return what the model says whatever the optimizer
    /// does: `e_N`, `i_N`, `u_N_J` and `c_N`.
    const PLAIN: Forms = Forms(1);
    /// The programs that return what they decoded, which the optimizer may shorten to the
    /// input: `ed_N` and `id_N`.
    const VALUES: Forms = Forms(2);
    /// The programs that use what they decoded in one branch only: `b_N`.
    const BRANCHES: Forms = Forms(4);
    /// Two decoded values through a generic function: `g_N_M`.
    const GENERIC: Forms = Forms(8);
    const ALL: Forms = Forms(15);
    const NOT_GENERIC: Forms = Forms(7);

    fn has(self, form: Form) -> bool {
        let category = match form {
            Form::Expect | Form::IfIs | Form::Coerce | Form::Upcast(_) => Forms::PLAIN,
            Form::ExpectData | Form::IfIsData => Forms::VALUES,
            Form::Branch => Forms::BRANCHES,
            Form::Generic => Forms::GENERIC,
        };

        self.0 & category.0 != 0
    }
}

fn describe(errors: &[Error]) -> String {
    let message = errors
        .iter()
        .map(|error| match error {
            Error::Type { error, .. } => format!("{error:?}"),
            other => format!("{other:?}"),
        })
        .collect::<Vec<_>>()
        .join("; ");

    message.chars().take(400).collect()
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| payload.downcast_ref::<&str>().copied())
        .unwrap_or("a panic")
        .lines()
        .next()
        .unwrap_or("")
        .to_string()
}

fn run_program(
    compiled: &Compiled,
    case: &Case,
    program: &Program,
    property: Property,
    failures: &mut Vec<String>,
) {
    let programs = compiled.function("m", &program.name);
    let level = compiled.level();

    for args in case.arguments(program) {
        let plutus: Vec<_> = args.iter().map(D::to_plutus).collect();
        let (baseline, optimized) = programs.run_both(&plutus);

        let shown = args
            .iter()
            .map(D::to_string)
            .collect::<Vec<_>>()
            .join(" | ");

        match property {
            Property::BaselineModel | Property::OptimizedModel => {
                let expected = case.expected(program, &args);
                let (which, outcome) = match property {
                    Property::BaselineModel => ("baseline", &baseline),
                    _ => ("optimized", &optimized),
                };
                let observed = Observed::of(outcome);

                if observed != expected {
                    failures.push(format!(
                        "{level:?} {} on {shown}: the {which} returns {observed:?}, the model says {expected:?}",
                        program.name
                    ));
                }
            }
            Property::Differential => {
                if baseline != optimized {
                    failures.push(format!(
                        "{level:?} {} on {shown}: the baseline returns {:?}, the optimized program {:?}",
                        program.name,
                        Observed::of(&baseline),
                        Observed::of(&optimized)
                    ));
                }
            }
        }
    }
}

fn check(case: &Case, property: Property, forms: Forms) -> Result<(), TestCaseError> {
    let programs = case.programs(forms);
    let source = case.source(&programs);
    let mut failures = vec![];

    for level in LEVELS {
        let compiled = match compile(&[("lib/m.ak", &source)], level) {
            Ok(compiled) => compiled,
            Err(errors) => {
                failures.push(format!(
                    "{level:?}: the compiler refuses what the model allows: {}",
                    describe(&errors)
                ));
                continue;
            }
        };

        for program in &programs {
            let outcome = catch_unwind(AssertUnwindSafe(|| {
                run_program(&compiled, case, program, property, &mut failures)
            }));

            if let Err(payload) = outcome {
                failures.push(format!(
                    "{level:?} {}: panic: {}",
                    program.name,
                    panic_message(payload.as_ref())
                ));
            }
        }
    }

    if failures.is_empty() {
        return Ok(());
    }

    let total = failures.len();
    failures.dedup();
    failures.truncate(6);

    Err(TestCaseError::fail(format!(
        "{total} failures:\n  {}\n\n{source}",
        failures.join("\n  ")
    )))
}

/// Whether every error is a type error at or after `start`.
fn refused_from(errors: &[Error], start: usize) -> bool {
    errors.iter().all(|error| match error {
        Error::Type { error, .. } => error
            .labels()
            .is_some_and(|mut labels| labels.next().is_some_and(|label| label.offset() >= start)),
        _ => false,
    })
}

/// The compiler refuses `cast` on every target the model refuses it on, with a type error
/// at the cast. Type checking doesn't depend on the trace level.
fn check_refusals(case: &Case, cast: Cast) -> Result<(), TestCaseError> {
    let refused: Vec<&Target> = case
        .targets
        .iter()
        .filter(|target| !case.model.allows(cast, &target.ty))
        .take(2)
        .collect();

    prop_assume!(!refused.is_empty());

    let base = case.source(&[]);

    for target in refused {
        let ty = target.ty.source();

        let function = match cast {
            Cast::Upcast => format!("pub fn bad(v: {ty}) -> Data {{\n  let d: Data = v\n  d\n}}\n"),
            Cast::Expect => expect_function("bad", &ty),
            Cast::IfIs => if_is_function("bad", &ty),
            Cast::Coerce => {
                format!("pub fn bad(d: Data) -> Bool {{\n  coerce x: {ty} = d\n  True\n}}\n")
            }
        };

        let source = format!("{base}\n{function}");

        match compile(&[("lib/m.ak", &source)], LEVELS[0]) {
            Ok(_) => {
                return Err(TestCaseError::fail(format!(
                    "the compiler accepts {cast:?} on {ty}\n\n{source}"
                )));
            }
            Err(errors) if !refused_from(&errors, base.len()) => {
                return Err(TestCaseError::fail(format!(
                    "{cast:?} on {ty} is refused, but not by a type error at the cast: {}\n\n{source}",
                    describe(&errors)
                )));
            }
            Err(_) => {}
        }
    }

    Ok(())
}

fn cases(features: Features) -> impl Strategy<Value = Case> {
    (
        any::<u64>(),
        1..=3usize,
        2..=5usize,
        proptest::collection::vec(arb_plutus_data(), 0..=3),
    )
        .prop_map(move |(seed, defs, targets, wild)| {
            let wild: Vec<D> = wild.iter().map(D::from_plutus).collect();

            Case::generate(seed, features, defs, targets, &wild)
        })
}

fn config(cases: u32) -> Config {
    let mut config = Config::with_cases(cases);

    config.failure_persistence = None;
    config.max_shrink_iters = 8;

    if matches!(config.rng_seed, RngSeed::Random) {
        config.rng_seed = RngSeed::Fixed(0x_ca57_5eed);
    }

    config
}

/// A property test: on every case generated from `$features`, `check` holds for `$property` on
/// the programs of `$forms`, or `check_refusals` holds for `$cast`.
macro_rules! property {
    (@test $name:ident, $features:expr, $cases:expr, $check:expr) => {
        proptest! {
            #![proptest_config(config($cases))]
            #[test]
            fn $name(case in cases($features)) {
                $check(&case)?;
            }
        }
    };
    ($name:ident, $features:expr, $cases:expr, refuses $cast:ident) => {
        property!(@test $name, $features, $cases, |case: &Case| {
            check_refusals(case, Cast::$cast)
        });
    };
    ($name:ident, $features:expr, $cases:expr, $property:ident, $forms:expr) => {
        property!(@test $name, $features, $cases, |case: &Case| {
            check(case, Property::$property, $forms)
        });
    };
}

const NONE: Features = Features {
    strings: false,
    values: false,
    points: false,
    tags: false,
    type_tags: false,
    lists: false,
    opaque: false,
    generics: false,
    recursion: false,
    phantom_args: false,
    illegal: false,
};

/// Strings, tags on constructors, parameters (phantom ones included) and recursion.
const SUPPORTED: Features = Features {
    strings: true,
    tags: true,
    generics: true,
    recursion: true,
    ..NONE
};

const WITH_LISTS: Features = Features {
    lists: true,
    ..SUPPORTED
};

const WITH_OPAQUE: Features = Features {
    opaque: true,
    generics: true,
    strings: true,
    ..NONE
};

mod baseline_agrees_with_the_model {
    use super::*;

    property!(supported, SUPPORTED, 12, BaselineModel, Forms::ALL);
    property!(list_types, WITH_LISTS, 8, BaselineModel, Forms::NOT_GENERIC);
    property!(opaque, WITH_OPAQUE, 8, BaselineModel, Forms::ALL);
}

mod optimized_agrees_with_the_model {
    use super::*;

    property!(supported, SUPPORTED, 10, OptimizedModel, Forms::PLAIN);
}

mod optimizer_agrees_with_baseline {
    use super::*;

    property!(opaque, WITH_OPAQUE, 6, Differential, Forms::PLAIN);
}

mod refuses_what_the_model_refuses {
    use super::*;

    property!(upcast, Features::everything(), 8, refuses Upcast);
    property!(expect, Features::everything(), 8, refuses Expect);
    property!(
        if_is,
        Features {
            illegal: true,
            opaque: true,
            ..NONE
        },
        8,
        refuses IfIs
    );
    property!(coerce, Features::everything(), 8, refuses Coerce);
}
