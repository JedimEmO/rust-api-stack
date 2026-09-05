        const $ = (id) => document.getElementById(id);

        function storageGet(key, fallback) {
            try {
                const value = sessionStorage.getItem(`${storagePrefix}:${key}`);
                return value ? JSON.parse(value) : fallback;
            } catch (_) {
                return fallback;
            }
        }

        function storageSet(key, value) {
            sessionStorage.setItem(`${storagePrefix}:${key}`, JSON.stringify(value));
        }

        function showToast(message) {
            const toast = $("toast");
            toast.textContent = message;
            toast.classList.add("show");
            setTimeout(() => toast.classList.remove("show"), 2200);
        }

        function setTheme(theme) {
            const next = theme === "light" ? "light" : "dark";
            document.documentElement.setAttribute("data-theme", next);
            localStorage.setItem("ras-explorer-theme", next);
        }

        function initializeTheme() {
            setTheme(localStorage.getItem("ras-explorer-theme") || "dark");
        }

