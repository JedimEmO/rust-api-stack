use crate::persistence::PersistedUserProfile;
use bidirectional_chat_api::*;

pub(super) fn persisted_cat_breed(breed: CatBreed) -> &'static str {
    match breed {
        CatBreed::Tabby => "tabby",
        CatBreed::Siamese => "siamese",
        CatBreed::Persian => "persian",
        CatBreed::MaineCoon => "maine_coon",
        CatBreed::BritishShorthair => "british_shorthair",
        CatBreed::Ragdoll => "ragdoll",
        CatBreed::Sphynx => "sphynx",
        CatBreed::ScottishFold => "scottish_fold",
        CatBreed::Calico => "calico",
        CatBreed::Tuxedo => "tuxedo",
    }
}

pub(super) fn persisted_cat_color(color: CatColor) -> &'static str {
    match color {
        CatColor::Orange => "orange",
        CatColor::Black => "black",
        CatColor::White => "white",
        CatColor::Gray => "gray",
        CatColor::Brown => "brown",
        CatColor::Cream => "cream",
        CatColor::Blue => "blue",
        CatColor::Lilac => "lilac",
        CatColor::Cinnamon => "cinnamon",
        CatColor::Fawn => "fawn",
    }
}

pub(super) fn persisted_cat_expression(expression: CatExpression) -> &'static str {
    match expression {
        CatExpression::Happy => "happy",
        CatExpression::Sleepy => "sleepy",
        CatExpression::Curious => "curious",
        CatExpression::Playful => "playful",
        CatExpression::Content => "content",
        CatExpression::Alert => "alert",
        CatExpression::Grumpy => "grumpy",
        CatExpression::Loving => "loving",
    }
}

pub(super) fn cat_breed_from_persisted(value: &str) -> CatBreed {
    match value {
        "tabby" => CatBreed::Tabby,
        "siamese" => CatBreed::Siamese,
        "persian" => CatBreed::Persian,
        "maine_coon" => CatBreed::MaineCoon,
        "british_shorthair" => CatBreed::BritishShorthair,
        "ragdoll" => CatBreed::Ragdoll,
        "sphynx" => CatBreed::Sphynx,
        "scottish_fold" => CatBreed::ScottishFold,
        "calico" => CatBreed::Calico,
        "tuxedo" => CatBreed::Tuxedo,
        _ => CatBreed::Tabby,
    }
}

pub(super) fn cat_color_from_persisted(value: &str) -> CatColor {
    match value {
        "orange" => CatColor::Orange,
        "black" => CatColor::Black,
        "white" => CatColor::White,
        "gray" => CatColor::Gray,
        "brown" => CatColor::Brown,
        "cream" => CatColor::Cream,
        "blue" => CatColor::Blue,
        "lilac" => CatColor::Lilac,
        "cinnamon" => CatColor::Cinnamon,
        "fawn" => CatColor::Fawn,
        _ => CatColor::Orange,
    }
}

pub(super) fn cat_expression_from_persisted(value: &str) -> CatExpression {
    match value {
        "happy" => CatExpression::Happy,
        "sleepy" => CatExpression::Sleepy,
        "curious" => CatExpression::Curious,
        "playful" => CatExpression::Playful,
        "content" => CatExpression::Content,
        "alert" => CatExpression::Alert,
        "grumpy" => CatExpression::Grumpy,
        "loving" => CatExpression::Loving,
        _ => CatExpression::Happy,
    }
}

pub(super) fn user_profile_from_persisted(persisted: &PersistedUserProfile) -> UserProfile {
    UserProfile {
        username: persisted.username.clone(),
        display_name: persisted.display_name.clone(),
        avatar: CatAvatar {
            breed: cat_breed_from_persisted(&persisted.avatar.breed),
            color: cat_color_from_persisted(&persisted.avatar.color),
            expression: cat_expression_from_persisted(&persisted.avatar.expression),
        },
        created_at: persisted.created_at.to_rfc3339(),
        last_seen: persisted.last_seen.to_rfc3339(),
    }
}

// Chat server state
