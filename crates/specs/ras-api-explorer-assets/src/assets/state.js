        function activeOperation() {
            return state.operations.find((operation) => operation.id === state.selectedId) || null;
        }

        function activeBaseUrl() {
            return state.environments[state.activeEnvironment]?.baseUrl || CONFIG.apiBasePath || "";
        }

        function toAbsoluteUrl(path) {
            if (/^https?:\/\//i.test(path)) return path;
            const prefix = path.startsWith("/") ? path : `/${path}`;
            return `${window.location.origin}${prefix}`;
        }

        function currentRequestSnapshot() {
            const operation = activeOperation();
            if (!operation) return null;
            if (operation.protocol === "rest") {
                const pathValues = {};
                document.querySelectorAll("[data-path-param]").forEach((input) => pathValues[input.dataset.pathParam] = input.value);
                const queryValues = {};
                document.querySelectorAll("[data-query-param]").forEach((input) => queryValues[input.dataset.queryParam] = input.value);
                return {
                    operationId: operation.id,
                    pathValues,
                    queryValues,
                    body: $("body-editor")?.value || ""
                };
            }
            return {
                operationId: operation.id,
                requestId: $("rpc-request-id")?.value || "",
                params: $("params-editor")?.value || ""
            };
        }

        function applySnapshot(snapshot) {
            if (!snapshot) return;
            selectOperation(snapshot.operationId, false);
            if (snapshot.pathValues) {
                Object.entries(snapshot.pathValues).forEach(([key, value]) => {
                    const input = document.querySelector(`[data-path-param="${CSS.escape(key)}"]`);
                    if (input) input.value = value;
                });
            }
            if (snapshot.queryValues) {
                Object.entries(snapshot.queryValues).forEach(([key, value]) => {
                    const input = document.querySelector(`[data-query-param="${CSS.escape(key)}"]`);
                    if (input) input.value = value;
                });
            }
            if ($("body-editor") && snapshot.body !== undefined) $("body-editor").value = snapshot.body;
            if ($("params-editor") && snapshot.params !== undefined) $("params-editor").value = snapshot.params;
            if ($("rpc-request-id") && snapshot.requestId !== undefined) $("rpc-request-id").value = snapshot.requestId;
            updateRequestUrl();
        }

