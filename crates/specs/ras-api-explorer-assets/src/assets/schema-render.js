        function exampleFromSchema(schema, seen = new Set()) {
            const resolved = resolveRef(schema);
            if (!resolved) return {};
            if (resolved.$ref) {
                if (seen.has(resolved.$ref)) return {};
                seen.add(resolved.$ref);
                return exampleFromSchema(resolveRef(resolved), seen);
            }
            if (resolved.example !== undefined) return resolved.example;
            if (Array.isArray(resolved.examples) && resolved.examples.length) return resolved.examples[0];
            if (resolved.default !== undefined) return resolved.default;
            if (Array.isArray(resolved.enum) && resolved.enum.length) return resolved.enum[0];
            const variants = resolved.oneOf || resolved.anyOf;
            if (Array.isArray(variants) && variants.length) {
                return exampleFromSchema(variants.find((item) => item.type !== "null") || variants[0], seen);
            }
            const type = Array.isArray(resolved.type) ? resolved.type.find((item) => item !== "null") : resolved.type;
            if (type === "string") return "example";
            if (type === "integer" || type === "number") return 0;
            if (type === "boolean") return false;
            if (type === "array") return [exampleFromSchema(resolved.items, seen)];
            if (type === "object" || resolved.properties) {
                const output = {};
                Object.entries(resolved.properties || {}).forEach(([key, prop]) => {
                    output[key] = exampleFromSchema(prop, seen);
                });
                return output;
            }
            return {};
        }

        function renderSchemaDocs(title, schema) {
            if (!schemaHasDocs(schema)) return null;

            const resolved = resolveRef(schema);
            const docs = document.createElement("div");
            docs.className = "schema-docs";
            const section = document.createElement("div");
            section.className = "section-title";
            section.textContent = title;
            const head = document.createElement("div");
            head.className = "schema-head";
            const name = document.createElement("strong");
            name.textContent = schemaTitle(schema);
            const type = document.createElement("span");
            type.className = "badge";
            type.textContent = schemaType(schema);
            head.append(name, type);
            docs.append(section, head);

            if (resolved?.description) {
                const description = document.createElement("div");
                description.className = "schema-desc";
                renderMarkdownInto(description, resolved.description);
                docs.appendChild(description);
            }

            const fields = schemaFields(schema);
            if (fields.length) {
                const rows = document.createElement("div");
                rows.className = "schema-fields";
                fields.forEach((field) => {
                    const row = document.createElement("div");
                    row.className = "schema-field";
                    const fieldName = document.createElement("div");
                    fieldName.className = "mono";
                    fieldName.textContent = `${field.name}${field.required ? " *" : ""}`;
                    const fieldType = document.createElement("span");
                    fieldType.className = "badge";
                    fieldType.textContent = field.type;
                    const fieldDescription = document.createElement("div");
                    fieldDescription.className = "schema-field-desc";
                    renderMarkdownInto(fieldDescription, field.description || "");
                    row.append(fieldName, fieldType, fieldDescription);
                    rows.appendChild(row);
                });
                docs.appendChild(rows);
            }

            return docs;
        }

