import { copyFileSync, mkdirSync } from "node:fs";
import rust from "@wasm-tool/rollup-plugin-rust";

function copyIndexHtml() {
    return {
        name: "copy-index-html",
        writeBundle() {
            mkdirSync("dist", { recursive: true });
            copyFileSync("index.html", "dist/index.html");
        },
    };
}

export default {
    input: {
        bundle: "Cargo.toml",
    },
    output: {
        dir: "dist/js",
        format: "es",
        sourcemap: true,
    },
    plugins: [
        rust({
            optimize: {release: false},
            extraArgs: {
                cargo: ["--config", "profile.dev.debug=true"],
                wasmBindgen: ["--debug", "--keep-debug"]
            },
        }),
        copyIndexHtml(),
    ],
};
