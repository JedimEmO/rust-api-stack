        function jsonPretty(value) {
            if (typeof value === "string") return value;
            return JSON.stringify(value, null, 2);
        }

        function normalizePermissions(value) {
            if (!Array.isArray(value)) return [];
            if (value.every((item) => typeof item === "string")) return value;
            return value.map((item) => Array.isArray(item) ? item.join(" + ") : String(item));
        }

        function normalizeOpenApi(spec) {
            const operations = [];
            Object.entries(spec.paths || {}).forEach(([path, pathItem]) => {
                Object.entries(pathItem || {}).forEach(([method, operation]) => {
                    if (!METHODS.includes(method)) return;
                    const upper = method.toUpperCase();
                    const params = operation.parameters || [];
                    const requestSchema = operation.requestBody?.content?.["application/json"]?.schema || null;
                    const response = Object.entries(operation.responses || {}).find(([code]) => code.startsWith("2"));
                    const responseSchema = response?.[1]?.content?.["application/json"]?.schema || null;
                    operations.push({
                        id: `${upper} ${path}`,
                        protocol: "rest",
                        label: path,
                        method: upper,
                        path,
                        summary: operation.summary || `${upper} ${path}`,
                        description: operation.description || operation.summary || "",
                        authRequired: Boolean(operation.security && operation.security.length),
                        permissions: normalizePermissions(operation["x-permissions"]),
                        pathParams: params.filter((param) => param.in === "path"),
                        queryParams: params.filter((param) => param.in === "query"),
                        requestSchema,
                        responseSchema
                    });
                });
            });
            return operations;
        }

        function normalizeOpenRpc(spec) {
            return (spec.methods || []).map((method) => {
                const auth = method["x-authentication"];
                const param = Array.isArray(method.params) ? method.params[0] : null;
                return {
                    id: method.name,
                    protocol: "jsonrpc",
                    label: method.name,
                    method: "RPC",
                    path: CONFIG.apiBasePath,
                    summary: method.summary || method.name,
                    description: method.description || method.summary || "",
                    authRequired: Boolean(auth && auth.required !== false),
                    permissions: normalizePermissions(method["x-permissions"]),
                    paramsSchema: param?.schema || null,
                    responseSchema: method.result?.schema || null
                };
            });
        }

