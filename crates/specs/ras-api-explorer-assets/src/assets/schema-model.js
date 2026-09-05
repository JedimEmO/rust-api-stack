        function resolveRef(schema) {
            if (!schema || !schema.$ref) return schema || null;
            const prefix = "#/components/schemas/";
            if (schema.$ref.startsWith(prefix)) {
                return state.spec?.components?.schemas?.[schema.$ref.slice(prefix.length)] || schema;
            }
            return schema;
        }

        function schemaType(schema) {
            const resolved = resolveRef(schema);
            if (!resolved) return "any";
            if (resolved.$ref) return resolved.$ref.split("/").pop();
            if (Array.isArray(resolved.type)) return resolved.type.filter((t) => t !== "null").join(" | ") || "null";
            if (resolved.type) return resolved.nullable ? `${resolved.type}?` : resolved.type;
            if (resolved.enum) return "enum";
            if (resolved.oneOf) return "oneOf";
            if (resolved.anyOf) return "anyOf";
            return "object";
        }

        function schemaTitle(schema) {
            const refName = schema?.$ref?.split("/").pop();
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

