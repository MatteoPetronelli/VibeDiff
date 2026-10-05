use super::*;
use std::path::PathBuf;

#[test]
fn test_csharp_formatting_and_comment_rejection() {
    let old_code = format!(
        "public class MathService {{\n    public int CalculateSum(int a, int b) {{\n        {} calculate sum of integers\n        int result = a + b;\n        return result;\n    }}\n}}\n",
        "\x2F\x2F"
    );

    let new_code = format!(
        "public class MathService {{\n    public int CalculateSum(\n        int a,\n\n        int b\n    )\n    {{\n        {} alternate block comment {}\n        int    result    =    a    +    b;\n        {} another trailing comment\n        return result;\n    }}\n}}\n",
        "\x2F\x2A", "\x2A\x2F", "\x2F\x2F"
    );

    let diff = diff_source(
        PathBuf::from("MathService.cs"),
        SupportedLanguage::CSharp,
        &old_code,
        &new_code,
    )
    .expect("diff should succeed");

    assert!(diff.hunks.is_empty());
}

#[test]
fn test_rust_signature_mutation_contract_break() {
    let old_code = "pub fn run(x: u32) -> bool {\n    x > 10\n}\n";
    let new_code = "pub fn run(x: u32, y: String) -> bool {\n    x > 10 && !y.is_empty()\n}\n";

    let diff = diff_source(
        PathBuf::from("service.rs"),
        SupportedLanguage::Rust,
        old_code,
        new_code,
    )
    .expect("diff should succeed");

    assert_eq!(diff.hunks.len(), 1);
    let hunk = &diff.hunks[0];
    assert_eq!(hunk.symbol_name, "run");
    assert_eq!(hunk.kind, AstChangeKind::ContractBroken);
    assert!(hunk.old_node.is_some());
    assert!(hunk.new_node.is_some());
    assert!(hunk.old_node.as_ref().unwrap().contains("run(x: u32)"));
    assert!(hunk
        .new_node
        .as_ref()
        .unwrap()
        .contains("run(x: u32, y: String)"));
}

#[test]
fn test_unity_hot_path_add_item_mutation() {
    let old_code = "public class InventoryController {\n    public void AddItem(Item item) {\n        if (item == null) return;\n        this.items.Add(item);\n    }\n}\n";
    let new_code = "public class InventoryController {\n    public void AddItem(Item item) {\n        if (item == null) {\n            throw new System.ArgumentNullException(\"item\");\n        }\n        this.items.Add(item);\n    }\n}\n";

    let diff = diff_source(
        PathBuf::from("InventoryController.cs"),
        SupportedLanguage::CSharp,
        old_code,
        new_code,
    )
    .expect("diff should succeed");

    assert_eq!(diff.hunks.len(), 1);
    let hunk = &diff.hunks[0];
    assert_eq!(hunk.symbol_name, "AddItem");
    assert_eq!(hunk.kind, AstChangeKind::Modified);
    assert!(hunk.old_node.is_some());
    assert!(hunk.new_node.is_some());
    assert!(hunk.old_node.as_ref().unwrap().contains("return;"));
    assert!(hunk
        .new_node
        .as_ref()
        .unwrap()
        .contains("ArgumentNullException"));
}

#[test]
fn test_added_and_deleted_functions() {
    let old_code = "def existing_func():\n    return 42\n\ndef deprecated_func():\n    return 0\n";
    let new_code = "def existing_func():\n    return 42\n\ndef brand_new_func():\n    return 100\n";

    let diff = diff_source(
        PathBuf::from("script.py"),
        SupportedLanguage::Python,
        old_code,
        new_code,
    )
    .expect("diff should succeed");

    assert_eq!(diff.hunks.len(), 2);
    let deleted_hunk = diff
        .hunks
        .iter()
        .find(|h| h.symbol_name == "deprecated_func")
        .expect("deprecated_func should be present");
    assert_eq!(deleted_hunk.kind, AstChangeKind::Deleted);
    assert!(deleted_hunk.old_node.is_some());
    assert!(deleted_hunk.new_node.is_none());

    let added_hunk = diff
        .hunks
        .iter()
        .find(|h| h.symbol_name == "brand_new_func")
        .expect("brand_new_func should be present");
    assert_eq!(added_hunk.kind, AstChangeKind::Added);
    assert!(added_hunk.old_node.is_none());
    assert!(added_hunk.new_node.is_some());
}

#[test]
fn test_llm_payload_serialization() {
    let old_code = "fn greet() -> &'static str {\n    \"hello\"\n}\n";
    let new_code = "fn greet() -> &'static str {\n    \"world\"\n}\n";

    let diff = diff_source(
        PathBuf::from("src/greet.rs"),
        SupportedLanguage::Rust,
        old_code,
        new_code,
    )
    .expect("diff should succeed");

    let payload = diff.to_llm_payload();
    assert!(payload.contains("FILE: src/greet.rs [rust]"));
    assert!(payload.contains("--- SYMBOL: greet [MODIFIED] ---"));
    assert!(payload.contains("<<< OLD"));
    assert!(payload.contains(">>> NEW"));
}
