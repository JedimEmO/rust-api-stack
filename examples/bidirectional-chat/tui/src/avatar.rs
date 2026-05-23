use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

// Different cat faces for animation frames
const CAT_FRAMES: &[&[&str]] = &[
    // Frame 1 - Normal
    &[" /\\_/\\  ", "( o.o ) ", " > ^ <  "],
    // Frame 2 - Blinking
    &[" /\\_/\\  ", "( -.o ) ", " > ^ <  "],
    // Frame 3 - Winking
    &[" /\\_/\\  ", "( o.- ) ", " > ^ <  "],
    // Frame 4 - Happy
    &[" /\\_/\\  ", "( ^.^ ) ", " > ^ <  "],
];

// Different cat variations based on username hash
const CAT_VARIATIONS: &[&[&str]] = &[
    // Classic cat
    &[" /\\_/\\  ", "( o.o ) ", " > ^ <  "],
    // Chubby cat
    &[" /\\_/\\  ", "( o.o ) ", " (> <)  "],
    // Sleepy cat
    &[" /\\_/\\  ", "( -.-)  ", " > ^ <  "],
    // Alert cat
    &[" /|_|\\  ", "( O.O ) ", " > ^ <  "],
    // Cool cat
    &[" /\\_/\\  ", "( ■.■ ) ", " > ^ <  "],
    // Surprised cat
    &[" /\\_/\\  ", "( O.O ) ", " > o <  "],
];

pub struct AvatarManager {
    user_avatars: HashMap<String, usize>,
    frame_counter: usize,
}

impl AvatarManager {
    pub fn new() -> Self {
        Self {
            user_avatars: HashMap::new(),
            frame_counter: 0,
        }
    }

    pub fn tick(&mut self) {
        self.frame_counter = (self.frame_counter + 1) % 40; // Animation cycle
    }

    pub fn get_avatar_for_user(&mut self, username: &str) -> Vec<String> {
        // Assign a consistent avatar variation to each user based on their username
        let avatar_index = self.user_avatars.get(username).copied().unwrap_or_else(|| {
            let mut hasher = DefaultHasher::new();
            username.hash(&mut hasher);
            let hash = hasher.finish();
            let index = (hash % CAT_VARIATIONS.len() as u64) as usize;
            self.user_avatars.insert(username.to_string(), index);
            index
        });

        // Get base avatar
        let base_avatar = CAT_VARIATIONS[avatar_index];

        // Determine animation frame based on counter
        let animated_avatar = if self.frame_counter < 30 {
            // Normal face most of the time
            base_avatar
        } else if self.frame_counter < 35 {
            // Blink or wink occasionally
            if avatar_index % 2 == 0 {
                CAT_FRAMES[1] // Blink
            } else {
                CAT_FRAMES[2] // Wink
            }
        } else {
            // Happy face sometimes
            CAT_FRAMES[3]
        };

        // Convert to Vec<String> for easier manipulation
        animated_avatar
            .iter()
            .map(|&line| line.to_string())
            .collect()
    }

    pub fn get_typing_avatar_for_user(&mut self, username: &str) -> Vec<String> {
        // Get the base avatar for the user
        let avatar_index = self.user_avatars.get(username).copied().unwrap_or_else(|| {
            let mut hasher = DefaultHasher::new();
            username.hash(&mut hasher);
            let hash = hasher.finish();
            let index = (hash % CAT_VARIATIONS.len() as u64) as usize;
            self.user_avatars.insert(username.to_string(), index);
            index
        });

        // Get base avatar
        let base_avatar = CAT_VARIATIONS[avatar_index];

        // Animated dots based on frame counter
        let dots = match (self.frame_counter / 10) % 4 {
            0 => "",
            1 => ".",
            2 => "..",
            _ => "...",
        };

        // Create avatar with speech bubble
        vec![
            format!("{} ( {} )", base_avatar[0], dots),
            base_avatar[1].to_string(),
            base_avatar[2].to_string(),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_typing_avatar_with_bubble() {
        let mut avatar_manager = AvatarManager::new();

        // Test normal avatar
        let normal_avatar = avatar_manager.get_avatar_for_user("TestUser");
        assert_eq!(normal_avatar.len(), 3);
        assert!(!normal_avatar[0].contains("("));

        // Test typing avatar with chat bubble
        let typing_avatar = avatar_manager.get_typing_avatar_for_user("TestUser");
        assert_eq!(typing_avatar.len(), 3);
        assert!(typing_avatar[0].contains("("));
        assert!(typing_avatar[0].contains(")"));

        // Test animation by advancing frames
        for _ in 0..15 {
            avatar_manager.tick();
        }
        let typing_avatar2 = avatar_manager.get_typing_avatar_for_user("TestUser");
        assert!(typing_avatar2[0].contains(".")); // Should have dots

        // Test different users get different base avatars
        let user2_avatar = avatar_manager.get_typing_avatar_for_user("AnotherUser");
        // They might or might not be different due to hash, but should have bubble
        assert!(user2_avatar[0].contains("("));
    }
}
