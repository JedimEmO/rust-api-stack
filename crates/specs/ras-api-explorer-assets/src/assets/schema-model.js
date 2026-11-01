        const SCHEMA_PREFIX = "#/components/schemas/";
        const ANNOTATION_KEYS = new Set([
            "title", "description", "default", "example", "examples",
            "deprecated", "readOnly", "writeOnly", "nullable", "$comment"
        ]);

        // A reference with annotations, which OpenAPI 3.0 and draft 7 cannot put
        // next to `$ref`: `{ "description": "...", "allOf": [{ "$ref": "..." }] }`.
        function wrappedRef(schema) {
            if (!schema || !Array.isArray(schema.allOf) || schema.allOf.length !== 1) return null;
            const inner = schema.allOf[0];
            if (!inner || typeof inner.$ref !== "string") return null;
            const annotationsOnly = Object.keys(schema).every((key) => key === "allOf" || ANNOTATION_KEYS.has(key));
            return annotationsOnly ? inner.$ref : null;
        }

        // The reference a schema stands for, direct or wrapped.
        function schemaRef(schema) {
            if (!schema) return null;
            return typeof schema.$ref === "string" ? schema.$ref : wrappedRef(schema);
        }

        function resolveRef(schema) {
            if (!schema) return null;
            const wrapped = wrappedRef(schema);
            if (wrapped) {
                const target = resolveRef({ $ref: wrapped });
                if (!target || target.$ref) return schema;
                const { allOf: _allOf, ...annotations } = schema;
                return { ...target, ...annotations };
            }
            if (!schema.$ref) return schema;
            if (schema.$ref.startsWith(SCHEMA_PREFIX)) {
                return state.spec?.components?.schemas?.[schema.$ref.slice(SCHEMA_PREFIX.length)] || schema;
            }
            return schema;
        }

        // Component names for what the server sends may carry a `.Serialize` suffix.
        function refDisplayName(ref) {
            return ref ? ref.split("/").pop().replace(/\.Serialize$/, "") : null;
        }

        function schemaType(schema) {
            const resolved = resolveRef(schema);
            if (!resolved) return "any";
            if (resolved.$ref) return refDisplayName(resolved.$ref);
            if (Array.isArray(resolved.type)) return resolved.type.filter((t) => t !== "null").join(" | ") || "null";
            if (resolved.type) return resolved.nullable ? `${resolved.type}?` : resolved.type;
            if (resolved.enum) return "enum";
            if (resolved.oneOf) return "oneOf";
            if (resolved.anyOf) return "anyOf";
            return "object";
        }

        function schemaTitle(schema) {
            const refName = refDisplayName(schemaRef(schema));
            const resolved = resolveRef(schema);
            return resolved?.title || refName || schemaType(schema);
        }

        function schemaFields(schema) {
            const resolved = resolveRef(schema);
            const properties = resolved?.properties || {};
            const required = new Set(resolved?.required || []);
            return Object.entries(properties).map(([name, prop]) => {
                const propSchema = resolveRef(prop);
                return {
                    name,
                    required: required.has(name),
                    type: schemaType(prop),
                    description: propSchema?.description || ""
                };
            });
        }

        function schemaHasDocs(schema) {
            const resolved = resolveRef(schema);
            if (!resolved) return false;
            return Boolean(resolved.description || schemaFields(schema).some((field) => field.description));
        }

