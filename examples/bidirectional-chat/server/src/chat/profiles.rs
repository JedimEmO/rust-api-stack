use super::*;

impl ChatServer {
    pub(super) async fn get_profile(
        &self,
        _client_id: ConnectionId,
        _connection_manager: &dyn ConnectionManager,
        _user: &AuthenticatedUser,
        request: GetProfileRequest,
    ) -> Result<GetProfileResponse, Box<dyn std::error::Error + Send + Sync>> {
        // Load current state
        let state = self.persistence.load_state().await?;

        // Get profile from persistence or create default
        let profile = if let Some(persisted) = state.user_profiles.get(&request.username) {
            user_profile_from_persisted(persisted)
        } else {
            // Create default profile
            UserProfile {
                username: request.username.clone(),
                display_name: None,
                avatar: CatAvatar {
                    breed: CatBreed::Tabby,
                    color: CatColor::Orange,
                    expression: CatExpression::Happy,
                },
                created_at: Utc::now().to_rfc3339(),
                last_seen: Utc::now().to_rfc3339(),
            }
        };

        Ok(GetProfileResponse { profile })
    }
    pub(super) async fn update_profile(
        &self,
        _client_id: ConnectionId,
        _connection_manager: &dyn ConnectionManager,
        user: &AuthenticatedUser,
        request: UpdateProfileRequest,
    ) -> Result<UpdateProfileResponse, Box<dyn std::error::Error + Send + Sync>> {
        // Load current state
        let mut state = self.persistence.load_state().await?;

        // Get existing profile or create new one
        let mut persisted_profile = state
            .user_profiles
            .get(&user.user_id)
            .cloned()
            .unwrap_or_else(|| PersistedUserProfile {
                username: user.user_id.clone(),
                display_name: None,
                avatar: PersistedCatAvatar {
                    breed: "tabby".to_string(),
                    color: "orange".to_string(),
                    expression: "happy".to_string(),
                },
                created_at: Utc::now(),
                last_seen: Utc::now(),
            });

        // Update fields if provided
        if let Some(display_name) = request.display_name {
            persisted_profile.display_name = Some(display_name);
        }

        if let Some(avatar) = request.avatar {
            persisted_profile.avatar = PersistedCatAvatar {
                breed: persisted_cat_breed(avatar.breed).to_string(),
                color: persisted_cat_color(avatar.color).to_string(),
                expression: persisted_cat_expression(avatar.expression).to_string(),
            };
        }

        // Update last seen
        persisted_profile.last_seen = Utc::now();

        // Save to persistence
        state
            .user_profiles
            .insert(user.user_id.clone(), persisted_profile.clone());
        self.persistence.save_state(&state).await?;

        // Convert to response
        let profile = user_profile_from_persisted(&persisted_profile);

        Ok(UpdateProfileResponse { profile })
    }
}
