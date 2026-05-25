use ras_file_macro::file_service;

file_service!({
    service_name: DownloadOnly,
    base_path: "/files",
    endpoints: [
        DOWNLOAD UNAUTHORIZED nested/{folder: String}/download/{id: String} {
            content_types: ["application/octet-stream"],
            ranges: false,
        },
    ]
});

#[test]
fn path_struct_name_tracks_nested_download_path() {
    let path = DownloadOnlyNestedByFolderDownloadByIdPath {
        folder: "a".to_string(),
        id: "b".to_string(),
    };
    assert_eq!(path.folder, "a");
    assert_eq!(path.id, "b");
}
