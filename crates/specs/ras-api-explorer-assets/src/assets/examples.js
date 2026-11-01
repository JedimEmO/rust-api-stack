        // Authored examples, normalized to one shape for both protocols:
        // { name, summary, description, params | path/query/body, outcome }
        // where outcome is { kind: "result" | "error", status?, value }.

        function firstLine(text) {
            return (text || "").split("\n")[0];
        }

        // The whole request a pairing describes: an object for by-name methods,
        // the single value for raw ones, nothing for methods without params.
        function rpcExampleParams(method, params) {
            if (!method["x-ras-params-schema"]) return undefined;
            const list = Array.isArray(params) ? params : [];
            if (method["x-ras-params-raw"]) return list[0]?.value;
            const value = {};
            list.forEach((param) => { value[param.name] = param.value; });
            return value;
        }

        function normalizeRpcExamples(method) {
            const examples = [];
            (method.examples || []).forEach((example) => {
                examples.push({
                    name: example.name,
                    summary: firstLine(example.description),
                    description: example.description || "",
                    params: rpcExampleParams(method, example.params),
                    outcome: { kind: "result", value: example.result?.value }
                });
            });
            (method["x-ras-error-examples"] || []).forEach((example) => {
                examples.push({
                    name: example.name,
                    summary: firstLine(example.description),
                    description: example.description || "",
                    params: rpcExampleParams(method, example.params),
                    outcome: { kind: "error", value: example.error }
                });
            });
            return examples;
        }

        // OpenAPI 3.0 has no request/response pairs: an example sits under the
        // same key on its parameters, request body and response.
        function normalizeRestExamples(operation) {
            const byName = new Map();
            const entry = (name, example) => {
                if (!byName.has(name)) {
                    byName.set(name, { name, summary: "", description: "", path: {}, query: {}, body: undefined, outcome: null });
                }
                const found = byName.get(name);
                if (!found.summary && example.summary) found.summary = example.summary;
                if (!found.description && example.description) found.description = example.description;
                return found;
            };
            (operation.parameters || []).forEach((param) => {
                Object.entries(param.examples || {}).forEach(([name, example]) => {
                    const found = entry(name, example);
                    if (param.in === "path") found.path[param.name] = example.value;
                    if (param.in === "query") found.query[param.name] = example.value;
                });
            });
            const bodyExamples = operation.requestBody?.content?.["application/json"]?.examples || {};
            Object.entries(bodyExamples).forEach(([name, example]) => {
                entry(name, example).body = example.value;
            });
            Object.entries(operation.responses || {}).forEach(([status, response]) => {
                const examples = response?.content?.["application/json"]?.examples || response?.["x-ras-examples"] || {};
                Object.entries(examples).forEach(([name, example]) => {
                    entry(name, example).outcome = {
                        kind: status.startsWith("2") ? "result" : "error",
                        status,
                        value: example.value
                    };
                });
            });
            return [...byName.values()];
        }

        function queryInputValue(value) {
            if (value === undefined || value === null) return "";
            return typeof value === "string" ? value : JSON.stringify(value);
        }

        function expectedText(example) {
            const outcome = example.outcome;
            if (!outcome) return "";
            const status = outcome.status ? `${outcome.status} ` : "";
            const label = outcome.kind === "error" ? "Error" : "Result";
            const noBody = outcome.value === undefined || (outcome.value === null && outcome.status === "204");
            const value = noBody ? "(no body)" : jsonPretty(outcome.value);
            return `${status}${label}\n${value}`;
        }

        // Fills the editors from an example. A field the example leaves out is
        // cleared, so nothing from an earlier example or guess is sent with it.
        function applyExample(operation, example) {
            if (operation.protocol === "jsonrpc") {
                const editor = $("params-editor");
                if (editor) editor.value = example.params === undefined ? "" : jsonPretty(example.params);
            } else {
                document.querySelectorAll("[data-path-param]").forEach((input) => {
                    input.value = queryInputValue(example.path[input.dataset.pathParam]);
                });
                document.querySelectorAll("[data-query-param]").forEach((input) => {
                    input.value = queryInputValue(example.query[input.dataset.queryParam]);
                });
                const body = $("body-editor");
                if (body) body.value = example.body === undefined ? "" : jsonPretty(example.body);
            }
            const expected = $("expected-output");
            if (expected) {
                expected.textContent = expectedText(example);
                expected.dataset.example = example.name;
            }
            document.querySelectorAll("[data-example-name]").forEach((row) => {
                row.classList.toggle("active", row.dataset.exampleName === example.name);
            });
            updateRequestUrl();
        }

        function renderExamples(operation) {
            if (!operation.examples.length) return null;
            const block = document.createElement("div");
            block.className = "field examples";
            const title = document.createElement("div");
            title.className = "section-title";
            title.textContent = "Examples";
            block.appendChild(title);

            operation.examples.forEach((example) => {
                const row = document.createElement("div");
                row.className = "example-row";
                row.dataset.exampleName = example.name;
                const head = document.createElement("div");
                head.className = "row";
                const name = document.createElement("span");
                name.className = "mono";
                name.textContent = example.name;
                const kind = document.createElement("span");
                kind.className = example.outcome?.kind === "error" ? "badge lock" : "badge open";
                kind.textContent = example.outcome?.status
                    ? example.outcome.status
                    : (example.outcome?.kind === "error" ? "error" : "result");
                const use = document.createElement("button");
                use.textContent = "Use";
                use.setAttribute("aria-label", `Use example ${example.name}`);
                use.addEventListener("click", () => applyExample(operation, example));
                head.append(name, kind, use);
                row.appendChild(head);
                if (example.description) {
                    const description = document.createElement("div");
                    description.className = "schema-desc";
                    renderMarkdownInto(description, example.description);
                    row.appendChild(description);
                }
                block.appendChild(row);
            });

            const expectedLabel = document.createElement("div");
            expectedLabel.className = "section-title";
            expectedLabel.textContent = "Expected response";
            const expected = document.createElement("pre");
            expected.id = "expected-output";
            expected.className = "mono";
            block.append(expectedLabel, expected);
            return block;
        }

        // An operation with examples opens with its first success example.
        function applyDefaultExample(operation) {
            const first = operation.examples.find((example) => example.outcome?.kind === "result");
            if (first) applyExample(operation, first);
        }
