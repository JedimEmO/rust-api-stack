        function renderResponseOutput() {
            $("response-output").textContent = state.lastResponse[state.responseTab] || "";
        }

        function saveCurrentRequest() {
            const operation = activeOperation();
            const snapshot = currentRequestSnapshot();
            if (!operation || !snapshot) return;
            const name = prompt("Saved request name", operation.label);
            if (!name) return;
            state.saved[operation.id] = state.saved[operation.id] || [];
            state.saved[operation.id].unshift({ name, snapshot, createdAt: Date.now() });
            storageSet("saved", state.saved);
            renderSaved();
        }

        async function loadSpec() {
            $("service-name").textContent = `${CONFIG.serviceName} Explorer`;
            $("service-subtitle").textContent = CONFIG.protocol === "rest" ? "REST OpenAPI" : "JSON-RPC OpenRPC";
            const response = await fetch(CONFIG.specPath, { headers: { Accept: "application/json" } });
            if (!response.ok) throw new Error(`Failed to load API specification: ${response.status}`);
            state.spec = await response.json();
            state.operations = CONFIG.protocol === "rest" ? normalizeOpenApi(state.spec) : normalizeOpenRpc(state.spec);
            renderOperations();
            if (state.operations.length) selectOperation(state.operations[0].id);
        }

        function bindEvents() {
            $("theme-toggle").addEventListener("click", () => {
                const current = document.documentElement.getAttribute("data-theme");
                setTheme(current === "dark" ? "light" : "dark");
            });
            $("operation-search").addEventListener("input", renderOperations);
            $("environment-select").addEventListener("change", (event) => {
                state.activeEnvironment = Number(event.target.value);
                storageSet("activeEnvironment", state.activeEnvironment);
                renderEnvironments();
                updateRequestUrl();
            });
            $("base-url").addEventListener("input", (event) => {
                state.environments[state.activeEnvironment].baseUrl = event.target.value;
                storageSet("environments", state.environments);
                updateRequestUrl();
            });
            $("add-environment").addEventListener("click", () => {
                const name = prompt("Environment name", `Env ${state.environments.length + 1}`);
                if (!name) return;
                state.environments.push({ name, baseUrl: activeBaseUrl() });
                state.activeEnvironment = state.environments.length - 1;
                storageSet("environments", state.environments);
                storageSet("activeEnvironment", state.activeEnvironment);
                renderEnvironments();
            });
            $("save-token").addEventListener("click", () => {
                state.token = $("bearer-token").value.trim();
                storageSet("bearer-token", state.token);
                $("auth-state").textContent = state.token ? "Token set" : "No token";
                showToast(state.token ? "Token applied for this session" : "Token cleared");
            });
            $("clear-token").addEventListener("click", () => {
                state.token = "";
                $("bearer-token").value = "";
                sessionStorage.removeItem(`${storagePrefix}:bearer-token`);
                $("auth-state").textContent = "No token";
            });
            $("send-request").addEventListener("click", sendCurrentRequest);
            $("save-request").addEventListener("click", saveCurrentRequest);
            $("clear-saved").addEventListener("click", () => {
                const operation = activeOperation();
                if (operation) {
                    state.saved[operation.id] = [];
                    storageSet("saved", state.saved);
                    renderSaved();
                }
            });
            $("clear-history").addEventListener("click", () => {
                state.history = [];
                storageSet("history", state.history);
                renderHistory();
            });
            $("copy-response").addEventListener("click", async () => {
                await navigator.clipboard.writeText($("response-output").textContent);
                showToast("Copied response");
            });
            document.querySelectorAll("[data-response-tab]").forEach((tab) => {
                tab.addEventListener("click", () => {
                    state.responseTab = tab.dataset.responseTab;
                    document.querySelectorAll("[data-response-tab]").forEach((item) => item.classList.toggle("active", item === tab));
                    renderResponseOutput();
                });
            });
        }

        document.addEventListener("DOMContentLoaded", async () => {
            initializeTheme();
            state.environments = storageGet("environments", [{ name: "Default", baseUrl: CONFIG.apiBasePath || "/" }]);
            state.activeEnvironment = storageGet("activeEnvironment", 0);
            state.saved = storageGet("saved", {});
            state.history = storageGet("history", []);
            state.token = storageGet("bearer-token", "");
            $("bearer-token").value = state.token;
            $("auth-state").textContent = state.token ? "Token set" : "No token";
            bindEvents();
            renderEnvironments();
            renderHistory();
            renderSaved();
            try {
                await loadSpec();
            } catch (error) {
                $("operation-list").textContent = "";
                const empty = document.createElement("div");
                empty.className = "empty";
                empty.textContent = error.message;
                $("operation-list").appendChild(empty);
                showToast(error.message);
            }
        });
