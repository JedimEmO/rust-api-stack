use anyhow::Result;
use reqwest::Client;

// Use types from the bidirectional-chat-api crate
use bidirectional_chat_api::auth::{
    LoginRequest, LoginResponse, RegisterRequest, RegisterResponse,
};

pub struct AuthClient {
    client: Client,
    base_url: String,
}

impl AuthClient {
    pub fn new(base_url: String) -> Self {
        Self {
            client: Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
        }
    }

    pub async fn login(&self, username: String, password: String) -> Result<LoginResponse> {
        let response = self
            .client
            .post(self.login_url())
            .json(&Self::login_request(username, password))
            .send()
            .await?;

        if response.status().is_success() {
            Ok(response.json().await?)
        } else {
            let error_text = response.text().await?;
            anyhow::bail!("Login failed: {}", error_text)
        }
    }

    pub async fn register(&self, username: String, password: String) -> Result<RegisterResponse> {
        let response = self
            .client
            .post(self.register_url())
            .json(&Self::register_request(username, password))
            .send()
            .await?;

        if response.status().is_success() {
            Ok(response.json().await?)
        } else {
            let error_text = response.text().await?;
            anyhow::bail!("Registration failed: {}", error_text)
        }
    }

    fn login_url(&self) -> String {
        format!("{}/auth/login", self.base_url)
    }

    fn register_url(&self) -> String {
        format!("{}/auth/register", self.base_url)
    }

    fn login_request(username: String, password: String) -> LoginRequest {
        LoginRequest {
            username,
            password,
            provider: None, // Use default "local" provider
        }
    }

    fn register_request(username: String, password: String) -> RegisterRequest {
        RegisterRequest {
            username,
            password,
            email: None,
            display_name: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_client_normalizes_trailing_slash_in_base_url() {
        let client = AuthClient::new("http://localhost:3000/".to_string());

        assert_eq!(client.login_url(), "http://localhost:3000/auth/login");
        assert_eq!(client.register_url(), "http://localhost:3000/auth/register");
    }

    #[test]
    fn login_request_uses_default_local_provider() {
        let request = AuthClient::login_request("alice".to_string(), "secret".to_string());

        assert_eq!(request.username, "alice");
        assert_eq!(request.password, "secret");
        assert_eq!(request.provider, None);
    }

    #[test]
    fn register_request_leaves_optional_profile_fields_empty() {
        let request = AuthClient::register_request("bob".to_string(), "hunter2".to_string());

        assert_eq!(request.username, "bob");
        assert_eq!(request.password, "hunter2");
        assert_eq!(request.email, None);
        assert_eq!(request.display_name, None);
    }
}
