use super::*;

#[test]
fn test_pkce_generation() {
    let pkce1 = PkceChallenge::new();
    let pkce2 = PkceChallenge::new();

    // Verifiers should be different
    assert_ne!(pkce1.code_verifier, pkce2.code_verifier);

    // Challenges should be different
    assert_ne!(pkce1.code_challenge, pkce2.code_challenge);

    // Method should be S256
    assert_eq!(pkce1.code_challenge_method, "S256");

    // Verify the challenge is correctly generated
    let expected_challenge = PkceChallenge::generate_code_challenge(&pkce1.code_verifier);
    assert_eq!(pkce1.code_challenge, expected_challenge);
}
