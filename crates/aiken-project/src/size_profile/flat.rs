//! Measure the serialized bit range of each UPLC term.
use super::{Result, ensure};
use pallas_codec::flat::de::{Decode, Decoder};
use serde::Serialize;
use uplc::{
    ast::{Constant, DeBruijn, Name, NamedDeBruijn, Program, Term},
    builtins::DefaultFunction,
};

#[derive(Debug, Clone, Serialize)]
pub struct Node {
    pub id: usize,
    pub parent: Option<usize>,
    pub kind: &'static str,
    pub name: Option<String>,
    pub start_bit: usize,
    pub end_bit: usize,
    pub inclusive_bits: usize,
    pub exclusive_bits: usize,
}

pub struct Measurement {
    pub flat: Vec<u8>,
    pub cbor: Vec<u8>,
    pub version_bits: usize,
    pub padding_bits: usize,
    pub nodes: Vec<Node>,
}

enum Task<'a> {
    Enter(&'a Term<NamedDeBruijn>, Option<usize>),
    Exit(usize),
    List(&'a [Term<NamedDeBruijn>], usize),
}

fn bit_offset(decoder: &Decoder<'_>) -> usize {
    decoder.pos * 8 + decoder.used_bits as usize
}

pub fn measure_program(program: Program<Name>) -> Result<Measurement> {
    let named = program.to_named_debruijn()?;
    let debruijn: Program<DeBruijn> = named.clone().into();
    let flat = debruijn.to_flat()?;
    let mut cbor = Vec::new();
    pallas_codec::minicbor::Encoder::new(&mut cbor).bytes(&flat)?;
    let mut decoder = Decoder::new(&flat);
    let version = (
        usize::decode(&mut decoder)?,
        usize::decode(&mut decoder)?,
        usize::decode(&mut decoder)?,
    );
    ensure(version == named.version, "Flat version mismatch")?;
    let version_bits = bit_offset(&decoder);
    let mut nodes: Vec<Node> = Vec::new();
    let mut tasks = vec![Task::Enter(&named.term, None)];
    while let Some(task) = tasks.pop() {
        match task {
            Task::Exit(id) => {
                let end = bit_offset(&decoder);
                let node = &mut nodes[id];
                node.end_bit = end;
                node.inclusive_bits = end - node.start_bit;
                node.exclusive_bits = node.inclusive_bits - node.exclusive_bits;
                if let Some(parent) = node.parent {
                    nodes[parent].exclusive_bits += node.inclusive_bits;
                }
            }
            Task::List(terms, parent) => {
                ensure(decoder.bool()? == !terms.is_empty(), "Flat list mismatch")?;
                if let Some((first, rest)) = terms.split_first() {
                    tasks.push(Task::List(rest, parent));
                    tasks.push(Task::Enter(first, Some(parent)));
                }
            }
            Task::Enter(term, parent) => {
                let id = nodes.len();
                let start_bit = bit_offset(&decoder);
                let (tag, kind, name) = match term {
                    Term::Var(n) => (0, "Var", Some(n.text.clone())),
                    Term::Delay(_) => (1, "Delay", None),
                    Term::Lambda { parameter_name, .. } => {
                        (2, "Lambda", Some(parameter_name.text.clone()))
                    }
                    Term::Apply { .. } => (3, "Apply", None),
                    Term::Constant(_) => (4, "Constant", None),
                    Term::Force(_) => (5, "Force", None),
                    Term::Error => (6, "Error", None),
                    Term::Builtin(_) => (7, "Builtin", None),
                    Term::Constr { .. } => (8, "Constr", None),
                    Term::Case { .. } => (9, "Case", None),
                };
                ensure(decoder.bits8(4)? == tag, "Flat term tag mismatch")?;
                nodes.push(Node {
                    id,
                    parent,
                    kind,
                    name,
                    start_bit,
                    end_bit: 0,
                    inclusive_bits: 0,
                    exclusive_bits: 0,
                });
                tasks.push(Task::Exit(id));
                match term {
                    Term::Var(n) => ensure(
                        DeBruijn::decode(&mut decoder)? == n.index,
                        "Flat variable mismatch",
                    )?,
                    Term::Delay(body) | Term::Force(body) | Term::Lambda { body, .. } => {
                        tasks.push(Task::Enter(body, Some(id)));
                    }
                    Term::Apply { function, argument } => {
                        tasks.push(Task::Enter(argument, Some(id)));
                        tasks.push(Task::Enter(function, Some(id)));
                    }
                    Term::Constant(c) => ensure(
                        Constant::decode(&mut decoder)? == **c,
                        "Flat constant mismatch",
                    )?,
                    Term::Error => {}
                    Term::Builtin(b) => ensure(
                        DefaultFunction::decode(&mut decoder)? == *b,
                        "Flat builtin mismatch",
                    )?,
                    Term::Constr { tag, fields } => {
                        ensure(
                            usize::decode(&mut decoder)? == *tag,
                            "Flat constructor mismatch",
                        )?;
                        tasks.push(Task::List(fields, id));
                    }
                    Term::Case { constr, branches } => {
                        tasks.push(Task::List(branches, id));
                        tasks.push(Task::Enter(constr, Some(id)));
                    }
                }
            }
        }
    }
    let padding_bits = flat.len() * 8 - bit_offset(&decoder);
    ensure(
        (1..=8).contains(&padding_bits),
        "Invalid Flat padding length",
    )?;
    decoder.filler()?;
    ensure(
        bit_offset(&decoder) == flat.len() * 8,
        "Trailing Flat bytes",
    )?;
    ensure(
        nodes.iter().map(|n| n.exclusive_bits).sum::<usize>() + version_bits + padding_bits
            == flat.len() * 8,
        "Flat bit partition mismatch",
    )?;
    Ok(Measurement {
        flat,
        cbor,
        version_bits,
        padding_bits,
        nodes,
    })
}
