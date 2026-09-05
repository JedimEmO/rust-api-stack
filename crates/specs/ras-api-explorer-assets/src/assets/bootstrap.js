        const CONFIG = JSON.parse(document.getElementById("ras-explorer-config").textContent);

        const METHODS = ["get", "post", "put", "patch", "delete", "head", "options"];
        const state = {
            spec: null,
            operations: [],
            selectedId: null,
            token: "",
            environments: [],
            activeEnvironment: 0,
            saved: {},
            history: [],
            lastResponse: { body: "", headers: "", request: "" },
            responseTab: "body"
        };
        const storagePrefix = `ras-explorer:${CONFIG.protocol}:${CONFIG.serviceName}:${location.pathname}`;

