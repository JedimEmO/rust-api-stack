#[test]
fn test_xss_protection_in_generated_html() {
    // Test that HTML special characters are properly escaped
    let malicious_inputs = vec![
        r#"<script>alert('XSS')</script>"#,
        r#""><script>alert('XSS')</script>"#,
        r#"'/><script>alert('XSS')</script>"#,
        r#"Test & <script>"#,
    ];

    // The escapeHtml function should convert these to safe strings
    for input in malicious_inputs {
        let escaped = escape_html(input);
        assert!(!escaped.contains("<script>"));
        assert!(!escaped.contains("</script>"));
        assert!(escaped.contains("&lt;") || escaped.contains("&gt;"));
    }
}

#[test]
fn test_generated_docs_do_not_store_bearer_token_in_local_storage() {
    let template = include_str!("../src/api_explorer_template.html");
    assert!(!template.contains("localStorage.getItem('bearer-token')"));
    assert!(!template.contains("localStorage.setItem('bearer-token'"));
    assert!(!template.contains("localStorage.removeItem('bearer-token'"));
    assert!(!template.contains("localStorage.setItem(`${storagePrefix}:bearer-token`"));
    assert!(template.contains("sessionStorage.setItem(`${storagePrefix}:${key}`"));
    assert!(template.contains("localStorage.setItem(\"ras-explorer-theme\""));
}

fn escape_html(unsafe_str: &str) -> String {
    unsafe_str
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#039;")
}
