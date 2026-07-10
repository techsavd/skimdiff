use std::fs;

use skimdiff::index::SymbolIndex;

fn write(dir: &std::path::Path, name: &str, content: &str) {
    fs::write(dir.join(name), content).unwrap();
}

#[test]
fn java_declarations_and_references() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        "Foo.java",
        "class Foo {\n    void bar() {}\n    void baz() {\n        bar();\n    }\n}\n",
    );

    let idx = SymbolIndex::build(tmp.path(), &["Foo.java".into()]);
    let hit = idx.lookup("bar");
    assert_eq!(hit.declarations.len(), 1, "one declaration of bar");
    assert_eq!(hit.declarations[0].line, 2);
    assert_eq!(hit.declarations[0].path, "Foo.java");
    assert_eq!(hit.references.len(), 1, "one call site");
    assert_eq!(hit.references[0].line, 4);

    let class_hit = idx.lookup("Foo");
    assert_eq!(class_hit.declarations.len(), 1);
    assert_eq!(class_hit.declarations[0].line, 1);
}

#[test]
fn kotlin_functions_resolve() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        "app.kt",
        "fun greet() {}\n\nfun main() {\n    greet()\n}\n",
    );

    let idx = SymbolIndex::build(tmp.path(), &["app.kt".into()]);
    let hit = idx.lookup("greet");
    assert_eq!(hit.declarations.len(), 1);
    assert_eq!(hit.declarations[0].line, 1);
    assert_eq!(hit.references.len(), 1);
    assert_eq!(hit.references[0].line, 4);
}

#[test]
fn strings_and_comments_are_not_references() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        "S.java",
        "class S {\n    // bar is mentioned here\n    String s = \"bar\";\n    void bar() {}\n}\n",
    );

    let idx = SymbolIndex::build(tmp.path(), &["S.java".into()]);
    let hit = idx.lookup("bar");
    assert_eq!(hit.declarations.len(), 1);
    assert!(
        hit.references.is_empty(),
        "comment/string mentions must not count: {:?}",
        hit.references
    );
}

#[test]
fn unknown_symbol_is_empty_and_cross_file_refs_found() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "A.java", "class A {\n    void hit() {}\n}\n");
    write(
        tmp.path(),
        "B.java",
        "class B {\n    void go(A a) {\n        a.hit();\n    }\n}\n",
    );

    let idx = SymbolIndex::build(tmp.path(), &["A.java".into(), "B.java".into()]);
    assert!(idx.lookup("nope").declarations.is_empty());
    assert!(idx.lookup("nope").references.is_empty());

    let hit = idx.lookup("hit");
    assert_eq!(hit.declarations.len(), 1);
    assert_eq!(hit.declarations[0].path, "A.java");
    assert_eq!(hit.references.len(), 1);
    assert_eq!(hit.references[0].path, "B.java");
    // reference context carries the source line for preview
    assert!(hit.references[0].context.contains("a.hit()"));
}

#[test]
fn rebuild_reflects_edits() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "A.java", "class A {\n    void hit() {}\n}\n");

    let idx = SymbolIndex::build(tmp.path(), &["A.java".into()]);
    assert_eq!(idx.lookup("hit").declarations.len(), 1);

    write(tmp.path(), "A.java", "class A {\n    void miss() {}\n}\n");
    let idx = SymbolIndex::build(tmp.path(), &["A.java".into()]);
    assert!(idx.lookup("hit").declarations.is_empty());
    assert_eq!(idx.lookup("miss").declarations.len(), 1);
}
