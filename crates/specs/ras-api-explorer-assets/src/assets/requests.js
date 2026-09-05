        function selectOperation(id, rerenderList = true) {
            state.selectedId = id;
            const operation = activeOperation();
            $("operation-title").textContent = operation ? `${operation.method} ${operation.label}` : "Select an operation";
            renderMarkdownInto(
                $("operation-description"),
                operation?.description || operation?.summary || "Prepare and send a request."
            );
            renderRequestForm();
            renderSaved();
            if (rerenderList) renderOperations();
        }

        function requestId() {
            if (globalThis.crypto?.randomUUID) return crypto.randomUUID();
            return `req_${Date.now()}_${Math.random().toString(36).slice(2, 8)}`;
        }

        function updateRequestUrl() {
            const operation = activeOperation();
            if (!operation) {
                $("request-url").textContent = "";
                return;
            }
            $("request-url").textContent = buildRequestPreview(operation);
        }

        function buildRequestPreview(operation) {
            if (operation.protocol === "jsonrpc") return toAbsoluteUrl(activeBaseUrl());
            let path = operation.path;
            document.querySelectorAll("[data-path-param]").forEach((input) => {
                path = path.replace(`{${input.dataset.pathParam}}`, encodeURIComponent(input.value || `{${input.dataset.pathParam}}`));
            });
            const query = new URLSearchParams();
            document.querySelectorAll("[data-query-param]").forEach((input) => {
                if (input.value) query.append(input.dataset.queryParam, input.value);
            });
            const base = activeBaseUrl().replace(/\/$/, "");
            return toAbsoluteUrl(`${base}${path}${query.toString() ? `?${query}` : ""}`);
        }

        function buildRequest(operation) {
            const headers = { "Content-Type": "application/json" };
            if (state.token) headers.Authorization = `Bearer ${state.token}`;
            if (operation.protocol === "rest") {
                const options = { method: operation.method, headers };
                const body = $("body-editor")?.value.trim();
                if (body) options.body = JSON.stringify(JSON.parse(body));
                return { url: buildRequestPreview(operation), options, requestBody: body || "" };
            }
            const payload = {
                jsonrpc: "2.0",
                method: operation.label,
                id: $("rpc-request-id")?.value || requestId()
            };
            const params = $("params-editor")?.value.trim();
            if (params) payload.params = JSON.parse(params);
            return {
                url: toAbsoluteUrl(activeBaseUrl()),
                options: { method: "POST", headers, body: JSON.stringify(payload) },
                requestBody: JSON.stringify(payload, null, 2)
            };
        }

        async function sendCurrentRequest() {
            const operation = activeOperation();
            if (!operation) return;
            const button = $("send-request");
            button.disabled = true;
            button.textContent = "Sending";
            const started = performance.now();
            try {
                const request = buildRequest(operation);
                const response = await fetch(request.url, request.options);
                const duration = Math.round(performance.now() - started);
                const text = await response.text();
                let body = text;
                try { body = JSON.parse(text); } catch (_) {}
                const headers = Object.fromEntries(response.headers.entries());
                const isRpcError = operation.protocol === "jsonrpc" && body && body.error;
                const statusText = `${response.status} ${response.statusText || ""}`.trim();
                state.lastResponse = {
                    body: jsonPretty(body),
                    headers: jsonPretty(headers),
                    request: jsonPretty({
                        url: request.url,
                        method: request.options.method,
                        headers: request.options.headers,
                        body: request.requestBody ? JSON.parse(request.requestBody) : undefined
                    })
                };
                $("response-status").className = `status ${response.ok && !isRpcError ? "ok" : response.status < 500 ? "warn" : "err"}`;
                $("response-status").textContent = isRpcError ? "RPC error" : statusText;
                $("response-meta").textContent = `${operation.method} ${operation.label} - ${duration}ms`;
                state.history.unshift({
                    title: `${operation.method} ${operation.label}`,
                    status: isRpcError ? "RPC error" : statusText,
                    duration,
                    createdAt: Date.now(),
                    snapshot: currentRequestSnapshot()
                });
                state.history = state.history.slice(0, 30);
                storageSet("history", state.history);
                renderHistory();
                renderResponseOutput();
            } catch (error) {
                $("response-status").className = "status err";
                $("response-status").textContent = "Failed";
                state.lastResponse = { body: error.message, headers: "", request: "" };
                renderResponseOutput();
                showToast(error.message);
            } finally {
                button.disabled = false;
                button.textContent = "Send";
            }
        }

