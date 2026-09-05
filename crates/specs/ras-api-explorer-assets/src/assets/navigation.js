        function renderOperations() {
            const query = $("operation-search").value.trim().toLowerCase();
            const list = $("operation-list");
            list.textContent = "";
            state.operations
                .filter((operation) => `${operation.method} ${operation.label} ${operation.summary}`.toLowerCase().includes(query))
                .forEach((operation) => {
                    const button = document.createElement("button");
                    button.className = `op ${operation.id === state.selectedId ? "active" : ""}`;
                    button.type = "button";
                    button.addEventListener("click", () => selectOperation(operation.id));

                    const main = document.createElement("div");
                    main.className = "op-main";
                    const method = document.createElement("span");
                    method.className = `badge ${operation.method.toLowerCase()}`;
                    method.textContent = operation.method;
                    const name = document.createElement("span");
                    name.className = "op-name mono";
                    name.textContent = operation.label;
                    const auth = document.createElement("span");
                    auth.className = operation.authRequired ? "badge lock" : "badge open";
                    auth.textContent = operation.authRequired ? "Auth" : "Open";
                    main.append(method, name, auth);

                    const desc = document.createElement("div");
                    desc.className = "op-desc";
                    desc.textContent = operation.summary || operation.description || "";
                    button.append(main, desc);
                    list.appendChild(button);
                });
            if (!list.children.length) {
                const empty = document.createElement("div");
                empty.className = "empty";
                empty.textContent = "No matching operations.";
                list.appendChild(empty);
            }
        }

        function renderEnvironments() {
            const select = $("environment-select");
            select.textContent = "";
            state.environments.forEach((env, index) => {
                const option = document.createElement("option");
                option.value = String(index);
                option.textContent = env.name;
                select.appendChild(option);
            });
            select.value = String(state.activeEnvironment);
            $("base-url").value = activeBaseUrl();
        }

        function renderSaved() {
            const container = $("saved-list");
            container.textContent = "";
            const operation = activeOperation();
            const items = operation ? (state.saved[operation.id] || []) : [];
            if (!items.length) {
                const empty = document.createElement("div");
                empty.className = "empty";
                empty.textContent = operation ? "No saved requests for this operation." : "Select an operation.";
                container.appendChild(empty);
                return;
            }
            items.forEach((item, index) => {
                const row = document.createElement("div");
                row.className = "saved-item";
                const name = document.createElement("strong");
                name.textContent = item.name;
                const time = document.createElement("span");
                time.className = "hint";
                time.textContent = new Date(item.createdAt).toLocaleString();
                const load = document.createElement("button");
                load.textContent = "Load";
                load.addEventListener("click", () => applySnapshot(item.snapshot));
                const remove = document.createElement("button");
                remove.textContent = "Remove";
                remove.addEventListener("click", () => {
                    state.saved[operation.id].splice(index, 1);
                    storageSet("saved", state.saved);
                    renderSaved();
                });
                const actions = document.createElement("div");
                actions.className = "row";
                actions.append(load, remove);
                row.append(name, time, actions);
                container.appendChild(row);
            });
        }

        function renderHistory() {
            const container = $("history-list");
            container.textContent = "";
            if (!state.history.length) {
                const empty = document.createElement("div");
                empty.className = "empty";
                empty.textContent = "Requests you send in this session appear here.";
                container.appendChild(empty);
                return;
            }
            state.history.forEach((item) => {
                const row = document.createElement("div");
                row.className = "history-item";
                const title = document.createElement("strong");
                title.textContent = item.title;
                const meta = document.createElement("span");
                meta.className = "hint";
                meta.textContent = `${item.status} - ${item.duration}ms - ${new Date(item.createdAt).toLocaleTimeString()}`;
                const load = document.createElement("button");
                load.textContent = "Load request";
                load.addEventListener("click", () => applySnapshot(item.snapshot));
                row.append(title, meta, load);
                container.appendChild(row);
            });
        }

