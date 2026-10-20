//! Tests of casts through `Data`: programs whose results must not change, a reference model of
//! the semantics (`site/src/content/docs/casts.mdx`), and the properties that compare the
//! compiler against the model, and the optimized programs against their baseline.
//!
//! Every program is compiled twice, at silent and verbose, and run twice: optimized, and as
//! the baseline (`Program::lower`), which only does the lowering every program needs.

use super::NoopEventListener;
use crate::{Project, ast::Definition, error::Error, options::Options};
use aiken_lang::{
    ast::{FunctionAccessKey, TraceLevel, Tracing},
    gen_uplc::Error as CodeGenError,
    tipo::error::Error as TypeError,
};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
use uplc::{
    PlutusData,
    ast::{Name, NamedDeBruijn, Program, Term},
    machine::cost_model::ExBudget,
    optimize::aiken_optimize_and_intern,
};

mod guards;
mod model;
mod properties;

pub const LEVELS: [TraceLevel; 2] = [TraceLevel::Silent, TraceLevel::Verbose];

/// A path in the temporary directory that no other test of this run uses.
fn temp_path() -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);

    std::env::temp_dir().join(format!(
        "aiken-casts-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

/// A project written to a temporary directory and type-checked at one trace level.
pub struct Compiled {
    project: Project<NoopEventListener>,
    level: TraceLevel,
    root: PathBuf,
}

impl Drop for Compiled {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// Writes `files` (paths such as `lib/m.ak` or `validators/v.ak`) into a fresh project, and
/// type-checks it at `level`.
pub fn compile(files: &[(&str, &str)], level: TraceLevel) -> Result<Compiled, Vec<Error>> {
    let root = temp_path();

    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("aiken.toml"),
        "name = \"test/casts\"\nversion = \"0.0.0\"\ncompiler = \"v1.3.0\"\nplutus = \"v3\"\n",
    )
    .unwrap();

    for (path, source) in files {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
    }

    let project = Project::new(root.clone(), NoopEventListener).map_err(|error| vec![error]);

    let mut compiled = Compiled {
        project: project?,
        level,
        root,
    };

    compiled.project.compile(Options {
        tracing: Tracing::All(level),
        ..Options::default()
    })?;

    Ok(compiled)
}

/// Compiles `files` at both trace levels, panicking on any error.
pub fn compile_ok(files: &[(&str, &str)]) -> Vec<Compiled> {
    LEVELS
        .iter()
        .map(|level| {
            compile(files, *level)
                .unwrap_or_else(|errors| panic!("compilation failed at {level:?}: {errors:#?}"))
        })
        .collect()
}

/// The type errors `files` gets at each trace level.
pub fn type_errors(files: &[(&str, &str)]) -> Vec<TypeError> {
    LEVELS
        .iter()
        .flat_map(|level| match compile(files, *level) {
            Ok(_) => vec![],
            Err(errors) => errors
                .into_iter()
                .map(|error| match error {
                    Error::Type { error, .. } => *error,
                    other => panic!("expected a type error, got {other:#?}"),
                })
                .collect(),
        })
        .collect()
}

/// Compiles `files` at both trace levels and checks that every unit test of `module` passes,
/// optimized and as the baseline.
pub fn assert_tests_pass(files: &[(&str, &str)], module: &str) -> Vec<Compiled> {
    assert_modules_pass(files, &[module])
}

/// `assert_tests_pass` on several modules of one project, which is compiled once.
pub fn assert_modules_pass(files: &[(&str, &str)], modules: &[&str]) -> Vec<Compiled> {
    let compiled = compile_ok(files);

    for project in &compiled {
        for module in modules {
            let tests = project.tests(module);
            assert!(!tests.is_empty(), "no tests in {module}");

            for (name, programs) in tests {
                assert!(
                    programs.run(&[]).is_true(),
                    "test {module}.{name} failed at {:?}",
                    project.level()
                );
            }
        }
    }

    compiled
}

impl Compiled {
    pub fn level(&self) -> TraceLevel {
        self.level
    }

    /// The function `module.name`, compiled with its arguments taken from `Data`.
    pub fn function(&self, module: &str, name: &str) -> Programs {
        let key = FunctionAccessKey {
            module_name: module.to_string(),
            function_name: name.to_string(),
        };

        let function = self
            .project
            .functions
            .get(&key)
            .unwrap_or_else(|| panic!("no function {module}.{name}"));

        let program = self
            .project
            .new_generator(Tracing::All(self.level))
            .generate_raw_unoptimized(&function.body, &function.arguments, module)
            .unwrap();

        Programs::new(program)
    }

    /// The unit tests of `module` that take no arguments, as the programs they lower to before
    /// any optimization, by name.
    pub fn test_programs(
        &self,
        module: &str,
    ) -> Vec<(String, Result<Program<Name>, CodeGenError>)> {
        self.definitions(module)
            .filter_map(|definition| match definition {
                Definition::Test(test) if test.arguments.is_empty() => {
                    let program = self
                        .project
                        .new_generator(Tracing::All(self.level))
                        .generate_raw_unoptimized(&test.body, &[], module);

                    Some((test.name.clone(), program))
                }
                _ => None,
            })
            .collect()
    }

    /// Every unit test of `module` that takes no arguments, by name.
    pub fn tests(&self, module: &str) -> Vec<(String, Programs)> {
        self.test_programs(module)
            .into_iter()
            .map(|(name, program)| (name, Programs::new(program.unwrap())))
            .collect()
    }

    /// The validator `module.name`, as the script that takes the script context.
    pub fn validator(&self, module: &str, name: &str) -> Programs {
        let validator = self
            .definitions(module)
            .find_map(|definition| match definition {
                Definition::Validator(validator) if validator.name == name => Some(validator),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no validator {module}.{name}"));

        let program = self
            .project
            .new_generator(Tracing::All(self.level))
            .generate_unoptimized(validator, module)
            .unwrap();

        Programs::new(program)
    }

    fn definitions(&self, module: &str) -> impl Iterator<Item = &aiken_lang::ast::TypedDefinition> {
        self.project
            .checked_modules
            .get(module)
            .unwrap_or_else(|| panic!("no module {module}"))
            .ast
            .definitions()
    }
}

/// What running a program gave.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Returned(Term<NamedDeBruijn>),
    Failed,
}

impl Outcome {
    pub fn is_true(&self) -> bool {
        self == &Outcome::Returned(Term::bool(true))
    }

    pub fn data(&self) -> Option<PlutusData> {
        match self {
            Outcome::Returned(Term::Constant(constant)) => match constant.as_ref() {
                uplc::ast::Constant::Data(data) => Some(data.into()),
                _ => None,
            },
            _ => None,
        }
    }
}

/// A program, optimized and as its baseline.
pub struct Programs {
    pub baseline: Program<Name>,
    pub optimized: Program<Name>,
}

impl Programs {
    fn new(unoptimized: Program<Name>) -> Self {
        Programs {
            baseline: unoptimized.clone().lower(),
            optimized: aiken_optimize_and_intern(unoptimized),
        }
    }

    /// Runs both programs on `args`, checks that they agree, and returns what they gave.
    pub fn run(&self, args: &[PlutusData]) -> Outcome {
        let baseline = run(&self.baseline, args);
        let optimized = run(&self.optimized, args);

        assert_eq!(
            optimized,
            baseline,
            "the optimized program disagrees with the baseline on {args:?}\n\nbaseline:\n{}\n\noptimized:\n{}",
            self.baseline.to_pretty(),
            self.optimized.to_pretty(),
        );

        optimized
    }

    /// Runs both programs on `args`, without asserting that they agree.
    pub fn run_both(&self, args: &[PlutusData]) -> (Outcome, Outcome) {
        (run(&self.baseline, args), run(&self.optimized, args))
    }
}

fn run(program: &Program<Name>, args: &[PlutusData]) -> Outcome {
    let program = args.iter().fold(program.clone(), |program, arg| {
        program.apply_data(arg.clone())
    });

    let program: Program<NamedDeBruijn> = program.try_into().unwrap();

    match program.eval(ExBudget::max()).result() {
        Ok(term) => Outcome::Returned(term),
        Err(_) => Outcome::Failed,
    }
}
