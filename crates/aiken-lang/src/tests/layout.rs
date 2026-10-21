use super::check::{check_with_deps, parse, parse_as};
use crate::tipo::{
    DataLayout,
    layout::{Contents, Representation},
};

fn layouts(source: &str, deps: &[(&str, &str)]) -> std::collections::HashMap<String, DataLayout> {
    let deps = deps
        .iter()
        .map(|(name, source)| parse_as(source, name))
        .collect();

    let (_, module) = check_with_deps(parse(source), deps).expect("module should type-check");

    module.type_info.layouts
}

#[test]
fn layouts_of_tags_erasure_and_phantom_parameters() {
    let layouts = layouts(
        r#"
        @tag(5)
        pub type R { a: Int }

        pub type S {
          @tag(3)
          A
          B
        }

        @tag(7)
        pub type T {
          @tag(2)
          T(Int)
        }

        pub opaque type Erased { Erased(Int) }
        pub opaque type TaggedConstructor { @tag(3) TaggedConstructor(Int) }
        @tag(1)
        pub opaque type TaggedType { inner: Int }
        @list
        pub opaque type Listed { inner: Int }
        pub opaque type TwoFields { a: Int, b: Int }
        pub type Transparent { Transparent(Int) }

        pub type Tagged<a> { Tagged(Int) }
        pub type Wrap<a> { Wrap(Tagged<a>) }
        pub type Holds<a> { Holds(List<a>) }
        pub type Loop<a> { Stop  Again(Loop<a>) }
        pub type Tree<a> { Leaf(a)  Node(Tree<a>, Tree<a>) }
        pub type Both<a, b> { Both(Tagged<a>, Option<b>) }
        "#,
        &[],
    );

    // A tag on a constructor beats the type's tag, which beats the constructor's position.
    let index = |ty: &str, ctor: &str| layouts[ty].constructor(ctor).unwrap().index;

    assert_eq!(index("R", "R"), 5);
    assert_eq!(index("S", "A"), 3);
    assert_eq!(index("S", "B"), 1);
    assert_eq!(index("T", "T"), 2);

    let representation = |ty: &str| layouts[ty].representation;

    assert_eq!(representation("Erased"), Representation::Erased);
    assert_eq!(representation("TaggedConstructor"), Representation::Constr);
    assert_eq!(representation("TaggedType"), Representation::Constr);
    assert_eq!(representation("Listed"), Representation::List);
    assert_eq!(representation("TwoFields"), Representation::Constr);
    assert_eq!(representation("Transparent"), Representation::Constr);

    let stored = |ty: &str| layouts[ty].summary.stored.clone();

    assert_eq!(stored("Tagged"), [false]);
    assert_eq!(stored("Wrap"), [false]);
    assert_eq!(stored("Holds"), [true]);
    assert_eq!(stored("Loop"), [false]);
    assert_eq!(stored("Tree"), [true]);
    assert_eq!(stored("Both"), [false, true]);
}

#[test]
fn layout_contents_of_local_and_imported_types() {
    let imported = layouts(
        r#"
        use m.{Hidden, Secret}

        pub type Tagged<a> { Tagged(Int) }
        pub type Phantom { p: Tagged<Secret> }
        pub type Direct { s: Secret }
        pub type Imported { h: Hidden }
        pub type Text { t: Option<String> }
        pub type Points { p: List<G1Element> }
        pub type Plain { i: Int, d: Data }
        "#,
        &[(
            "m",
            r#"
            pub opaque type Secret { inner: Int }
            pub type Hidden { h: Option<Secret> }
            "#,
        )],
    );

    let contents = |ty: &str| imported[ty].summary.contents;

    let opaque = Contents {
        opaque: true,
        ..Contents::default()
    };
    let untestable = Contents {
        untestable: true,
        ..Contents::default()
    };

    assert_eq!(contents("Phantom"), Contents::default());
    assert_eq!(contents("Direct"), opaque);
    assert_eq!(contents("Imported"), opaque);
    assert_eq!(contents("Text"), untestable);
    assert_eq!(contents("Points"), untestable);
    assert_eq!(contents("Plain"), Contents::default());

    // A local type named like a prelude type doesn't change what an imported type holds.
    let shadowed = layouts(
        r#"
        use m.{Hidden}

        pub type Option<a> { Nope(Int) }
        pub type Local { h: Hidden, o: Option<String> }
        "#,
        &[(
            "m",
            r#"
            pub opaque type Secret { inner: Int }
            pub type Hidden { h: Option<Secret> }
            "#,
        )],
    );

    assert!(shadowed["Local"].summary.contents.opaque);
    assert!(!shadowed["Local"].summary.contents.untestable);
    assert_eq!(shadowed["Option"].summary.stored, [false]);
}
