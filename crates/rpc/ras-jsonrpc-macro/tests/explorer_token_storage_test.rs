#[test]
fn test_generated_explorer_does_not_store_bearer_token_in_local_storage() {
    let template = include_str!("../../../rest/ras-rest-macro/src/api_explorer_template.html");
    assert!(!template.contains("localStorage.getItem('bearer-token')"));
    assert!(!template.contains("localStorage.setItem('bearer-token'"));
    assert!(!template.contains("localStorage.removeItem('bearer-token'"));
    assert!(!template.contains("localStorage.setItem(`${storagePrefix}:bearer-token`"));
    assert!(template.contains("sessionStorage.setItem(`${storagePrefix}:${key}`"));
    assert!(template.contains("localStorage.setItem(\"ras-explorer-theme\""));
}
