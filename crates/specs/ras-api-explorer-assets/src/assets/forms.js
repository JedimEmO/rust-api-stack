        function renderRestForm(operation) {
            const fragment = document.createDocumentFragment();
            const allParams = [
                ["Path parameters", operation.pathParams, "path"],
                ["Query parameters", operation.queryParams, "query"]
            ];
            allParams.forEach(([title, params, kind]) => {
                if (!params.length) return;
                const group = document.createElement("div");
                group.className = "field";
                const label = document.createElement("div");
                label.className = "section-title";
                label.textContent = title;
                const rows = document.createElement("div");
                rows.className = "params";
                params.forEach((param) => {
                    const row = document.createElement("div");
                    row.className = "param-row";
                    const name = document.createElement("div");
                    name.className = "name mono";
                    name.textContent = `${param.name}${param.required ? " *" : ""}`;
                    const input = document.createElement("input");
                    input.placeholder = param.description || param.name;
                    input.dataset[kind === "path" ? "pathParam" : "queryParam"] = param.name;
                    input.addEventListener("input", updateRequestUrl);
                    const type = document.createElement("span");
                    type.className = "badge";
                    type.textContent = schemaType(param.schema);
                    row.append(name, input, type);
                    rows.appendChild(row);
                });
                group.append(label, rows);
                fragment.appendChild(group);
            });
            if (operation.requestSchema) {
                fragment.appendChild(editorBlock("JSON body", "body-editor", jsonPretty(exampleFromSchema(operation.requestSchema))));
                const docs = renderSchemaDocs("Request schema", operation.requestSchema);
                if (docs) fragment.appendChild(docs);
            }
            const responseDocs = renderSchemaDocs("Response schema", operation.responseSchema);
            if (responseDocs) fragment.appendChild(responseDocs);
            return fragment;
        }

        function renderRpcForm(operation) {
            const fragment = document.createDocumentFragment();
            const grid = document.createElement("div");
            grid.className = "grid2";
            const idField = document.createElement("div");
            idField.className = "field";
            const idLabel = document.createElement("label");
            idLabel.textContent = "Request ID";
            idLabel.htmlFor = "rpc-request-id";
            const idRow = document.createElement("div");
            idRow.className = "row";
            const idInput = document.createElement("input");
            idInput.id = "rpc-request-id";
            idInput.className = "grow";
            idInput.value = requestId();
            const regen = document.createElement("button");
            regen.textContent = "Regenerate";
            regen.addEventListener("click", () => idInput.value = requestId());
            idRow.append(idInput, regen);
            idField.append(idLabel, idRow);
            const methodField = document.createElement("div");
            methodField.className = "field";
            const methodLabel = document.createElement("label");
            methodLabel.textContent = "JSON-RPC method";
            const methodValue = document.createElement("input");
            methodValue.value = operation.label;
            methodValue.readOnly = true;
            methodField.append(methodLabel, methodValue);
            grid.append(idField, methodField);
            fragment.appendChild(grid);
            if (operation.paramsSchema) {
                fragment.appendChild(editorBlock("Params", "params-editor", jsonPretty(exampleFromSchema(operation.paramsSchema))));
                const docs = renderSchemaDocs("Params schema", operation.paramsSchema);
                if (docs) fragment.appendChild(docs);
            } else {
                const empty = document.createElement("div");
                empty.className = "empty";
                empty.textContent = "This method has no params.";
                fragment.appendChild(empty);
            }
            const responseDocs = renderSchemaDocs("Result schema", operation.responseSchema);
            if (responseDocs) fragment.appendChild(responseDocs);
            return fragment;
        }

        function editorBlock(labelText, id, value) {
            const field = document.createElement("div");
            field.className = "field";
            const label = document.createElement("label");
            label.textContent = labelText;
            label.htmlFor = id;
            const editor = document.createElement("textarea");
            editor.id = id;
            editor.spellcheck = false;
            editor.value = value;
            field.append(label, editor);
            return field;
        }

        function renderRequestForm() {
            const operation = activeOperation();
            const form = $("request-form");
            form.textContent = "";
            $("send-request").disabled = !operation;
            if (!operation) {
                const empty = document.createElement("div");
                empty.className = "empty";
                empty.textContent = "No operation selected.";
                form.appendChild(empty);
                return;
            }
            const auth = document.createElement("div");
            auth.className = "row";
            const authBadge = document.createElement("span");
            authBadge.className = operation.authRequired ? "badge lock" : "badge open";
            authBadge.textContent = operation.authRequired ? "Authentication required" : "No authentication required";
            auth.appendChild(authBadge);
            operation.permissions.forEach((permission) => {
                const badge = document.createElement("span");
                badge.className = "badge";
                badge.textContent = permission;
                auth.appendChild(badge);
            });
            form.appendChild(auth);
            form.appendChild(operation.protocol === "rest" ? renderRestForm(operation) : renderRpcForm(operation));
            updateRequestUrl();
        }

