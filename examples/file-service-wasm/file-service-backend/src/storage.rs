use anyhow::Result;
use std::path::PathBuf;
use uuid::Uuid;

/// Extensions preserved on disk; anything else is stored as `.bin` so an
/// attacker-supplied filename cannot smuggle a renderable extension (e.g.
/// `.html`) into the download path.
const SAFE_EXTENSIONS: &[&str] = &[
    "txt", "png", "jpg", "jpeg", "webp", "gif", "pdf", "json", "bin",
];

/// Content types the example will echo back on download. Everything else is
/// served as `application/octet-stream` so uploaded content can never be
/// rendered inline by a browser.
const SAFE_CONTENT_TYPES: &[&str] = &[
    "text/plain",
    "image/png",
    "image/jpeg",
    "image/webp",
    "image/gif",
    "application/pdf",
    "application/json",
];

#[derive(Debug, Clone)]
pub struct FileMetadata {
    pub id: String,
    pub original_name: String,
    pub size: u64,
    pub content_type: Option<String>,
    pub stored_path: PathBuf,
}

pub struct FileStorage {
    base_path: PathBuf,
}

impl FileStorage {
    pub fn new(base_path: impl Into<PathBuf>) -> Self {
        Self {
            base_path: base_path.into(),
        }
    }

    /// Directory for a file's scope: anonymous uploads land in `public/`,
    /// owned uploads under `users/{owner}/` so downloads can enforce
    /// object ownership by construction.
    fn scope_dir(&self, owner: Option<&str>) -> Result<PathBuf> {
        match owner {
            None => Ok(self.base_path.join("public")),
            Some(owner) => {
                // The owner id becomes a path segment: restrict it to a safe
                // charset instead of trusting arbitrary subject strings.
                if owner.is_empty()
                    || !owner
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                {
                    anyhow::bail!("invalid owner id");
                }
                Ok(self.base_path.join("users").join(owner))
            }
        }
    }

    pub async fn save_file(
        &self,
        data: Vec<u8>,
        original_name: &str,
        content_type: Option<String>,
        owner: Option<&str>,
    ) -> Result<FileMetadata> {
        let file_id = Uuid::new_v4().to_string();
        let extension = std::path::Path::new(original_name)
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase())
            .filter(|ext| SAFE_EXTENSIONS.contains(&ext.as_str()))
            .unwrap_or_else(|| "bin".to_string());

        let dir = self.scope_dir(owner)?;
        tokio::fs::create_dir_all(&dir).await?;

        let stored_name = format!("{}.{}", file_id, extension);
        let stored_path = dir.join(&stored_name);

        // Save file
        tokio::fs::write(&stored_path, &data).await?;

        Ok(FileMetadata {
            id: file_id.clone(),
            original_name: original_name.to_string(),
            size: data.len() as u64,
            content_type,
            stored_path,
        })
    }

    /// Fetch a file by id within a scope. Only files saved with the same
    /// `owner` are visible, so a caller can never read another user's
    /// objects, and the id must be a full UUID matched exactly against the
    /// stored file stem — no prefix matching.
    pub async fn get_file(
        &self,
        file_id: &str,
        owner: Option<&str>,
    ) -> Result<(Vec<u8>, Option<FileMetadata>)> {
        // Strict id validation: rejects traversal characters and the
        // prefix/enumeration tricks a partial id would allow.
        if Uuid::parse_str(file_id).is_err() {
            anyhow::bail!("File not found: {}", file_id);
        }

        let dir = self.scope_dir(owner)?;
        let Ok(mut entries) = tokio::fs::read_dir(&dir).await else {
            anyhow::bail!("File not found: {}", file_id);
        };

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.file_stem().and_then(|stem| stem.to_str()) != Some(file_id) {
                continue;
            }

            let data = tokio::fs::read(&path).await?;
            let metadata = entry.metadata().await?;

            let extension = path
                .extension()
                .and_then(|ext| ext.to_str())
                .unwrap_or("bin");

            // Only echo back known-safe content types; everything else is
            // an opaque download.
            let content_type = mime_guess::from_path(&path)
                .first()
                .map(|mime| mime.to_string())
                .filter(|mime| SAFE_CONTENT_TYPES.contains(&mime.as_str()))
                .unwrap_or_else(|| "application/octet-stream".to_string());

            let file_metadata = FileMetadata {
                id: file_id.to_string(),
                original_name: format!("file.{}", extension),
                size: metadata.len(),
                content_type: Some(content_type),
                stored_path: path,
            };

            return Ok((data, Some(file_metadata)));
        }

        anyhow::bail!("File not found: {}", file_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn save_file_writes_bytes_and_returns_metadata() {
        let temp_dir = TempDir::new().expect("temp dir");
        let storage = FileStorage::new(temp_dir.path());

        let metadata = storage
            .save_file(
                b"hello world".to_vec(),
                "greeting.txt",
                Some("text/plain".to_string()),
                None,
            )
            .await
            .expect("save file");

        assert_eq!(metadata.original_name, "greeting.txt");
        assert_eq!(metadata.size, 11);
        assert_eq!(metadata.content_type.as_deref(), Some("text/plain"));
        assert!(metadata.stored_path.starts_with(temp_dir.path()));
        assert_eq!(
            tokio::fs::read(&metadata.stored_path)
                .await
                .expect("stored bytes"),
            b"hello world"
        );
    }

    #[tokio::test]
    async fn save_file_uses_generated_filename_inside_storage_dir() {
        let temp_dir = TempDir::new().expect("temp dir");
        let storage = FileStorage::new(temp_dir.path());

        let metadata = storage
            .save_file(b"secret".to_vec(), "../../secret.txt", None, None)
            .await
            .expect("save file");

        assert_eq!(metadata.original_name, "../../secret.txt");
        assert!(metadata.stored_path.starts_with(temp_dir.path()));
        assert_ne!(
            metadata.stored_path,
            temp_dir.path().join("../../secret.txt")
        );
        assert_eq!(
            metadata
                .stored_path
                .extension()
                .and_then(|extension| extension.to_str()),
            Some("txt")
        );
    }

    #[tokio::test]
    async fn save_file_replaces_unsafe_extensions_with_bin() {
        let temp_dir = TempDir::new().expect("temp dir");
        let storage = FileStorage::new(temp_dir.path());

        let metadata = storage
            .save_file(
                b"<script>alert(1)</script>".to_vec(),
                "evil.html",
                None,
                None,
            )
            .await
            .expect("save file");

        assert_eq!(
            metadata
                .stored_path
                .extension()
                .and_then(|extension| extension.to_str()),
            Some("bin")
        );

        // And the download side serves it as an opaque blob.
        let (_, fetched) = storage
            .get_file(&metadata.id, None)
            .await
            .expect("get file");
        assert_eq!(
            fetched.expect("metadata").content_type.as_deref(),
            Some("application/octet-stream")
        );
    }

    #[tokio::test]
    async fn get_file_reads_saved_file_by_id() {
        let temp_dir = TempDir::new().expect("temp dir");
        let storage = FileStorage::new(temp_dir.path());
        let saved = storage
            .save_file(b"download me".to_vec(), "download.txt", None, None)
            .await
            .expect("save file");

        let (data, metadata) = storage.get_file(&saved.id, None).await.expect("get file");

        assert_eq!(data, b"download me");
        let metadata = metadata.expect("file metadata");
        assert_eq!(metadata.id, saved.id);
        assert_eq!(metadata.original_name, "file.txt");
        assert_eq!(metadata.size, 11);
        assert_eq!(metadata.content_type.as_deref(), Some("text/plain"));
    }

    #[tokio::test]
    async fn get_file_returns_error_when_id_is_missing() {
        let temp_dir = TempDir::new().expect("temp dir");
        let storage = FileStorage::new(temp_dir.path());

        let error = storage
            .get_file("missing", None)
            .await
            .expect_err("missing file should be rejected");

        assert!(error.to_string().contains("File not found: missing"));
    }

    #[tokio::test]
    async fn get_file_rejects_id_prefixes() {
        let temp_dir = TempDir::new().expect("temp dir");
        let storage = FileStorage::new(temp_dir.path());
        let saved = storage
            .save_file(b"target".to_vec(), "target.txt", None, None)
            .await
            .expect("save file");

        // A truncated id must not match by prefix (the old behavior).
        let prefix = &saved.id[..8];
        assert!(storage.get_file(prefix, None).await.is_err());

        // Nor may a non-UUID id reach the filesystem at all.
        assert!(storage.get_file("..", None).await.is_err());
    }

    #[tokio::test]
    async fn get_file_enforces_owner_scope() {
        let temp_dir = TempDir::new().expect("temp dir");
        let storage = FileStorage::new(temp_dir.path());
        let saved = storage
            .save_file(b"alice data".to_vec(), "doc.txt", None, Some("alice"))
            .await
            .expect("save file");

        // The owner can read it.
        assert!(storage.get_file(&saved.id, Some("alice")).await.is_ok());

        // Another user cannot, even with a valid id.
        assert!(storage.get_file(&saved.id, Some("bob")).await.is_err());

        // Nor can the public scope.
        assert!(storage.get_file(&saved.id, None).await.is_err());
    }

    #[tokio::test]
    async fn save_file_rejects_path_like_owner_ids() {
        let temp_dir = TempDir::new().expect("temp dir");
        let storage = FileStorage::new(temp_dir.path());

        let error = storage
            .save_file(b"x".to_vec(), "x.txt", None, Some("../escape"))
            .await
            .expect_err("path-like owner must be rejected");
        assert!(error.to_string().contains("invalid owner id"));
    }
}
