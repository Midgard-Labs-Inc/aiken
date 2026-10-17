//! Private ownership boundary for parallel execution of Rc-backed programs.
//!
//! Public test values are not Send: cloning them can retain aliases on the
//! calling thread. Only detached programs, with display-only AST/type metadata
//! removed, may cross this boundary. These wrappers cannot be cloned or expose
//! their contents until execution has finished.
use aiken_lang::{
    ast::Tracing,
    expr::TypedExpr,
    plutus_version::PlutusVersion,
    test_framework::{Assertion, Test, TestResult},
    tipo::Type,
};
use std::rc::Rc;
use uplc::{
    PlutusData,
    ast::{Constant, Name, Program},
};

type RawResult = TestResult<(Constant, Rc<Type>), PlutusData>;
type TypedAssertion = Option<Box<Assertion<TypedExpr>>>;

pub(crate) struct DetachedTest(Test);
pub(crate) struct DetachedResult(RawResult);

pub(crate) enum LocalMetadata {
    Unit(TypedAssertion),
    Property(Rc<Type>, Rc<Type>),
    Benchmark(Rc<Type>, Rc<Type>),
}

// SAFETY: the only constructor is `detach`. Every program is rebuilt with
// fresh Rc allocations, and all AST/type metadata which can alias the caller
// is retained in LocalMetadata. The remaining fields are owned strings,
// paths, scalar enums and these private program graphs. No inner reference or
// clone is exposed before the wrapper is consumed on a worker.
unsafe impl Send for DetachedTest {}

// SAFETY: this wrapper is constructed only by consuming a DetachedTest on a
// worker. Evaluation and optimization create only worker-local Rc graphs; no
// other thread has access to them. The wrapper is not Clone or Sync and is
// unwrapped only after the worker has relinquished ownership.
unsafe impl Send for DetachedResult {}

fn detach_program(program: &mut Program<Name>) {
    program.term = program.term.deep_clone();
}

pub(crate) fn detach(mut test: Test) -> (DetachedTest, LocalMetadata) {
    let metadata = match &mut test {
        Test::UnitTest(unit) => {
            detach_program(&mut unit.program);
            LocalMetadata::Unit(unit.assertion.take())
        }
        Test::PropertyTest(property) => {
            detach_program(&mut property.program);
            detach_program(&mut property.fuzzer.program);
            // Types are used for reifying results, not for executing programs.
            // Keep their shared inference graph on the calling thread.
            LocalMetadata::Property(
                std::mem::replace(&mut property.fuzzer.type_info, Type::data()),
                std::mem::replace(&mut property.fuzzer.stripped_type_info, Type::data()),
            )
        }
        Test::Benchmark(benchmark) => {
            detach_program(&mut benchmark.program);
            detach_program(&mut benchmark.sampler.program);
            LocalMetadata::Benchmark(
                std::mem::replace(&mut benchmark.sampler.type_info, Type::data()),
                std::mem::replace(&mut benchmark.sampler.stripped_type_info, Type::data()),
            )
        }
    };
    (DetachedTest(test), metadata)
}

impl DetachedTest {
    pub(crate) fn run(
        self,
        seed: u32,
        max_success: usize,
        plutus_version: &PlutusVersion,
        tracing: Tracing,
    ) -> DetachedResult {
        DetachedResult(self.0.run(seed, max_success, plutus_version, tracing))
    }
}

impl DetachedResult {
    pub(crate) fn restore(self, metadata: LocalMetadata) -> (RawResult, TypedAssertion) {
        let mut result = self.0;
        let assertion = match (&mut result, metadata) {
            (TestResult::UnitTestResult(_), LocalMetadata::Unit(assertion)) => assertion,
            (TestResult::PropertyTestResult(property), LocalMetadata::Property(tipo, stripped)) => {
                property.test.fuzzer.type_info = tipo;
                property.test.fuzzer.stripped_type_info = stripped;
                None
            }
            (TestResult::BenchmarkResult(benchmark), LocalMetadata::Benchmark(tipo, stripped)) => {
                benchmark.bench.sampler.type_info = tipo;
                benchmark.bench.sampler.stripped_type_info = stripped;
                None
            }
            _ => unreachable!("indexed parallel execution preserves runnable order"),
        };
        (result, assertion)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aiken_lang::{
        ast::OnTestFailure,
        test_framework::{Benchmark, Fuzzer, PropertyTest, Sampler, UnitTest},
    };
    use std::path::PathBuf;
    use uplc::ast::Term;

    fn program() -> Program<Name> {
        Program {
            version: (1, 1, 0),
            term: Term::bool(true),
        }
    }

    #[test]
    fn detached_unit_does_not_share_program_allocations_with_its_clone() {
        let original = Test::UnitTest(UnitTest {
            input_path: PathBuf::new(),
            module: "test".into(),
            name: "unit".into(),
            on_test_failure: OnTestFailure::FailImmediately,
            program: program(),
            assertion: None,
        });
        let retained = original.clone();
        let (detached, metadata) = detach(original);
        let Test::UnitTest(unit) = &detached.0 else {
            unreachable!()
        };
        let Test::UnitTest(retained) = &retained else {
            unreachable!()
        };
        let (Term::Constant(left), Term::Constant(right)) =
            (&unit.program.term, &retained.program.term)
        else {
            unreachable!()
        };
        assert!(!Rc::ptr_eq(left, right));
        let result =
            std::thread::spawn(move || detached.run(42, 1, &PlutusVersion::V3, Tracing::silent()))
                .join()
                .unwrap();
        let (result, _) = result.restore(metadata);
        assert!(result.is_success());
        assert_eq!(retained.program, program());
    }

    #[test]
    fn display_types_stay_local_for_properties_and_benchmarks() {
        let tipo = Type::int();
        let stripped = Type::int();
        let property = Test::PropertyTest(PropertyTest {
            input_path: PathBuf::new(),
            module: "test".into(),
            name: "property".into(),
            on_test_failure: OnTestFailure::FailImmediately,
            program: program(),
            fuzzer: Fuzzer {
                program: program(),
                type_info: tipo.clone(),
                stripped_type_info: stripped.clone(),
            },
        });
        let (detached, local) = detach(property);
        let Test::PropertyTest(property) = &detached.0 else {
            unreachable!()
        };
        assert!(!Rc::ptr_eq(&tipo, &property.fuzzer.type_info));
        let LocalMetadata::Property(kept, kept_stripped) = local else {
            unreachable!()
        };
        assert!(Rc::ptr_eq(&tipo, &kept));
        assert!(Rc::ptr_eq(&stripped, &kept_stripped));

        let benchmark = Test::Benchmark(Benchmark {
            input_path: PathBuf::new(),
            module: "test".into(),
            name: "benchmark".into(),
            on_test_failure: OnTestFailure::FailImmediately,
            program: program(),
            sampler: Sampler {
                program: program(),
                type_info: tipo.clone(),
                stripped_type_info: stripped.clone(),
            },
        });
        let (detached, local) = detach(benchmark);
        let Test::Benchmark(benchmark) = &detached.0 else {
            unreachable!()
        };
        assert!(!Rc::ptr_eq(&tipo, &benchmark.sampler.type_info));
        let LocalMetadata::Benchmark(kept, kept_stripped) = local else {
            unreachable!()
        };
        assert!(Rc::ptr_eq(&tipo, &kept));
        assert!(Rc::ptr_eq(&stripped, &kept_stripped));
    }
    #[test]
    fn failed_unit_assertions_are_evaluated_after_returning_to_the_caller() {
        crate::test_framework::test::with_test_from_source(
            "test mismatch() { 1 == 2 }",
            |test, generator, data_types| {
                let expected = test
                    .clone()
                    .run(42, 1, &PlutusVersion::V3, Tracing::verbose());
                let (worker, metadata) = detach(test);
                let outcome = std::thread::spawn(move || {
                    worker.run(42, 1, &PlutusVersion::V3, Tracing::verbose())
                })
                .join()
                .unwrap();
                let (mut actual, assertion) = outcome.restore(metadata);
                let TestResult::UnitTestResult(expected) = expected else {
                    unreachable!()
                };
                let TestResult::UnitTestResult(unit) = &mut actual else {
                    unreachable!()
                };
                assert!(!unit.success);
                assert_eq!(unit.spent_budget, expected.spent_budget);
                assert_eq!(unit.logs, expected.logs);
                unit.assertion = Some(assertion.unwrap().evaluate(generator, &unit.test.module));
                let actual = actual.reify(&crate::utils::indexmap::as_ref_values(data_types));
                let TestResult::UnitTestResult(unit) = actual else {
                    unreachable!()
                };
                assert!(unit.assertion.is_some());
                assert!(!unit.success);
            },
        );
    }
    #[test]
    fn parallel_property_preserves_counterexamples_and_restores_original_display_types() {
        let (property, reify) = crate::test_framework::test::property_raw(
            "test nonnegative(n: Int via int()) { n < 0 }",
        );
        let tipo = property.fuzzer.type_info.clone();
        let expected = property.clone().run(42, 10, &PlutusVersion::V3);
        let (worker, metadata) = detach(Test::PropertyTest(property));
        let outcome =
            std::thread::spawn(move || worker.run(42, 10, &PlutusVersion::V3, Tracing::silent()))
                .join()
                .unwrap();
        let (actual, _) = outcome.restore(metadata);
        let TestResult::PropertyTestResult(actual) = actual else {
            unreachable!()
        };
        assert!(Rc::ptr_eq(&actual.test.fuzzer.type_info, &tipo));
        assert_eq!(actual.iterations, expected.iterations);
        assert_eq!(actual.labels, expected.labels);
        assert_eq!(actual.logs, expected.logs);
        assert_eq!(actual.counterexample, expected.counterexample);
        let counterexample = actual.counterexample.unwrap().unwrap();
        assert_eq!(reify(counterexample), "0");
    }
}
