//! Basic usage example for ras-openrpc-types

use ras_openrpc_types::{
    Components, ContentDescriptor, ContentDescriptorOrReference, Info, Method, OpenRpc,
    ParameterStructure, Schema, Validate,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create API info
    let info = Info::new("Example API", "1.0.0")
        .with_description("A simple example API demonstrating OpenRPC types");

    // Create a user schema component
    let user_schema = Schema::object()
        .with_title("User")
        .with_description("A user object")
        .with_property("id", Schema::string().with_format("uuid"))
        .with_property("name", Schema::string().with_min_length(1))
        .with_property("email", Schema::string().with_format("email"))
        .require_property("id")
        .require_property("name")
        .require_property("email");

    // Create components
    let components = Components::new().with_schema("User", user_schema);

    // Create method parameters
    let user_id_param = ContentDescriptor::new("userId", Schema::string())
        .with_description("The unique identifier for a user")
        .required();

    // Create method result
    let user_result = ContentDescriptor::new(
        "user",
        Schema::object()
            .with_property("id", Schema::string())
            .with_property("name", Schema::string())
            .with_property("email", Schema::string()),
    );

    // Create a method
    let get_user_method = Method::new(
        "getUser",
        vec![ContentDescriptorOrReference::ContentDescriptor(Box::new(
            user_id_param,
        ))],
    )
    .with_summary("Get user by ID")
    .with_description("Retrieves a user by their unique identifier")
    .with_result(ContentDescriptorOrReference::ContentDescriptor(Box::new(
        user_result,
    )))
    .with_param_structure(ParameterStructure::ByName);

    // Create the OpenRPC document
    let openrpc_doc =
        OpenRpc::new("1.3.2", info, vec![get_user_method.into()]).with_components(components);

    // Validate the document
    openrpc_doc.validate()?;

    // Serialize to JSON
    let json = serde_json::to_string_pretty(&openrpc_doc)?;

    println!("Generated OpenRPC document:");
    println!("{}", json);

    // Verify we can deserialize it back
    let _deserialized: OpenRpc = serde_json::from_str(&json)?;
    println!("\nSuccessfully round-tripped the document!");

    Ok(())
}
