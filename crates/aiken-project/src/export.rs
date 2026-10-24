use crate::{
    blueprint::{
        self,
        definitions::Definitions,
        parameter::Parameter,
        schema::{Annotated, Declaration, Schema},
    },
    module::{CheckedModule, CheckedModules},
};
use aiken_lang::{
    ast::{ArgName, Span, TypedArg, TypedFunction},
    gen_uplc::CodeGenerator,
    plutus_version::PlutusVersion,
    tipo::{Type, error as tipo, requirement},
};
use miette::NamedSource;
use std::rc::Rc;
use uplc::ast::SerializableProgram;

#[derive(Debug, PartialEq, Clone, serde::Serialize, serde::Deserialize)]
pub struct Export {
    pub name: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub doc: Option<String>,

    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[serde(default)]
    pub parameters: Vec<Parameter>,

    pub return_type: Parameter,

    #[serde(flatten)]
    pub program: SerializableProgram,

    #[serde(skip_serializing_if = "Definitions::is_empty")]
    #[serde(default)]
    pub definitions: Definitions<Annotated<Schema>>,
}

impl Export {
    pub fn from_function(
        func: &TypedFunction,
        module: &CheckedModule,
        generator: &mut CodeGenerator,
        modules: &CheckedModules,
        plutus_version: &PlutusVersion,
    ) -> Result<Export, crate::Error> {
        let mut definitions = Definitions::new();

        let illegal_opaque_type =
            |module: &CheckedModule, tipo: Rc<Type>, location: Span, position: &str| {
                crate::Error::Type {
                    path: Box::new(module.input_path.clone()),
                    src: Box::new(module.code.clone()),
                    named: Box::new(NamedSource::new(
                        module.input_path.display().to_string(),
                        module.code.clone(),
                    )),
                    error: Box::new(tipo::Error::IllegalOpaqueType {
                        tipo,
                        location,
                        position: position.to_string(),
                    }),
                }
            };

        // A value of the type can hold a value of an opaque type, which an off-chain caller
        // could build breaking its invariants; a phantom argument holds none.
        let is_opaque = |tipo: &Rc<Type>| {
            requirement::holds_opaque(tipo, &|module_name, name| {
                modules
                    .get(module_name)
                    .and_then(|module| module.ast.type_info.layouts.get(name))
            })
        };

        let parameters = func
            .arguments
            .iter()
            .map(|param| {
                if is_opaque(&param.tipo) {
                    return Err(illegal_opaque_type(
                        module,
                        param.tipo.clone(),
                        param.location,
                        "function argument",
                    ));
                }

                Annotated::from_type(
                    modules.into(),
                    blueprint::validator::tipo_or_annotation(module, param),
                    &mut definitions,
                )
                .map(|schema| Parameter {
                    title: Some(param.arg_name.get_label()),
                    schema: Declaration::Referenced(schema),
                })
                .map_err(|error| {
                    crate::Error::Blueprint(Box::new(blueprint::Error::Schema {
                        error: Box::new(error),
                        location: param.location,
                        source_code: NamedSource::new(
                            module.input_path.display().to_string(),
                            module.code.clone(),
                        ),
                    }))
                })
            })
            .collect::<Result<_, _>>()?;

        if is_opaque(&func.return_type) {
            return Err(illegal_opaque_type(
                module,
                func.return_type.clone(),
                func.location,
                "function return type",
            ));
        }

        let return_type = Annotated::from_type(
            modules.into(),
            blueprint::validator::tipo_or_annotation(
                module,
                &TypedArg {
                    arg_name: ArgName::Discarded {
                        name: "".to_string(),
                        label: "".to_string(),
                        location: Span::empty(),
                    },
                    location: Span::empty(),
                    annotation: func.return_annotation.clone(),
                    doc: None,
                    is_validator_param: false,
                    tipo: func.return_type.clone(),
                },
            ),
            &mut definitions,
        )
        .map(|schema| Parameter {
            title: Some("return_type".to_string()),
            schema: Declaration::Referenced(schema),
        })
        .map_err(|error| {
            crate::Error::Blueprint(Box::new(blueprint::Error::Schema {
                error: Box::new(error),
                location: func.location,
                source_code: NamedSource::new(
                    module.input_path.display().to_string(),
                    module.code.clone(),
                ),
            }))
        })?;

        let program = generator
            .generate_raw(&func.body, &func.arguments, &module.name)?
            .to_debruijn()
            .unwrap();

        let program = match plutus_version {
            PlutusVersion::V1 => SerializableProgram::PlutusV1Program(program),
            PlutusVersion::V2 => SerializableProgram::PlutusV2Program(program),
            PlutusVersion::V3 => SerializableProgram::PlutusV3Program(program),
        };

        Ok(Export {
            name: format!("{}.{}", module.name, func.name),
            doc: func.doc.clone(),
            parameters,
            return_type,
            program,
            definitions,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{CheckedModules, Export};
    use crate::tests::TestProject;
    use aiken_lang::{
        self,
        ast::{TraceLevel, Tracing},
        plutus_version::PlutusVersion,
    };

    macro_rules! assert_export {
        ($code:expr) => {
            let mut project = TestProject::new();

            let modules = CheckedModules::singleton(project.check(project.parse(indoc::indoc! { $code })));

            let mut generator = project.new_generator(
                Tracing::All(TraceLevel::Verbose),
            );

            let (module, func) = modules
                .functions()
                .next()
                .expect("source code did no yield any exports");

            let export = Export::from_function(func, module, &mut generator, &modules, &PlutusVersion::default());

            match export {
                Err(e) => insta::with_settings!({
                    description => concat!("Code:\n\n", indoc::indoc! { $code }),
                    omit_expression => true
                }, {
                    insta::assert_debug_snapshot!(e);
                }),

                Ok(validator) => insta::with_settings!({
                    description => concat!("Code:\n\n", indoc::indoc! { $code }),
                    omit_expression => true
                }, {
                    insta::assert_json_snapshot!(validator);
                }),
            };
        };
    }

    #[test]
    fn basic_export() {
        assert_export!(
            r#"
            pub fn add(a: Int, b: Int) -> Int {
                a + b
            }
            "#
        );
    }

    #[test]
    fn illegal_opaque_type() {
        assert_export!(
            r#"
            pub opaque type Thing {
              a: Int
            }

            pub fn add(a: Thing, b: Int) -> Int {
                a.a + b
            }
            "#
        );
    }

    #[test]
    fn recursive_types() {
        assert_export!(
            r#"
            pub type Foo<a> {
              Empty
              Bar(a, Foo<a>)
            }

            pub fn add(a: Foo<Int>, b: Foo<Int>) -> Int {
              when (a, b) is {
                (Empty, Empty) -> 0
                (Bar(x, y), Bar(c, d)) -> x + c + add(y, d)
                (Empty, Bar(c, d)) -> c + add(Empty, d)
                (Bar(x, y), Empty) -> x + add(y, Empty)
              }
            }
            "#
        );
    }

    #[test]
    fn cannot_export_generics() {
        assert_export!(
            r#"
            pub fn add(_a: a, _b: b) -> Bool {
                True
            }
            "#
        );
    }

    fn export(code: &str) -> Result<Export, crate::Error> {
        let mut project = TestProject::new();
        let modules = CheckedModules::singleton(project.check(project.parse(code)));
        let mut generator = project.new_generator(Tracing::All(TraceLevel::Verbose));
        let (module, func) = modules
            .functions()
            .next()
            .expect("source code did no yield any exports");

        Export::from_function(
            func,
            module,
            &mut generator,
            &modules,
            &PlutusVersion::default(),
        )
    }

    fn is_illegal_opaque_type(result: &Result<Export, crate::Error>) -> bool {
        matches!(
            result,
            Err(crate::Error::Type { error, .. })
                if matches!(**error, aiken_lang::tipo::error::Error::IllegalOpaqueType { .. })
        )
    }

    #[test]
    fn an_opaque_type_in_an_argument_type_argument_is_refused() {
        let result = export(
            r#"
            pub opaque type Thing {
              a: Int
            }

            pub fn count(things: Option<Thing>) -> Int {
              when things is {
                Some(_) -> 1
                None -> 0
              }
            }
            "#,
        );

        assert!(is_illegal_opaque_type(&result), "{result:#?}");
    }

    #[test]
    fn an_opaque_type_in_a_return_type_argument_is_refused() {
        let result = export(
            r#"
            pub opaque type Thing {
              a: Int
            }

            pub fn things() -> List<Thing> {
              []
            }
            "#,
        );

        assert!(is_illegal_opaque_type(&result), "{result:#?}");
    }

    /// A phantom argument holds no value of the opaque type, through a data type or an alias.
    #[test]
    fn a_phantom_opaque_argument_is_accepted() {
        let result = export(
            r#"
            pub opaque type Thing {
              a: Int
            }

            pub type Tagged<a> {
              Tagged(Int)
            }

            pub type Tag<a> =
              Int

            pub fn add(tagged: Tagged<Thing>, tag: Tag<Thing>) -> Int {
              let Tagged(n) = tagged
              n + tag
            }
            "#,
        );

        assert!(result.is_ok(), "{result:#?}");
    }
}
