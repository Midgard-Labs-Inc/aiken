use super::*;
use pretty_assertions::assert_eq;

/// Compile the `observe` function of `source`, after checking its dependencies.
fn compile(dependencies: &[(&str, &str)], source: &str, tracing: Tracing) -> Program<DeBruijn> {
    let mut project = TestProject::new();

    for (name, code) in dependencies {
        let mut dependency = project.parse(code);
        dependency.name = name.to_string();
        dependency.ast.name = name.to_string();
        project.check(dependency);
    }

    let module = project.check(project.parse(source));

    let function = module
        .ast
        .definitions()
        .find_map(|definition| match definition {
            Definition::Fn(function) if function.name == "observe" => Some(function),
            _ => None,
        })
        .expect("source should define an observe function");

    project
        .new_generator(tracing)
        .generate_raw(&function.body, &function.arguments, &module.name)
        .unwrap()
        .try_into()
        .unwrap()
}

/// A coerce costs nothing beyond converting from Data: it must compile to the same program as
/// taking a typed argument (which is converted from Data without any check), with or without
/// traces, and evaluate the same way.
fn assert_free(dependencies: &[(&str, &str)], baseline: &str, coerced: &str, inputs: &[Data]) {
    for tracing in [
        Tracing::All(TraceLevel::Silent),
        Tracing::All(TraceLevel::Verbose),
    ] {
        let baseline = compile(dependencies, baseline, tracing);
        let coerced = compile(dependencies, coerced, tracing);

        assert_eq!(baseline.to_pretty(), coerced.to_pretty());

        for input in inputs {
            let expected = baseline.apply_data(input.clone()).eval(ExBudget::default());
            let actual = coerced.apply_data(input.clone()).eval(ExBudget::default());

            assert_eq!(expected.cost(), actual.cost());
            assert_eq!(expected.result().unwrap(), actual.result().unwrap());
        }
    }
}

const ASSETS: (&str, &str) = (
    "cardano/assets",
    r#"
    pub opaque type Dict<key, value> {
      inner: Pairs<ByteArray, value>,
    }

    pub opaque type Assets {
      inner: Dict<ByteArray, Dict<ByteArray, Int>>,
    }
    "#,
);

fn assets(policies: u8) -> Data {
    Data::map(
        (0..policies)
            .map(|policy| {
                (
                    Data::bytestring(vec![policy]),
                    Data::map(vec![
                        (Data::bytestring(b"alpha".to_vec()), Data::integer(5.into())),
                        (Data::bytestring(b"beta".to_vec()), Data::integer(7.into())),
                    ]),
                )
            })
            .collect(),
    )
}

#[test]
fn coerce_into_record_is_free() {
    assert_free(
        &[],
        r#"
        pub type Datum { owner: ByteArray, amount: Int }

        pub fn observe(datum: Datum) -> Int {
          datum.amount
        }
        "#,
        r#"
        pub type Datum { owner: ByteArray, amount: Int }

        pub fn observe(data: Data) -> Int {
          coerce datum: Datum = data
          datum.amount
        }
        "#,
        &[Data::constr(
            0,
            vec![Data::bytestring(vec![0; 28]), Data::integer(42.into())],
        )],
    );
}

#[test]
fn coerce_into_primitive_is_free() {
    assert_free(
        &[],
        r#"
        pub fn observe(n: Int) -> Int {
          n + 1
        }
        "#,
        r#"
        pub fn observe(data: Data) -> Int {
          coerce n: Int = data
          n + 1
        }
        "#,
        &[Data::integer(41.into())],
    );
}

#[test]
fn coerce_into_list_is_free() {
    assert_free(
        &[],
        r#"
        pub fn observe(xs: List<Int>) -> Int {
          when xs is {
            [] -> 0
            [x, ..] -> x
          }
        }
        "#,
        r#"
        pub fn observe(data: Data) -> Int {
          coerce xs: List<Int> = data
          when xs is {
            [] -> 0
            [x, ..] -> x
          }
        }
        "#,
        &[
            Data::list(vec![]),
            Data::list(vec![Data::integer(1.into()), Data::integer(2.into())]),
        ],
    );
}

#[test]
fn coerce_through_nested_opaque_types_is_free() {
    assert_free(
        &[ASSETS],
        r#"
        pub fn observe(xs: Pairs<ByteArray, Pairs<ByteArray, Int>>) -> Data {
          as_data(xs)
        }
        "#,
        r#"
        use cardano/assets.{Assets}

        pub fn observe(xs: Pairs<ByteArray, Pairs<ByteArray, Int>>) -> Data {
          coerce assets: Assets = xs
          coerce pairs: Pairs<ByteArray, Pairs<ByteArray, Int>> = assets
          as_data(pairs)
        }
        "#,
        &[assets(0), assets(1), assets(8)],
    );
}

#[test]
fn coerce_from_data_into_opaque_type_is_free() {
    assert_free(
        &[ASSETS],
        r#"
        pub fn observe(xs: Pairs<ByteArray, Pairs<ByteArray, Int>>) -> Data {
          as_data(xs)
        }
        "#,
        r#"
        use cardano/assets.{Assets}

        pub fn observe(data: Data) -> Data {
          coerce assets: Assets = data
          coerce pairs: Pairs<ByteArray, Pairs<ByteArray, Int>> = assets
          as_data(pairs)
        }
        "#,
        &[assets(0), assets(1), assets(8)],
    );
}

#[test]
fn coerce_does_not_check() {
    let source = |keyword: &str| {
        format!(
            r#"
            pub type Datum {{ owner: ByteArray, amount: Int }}

            pub fn observe(data: Data) -> Int {{
              {keyword} datum: Datum = data
              datum.amount
            }}
            "#
        )
    };

    // The owner isn't a ByteArray, which expect notices but coerce trusts.
    let input = Data::constr(0, vec![Data::integer(1.into()), Data::integer(42.into())]);

    let tracing = Tracing::All(TraceLevel::Silent);

    let expected = compile(&[], &source("expect"), tracing)
        .apply_data(input.clone())
        .eval(ExBudget::default());

    assert!(expected.result().is_err());

    let coerced = compile(&[], &source("coerce"), tracing)
        .apply_data(input)
        .eval(ExBudget::default());

    assert_eq!(coerced.result().unwrap(), Term::integer(42.into()));
}
