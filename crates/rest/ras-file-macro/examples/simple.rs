use ras_file_macro::file_service;

file_service!({
    service_name: SimpleService,
    base_path: "/api",
    endpoints: [
        UPLOAD UNAUTHORIZED upload multipart {
            max_total_bytes: 1024,
            parts: [
                file file {
                    required: true,
                    max_bytes: 1024,
                    filename: optional,
                },
            ],
        } -> (),
    ]
});

fn main() {
    println!("Compiled!");
}
